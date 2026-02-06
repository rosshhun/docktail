use async_graphql::{Context, Result, Subscription};
use futures::{Stream, StreamExt};
use std::sync::Arc;

use crate::state::AppState;
use crate::error::ApiError;
use crate::graphql::types::log::{LogEntry, LogStreamOptions};
use crate::graphql::types::agent::{AgentHealthEvent, AgentStatus, MetadataEntry};
use crate::graphql::types::stats::ContainerStats;
use crate::agent::client::{LogStreamRequest, HealthCheckRequest, ContainerStatsRequest};
use crate::metrics::SubscriptionMetrics;

/// RAII guard that ensures subscription_ended is called when the stream is dropped,
/// even on abrupt client disconnects.
struct SubscriptionGuard {
    metrics: Arc<SubscriptionMetrics>,
    agent_id: String,
}

impl Drop for SubscriptionGuard {
    fn drop(&mut self) {
        self.metrics.subscription_ended(&self.agent_id);
        tracing::debug!(agent_id = %self.agent_id, "Subscription guard dropped, metrics updated");
    }
}

/// Root subscription type
pub struct SubscriptionRoot;

#[Subscription]
impl SubscriptionRoot {
    /// Stream logs from a single container in real-time
    /// 
    /// # Arguments
    /// * `container_id` - The container ID to stream logs from
    /// * `agent_id` - The agent ID where the container is running
    /// * `options` - Optional streaming options (filters, follow mode, etc.)
    /// 
    /// # Example
    /// ```graphql
    /// subscription {
    ///   logStream(
    ///     containerId: "abc123"
    ///     agentId: "agent-local"
    ///     options: {
    ///       follow: true
    ///       tail: 50
    ///       filter: "ERROR"
    ///       filterMode: INCLUDE
    ///     }
    ///   ) {
    ///     timestamp
    ///     level
    ///     content
    ///     sequence
    ///   }
    /// }
    /// ```
    async fn log_stream(
        &self,
        ctx: &Context<'_>,
        container_id: String,
        agent_id: String,
        options: Option<LogStreamOptions>,
    ) -> Result<impl Stream<Item = Result<LogEntry>>> {
        let state = ctx.data::<AppState>()?;
        
        // Track subscription metrics
        state.metrics.subscription_started(&agent_id);
        let metrics = state.metrics.clone();
        
        // Create a RAII guard that will call subscription_ended when the stream is dropped.
        // This works even on abrupt client disconnects, unlike the previous chain approach.
        let guard = Arc::new(SubscriptionGuard {
            metrics: metrics.clone(),
            agent_id: agent_id.clone(),
        });
        
        // Get agent connection
        let agent_conn = state
            .agent_pool
            .get_agent(&agent_id)
            .ok_or_else(|| {
                state.metrics.subscription_failed();
                ApiError::AgentNotFound(agent_id.clone()).extend()
            })?;
        
        // Check agent health
        if !agent_conn.is_healthy() {
            state.metrics.subscription_failed();
            return Err(ApiError::AgentUnavailable(format!(
                "Agent '{}' is not healthy. Try again later or check agent status.",
                agent_id
            )).extend());
        }
        
        // Default options with follow=true for subscriptions
        let opts = options.unwrap_or(LogStreamOptions {
            since: None,
            until: None,
            tail: Some(50),
            follow: true,  // Always follow for subscriptions
            filter: None,
            filter_mode: crate::graphql::types::log::FilterMode::None,
            timestamps: true,
        });
        
        // Build gRPC request
        let request = LogStreamRequest {
            container_id: container_id.clone(),
            since: opts.since.map(|dt| dt.timestamp()),
            until: opts.until.map(|dt| dt.timestamp()),
            tail_lines: opts.tail.and_then(|t| if t > 0 { Some(t as u32) } else { None }),
            follow: opts.follow,
            filter_pattern: opts.filter.clone(),
            filter_mode: {
                let proto_mode: crate::agent::client::FilterMode = opts.filter_mode.into();
                proto_mode as i32
            },
            timestamps: opts.timestamps,
            disable_parsing: false,  // Enable parsing by default
        };
        
        // ⚡ FIX 1: Clone client to release lock immediately
        let mut client = {
            let guard = agent_conn.client.lock().await;
            guard.clone()
        };
        
        // Get gRPC client and open stream
        let grpc_stream = client
            .stream_logs(request)
            .await
            .map_err(|e| {
                metrics.subscription_failed();
                ApiError::Internal(format!("Failed to open log stream: {}. Check agent logs for details.", e)).extend()
            })?;
        
        // Clone metrics for use in stream closure
        let metrics_for_stream = metrics.clone();
        
        // Convert gRPC stream to GraphQL stream with metrics tracking.
        // The guard is moved into the stream closure; when the stream is dropped
        // (client disconnect, error, or normal completion), the guard's Drop
        // implementation calls subscription_ended automatically.
        let log_stream = grpc_stream
            .map(move |result| {
                // Keep guard alive as long as the stream is alive
                let _guard = &guard;
                match result {
                    Ok(response) => {
                        // Track message sent
                        let byte_count = response.raw_content.len();
                        metrics_for_stream.message_sent(byte_count);
                        
                        // Convert proto response to LogEntry
                        LogEntry::from_proto(response, agent_id.clone())
                    }
                    Err(e) => {
                        // Let errors bubble up to frontend so they know why connection closed
                        Err(ApiError::Internal(format!("Stream error: {}", e)).extend())
                    }
                }
            });
        
        Ok(log_stream)
    }
    
    /// Stream logs from multiple containers across multiple agents, aggregated and sorted by timestamp
    /// 
    /// # Arguments
    /// * `containers` - List of container sources (container ID + agent ID pairs)
    /// * `options` - Optional streaming options (filters, follow mode, etc.)
    /// 
    /// # Example
    /// ```graphql
    /// subscription {
    ///   logsFromContainers(
    ///     containers: [
    ///       { containerId: "abc123", agentId: "agent-1" }
    ///       { containerId: "def456", agentId: "agent-2" }
    ///     ]
    ///     options: { follow: true }
    ///   ) {
    ///     containerId
    ///     agentId
    ///     timestamp
    ///     level
    ///     content
    ///   }
    /// }
    /// ```
    async fn logs_from_containers(
        &self,
        ctx: &Context<'_>,
        containers: Vec<crate::graphql::types::log::ContainerSource>,
        options: Option<LogStreamOptions>,
    ) -> Result<impl Stream<Item = Result<LogEntry>>> {
        let state = ctx.data::<AppState>()?;
        
        if containers.is_empty() {
            return Err(ApiError::InvalidRequest("At least one container is required".to_string()).extend());
        }

        // Limit the number of concurrent container streams to prevent resource exhaustion
        const MAX_CONTAINER_STREAMS: usize = 20;
        if containers.len() > MAX_CONTAINER_STREAMS {
            return Err(ApiError::InvalidRequest(format!(
                "Too many containers requested ({}). Maximum is {}",
                containers.len(),
                MAX_CONTAINER_STREAMS
            )).extend());
        }

        // Track subscription metrics for each container source
        let mut guards = Vec::new();
        for cs in &containers {
            state.metrics.subscription_started(&cs.agent_id);
            guards.push(Arc::new(SubscriptionGuard {
                metrics: state.metrics.clone(),
                agent_id: cs.agent_id.clone(),
            }));
        }
        
        // Default options with follow=true for subscriptions
        let opts = options.unwrap_or(LogStreamOptions {
            since: None,
            until: None,
            tail: Some(50),
            follow: true,
            filter: None,
            filter_mode: crate::graphql::types::log::FilterMode::None,
            timestamps: true,
        });
        
        // Open a stream for each container (potentially across multiple agents)
        let mut streams = Vec::new();
        let mut failed_containers = Vec::new();
        
        for container_source in containers {
            let container_id = container_source.container_id.clone();
            let agent_id = container_source.agent_id.clone();
            
            // Get agent connection
            let agent_conn = match state.agent_pool.get_agent(&agent_id) {
                Some(conn) => conn,
                None => {
                    tracing::warn!("Agent '{}' not found, skipping container '{}'", agent_id, container_id);
                    failed_containers.push((container_id, agent_id, "Agent not found".to_string()));
                    continue;
                }
            };
            
            // Check agent health (but continue with others if this one is down)
            if !agent_conn.is_healthy() {
                tracing::warn!("Agent '{}' is not healthy, skipping container '{}'", agent_id, container_id);
                failed_containers.push((container_id, agent_id, "Agent not healthy".to_string()));
                continue;
            }
            
            let request = LogStreamRequest {
                container_id: container_id.clone(),
                since: opts.since.map(|dt| dt.timestamp()),
                until: opts.until.map(|dt| dt.timestamp()),
                tail_lines: opts.tail.and_then(|t| if t > 0 { Some(t as u32) } else { None }),
                follow: opts.follow,
                filter_pattern: opts.filter.clone(),
                filter_mode: {
                    let proto_mode: crate::agent::client::FilterMode = opts.filter_mode.into();
                    proto_mode as i32
                },
                timestamps: opts.timestamps,
                disable_parsing: false,  // Enable parsing by default
            };
            
            // ⚡ FIX 1: Clone client to release lock immediately
            let mut client = {
                let guard = agent_conn.client.lock().await;
                guard.clone()
            };
            
            // Try to open stream from this agent
            match client.stream_logs(request).await {
                Ok(grpc_stream) => {
                    // Clone agent_id for use in the closure and after
                    let agent_id_for_stream = agent_id.clone();
                    let container_id_for_log = container_id.clone();
                    
                    // Convert gRPC stream to LogEntry stream
                    // ⚡ No timeout - let errors bubble up naturally
                    let log_stream = grpc_stream.map(move |result| match result {
                        Ok(response) => {
                            LogEntry::from_proto(response, agent_id_for_stream.clone())
                        }
                        Err(e) => Err(ApiError::Internal(format!("Stream error: {}", e)).extend()),
                    });
                    
                    streams.push(Box::pin(log_stream));
                    tracing::info!("Opened log stream for container '{}' on agent '{}'", container_id_for_log, agent_id);
                }
                Err(e) => {
                    tracing::warn!("Failed to open log stream for container '{}' on agent '{}': {}", container_id, agent_id, e);
                    failed_containers.push((container_id, agent_id, format!("Stream open failed: {}", e)));
                    continue;
                }
            }
        }
        
        // If all containers failed, return error
        if streams.is_empty() {
            let error_msg = failed_containers
                .iter()
                .map(|(cid, aid, err)| format!("{}@{}: {}", cid, aid, err))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(ApiError::Internal(format!(
                "Failed to open any log streams. Errors: {}",
                error_msg
            )).extend());
        }
        
        // Log warnings if some containers failed
        if !failed_containers.is_empty() {
            tracing::warn!(
                "Streaming from {}/{} containers (failed: {:?})",
                streams.len(),
                streams.len() + failed_containers.len(),
                failed_containers
            );
        }
        
        // Merge all streams using select_all (interleaves items as they arrive)
        // ⚡ FIX 2: No timeout on stream items - quiet containers are normal
        // The brilliant ready_chunks(10) + flat_map provides rough timestamp ordering
        // without buffering thousands of lines or creating head-of-line blocking
        let merged_stream = futures::stream::select_all(streams)
            .ready_chunks(10)
            .flat_map(|mut chunk| {
                // Sort by timestamp within each chunk
                chunk.sort_by(|a, b| {
                    match (a, b) {
                        (Ok(entry_a), Ok(entry_b)) => entry_a.timestamp.cmp(&entry_b.timestamp),
                        _ => std::cmp::Ordering::Equal,
                    }
                });
                futures::stream::iter(chunk)
            })
            // Keep guards alive for the lifetime of the stream.
            // When the stream is dropped, all guards are dropped and metrics updated.
            .map(move |item| {
                let _guards = &guards;
                item
            });
        
        Ok(merged_stream)
    }

    /// Stream real-time health status from an agent
    /// 
    /// # Arguments
    /// * `agent_id` - The agent ID to monitor
    /// 
    /// # Example
    /// ```graphql
    /// subscription {
    ///   agentHealthStream(agentId: "agent-local") {
    ///     agentId
    ///     status
    ///     message
    ///     timestamp
    ///     metadata {
    ///       key
    ///       value
    ///     }
    ///   }
    /// }
    /// ```
    async fn agent_health_stream(
        &self,
        ctx: &Context<'_>,
        agent_id: String,
    ) -> Result<impl Stream<Item = Result<AgentHealthEvent>>> {
        let state = ctx.data::<AppState>()?;
        
        // Track subscription metrics with RAII guard
        state.metrics.subscription_started(&agent_id);
        let guard = Arc::new(SubscriptionGuard {
            metrics: state.metrics.clone(),
            agent_id: agent_id.clone(),
        });
        
        // Get agent connection
        let agent_conn = state
            .agent_pool
            .get_agent(&agent_id)
            .ok_or_else(|| {
                state.metrics.subscription_failed();
                ApiError::AgentNotFound(agent_id.clone()).extend()
            })?;
        
        // Clone client to release lock immediately
        let mut client = {
            let guard = agent_conn.client.lock().await;
            guard.clone()
        };
        
        // Build gRPC request
        let request = HealthCheckRequest {
            service: String::new(), // Empty = overall agent health
        };
        
        // Open health watch stream
        let grpc_stream = client
            .watch_health(request)
            .await
            .map_err(|e| {
                state.metrics.subscription_failed();
                ApiError::Internal(format!("Failed to open health stream: {}", e)).extend()
            })?;
        
        // Convert gRPC stream to GraphQL stream.
        // The guard is moved into the closure to track metrics on disconnect.
        let agent_id_clone = agent_id.clone();
        let health_stream = grpc_stream.map(move |result| {
            let _guard = &guard;
            match result {
            Ok(response) => {
                // Convert proto health status to GraphQL AgentStatus
                let status = match response.status {
                    1 => AgentStatus::Healthy,
                    2 => AgentStatus::Unhealthy,
                    3 => AgentStatus::Degraded,
                    _ => AgentStatus::Unknown,
                };
                
                // Convert metadata map to vec of entries
                let metadata: Vec<MetadataEntry> = response
                    .metadata
                    .into_iter()
                    .map(|(key, value)| MetadataEntry { key, value })
                    .collect();
                
                Ok(AgentHealthEvent {
                    agent_id: agent_id_clone.clone(),
                    status,
                    message: response.message,
                    timestamp: response.timestamp,
                    metadata,
                })
            }
            Err(e) => Err(ApiError::Internal(format!("Health stream error: {}", e)).extend()),
            }
        });
        
        Ok(health_stream)
    }

    /// Stream real-time resource statistics for a container
    /// 
    /// # Arguments
    /// * `container_id` - The container ID to monitor
    /// * `agent_id` - The agent ID where the container is running
    /// 
    /// # Example
    /// ```graphql
    /// subscription {
    ///   containerStatsStream(
    ///     containerId: "abc123"
    ///     agentId: "agent-local"
    ///   ) {
    ///     containerId
    ///     timestamp
    ///     cpuStats {
    ///       cpuPercentage
    ///       totalUsage
    ///     }
    ///     memoryStats {
    ///       usage
    ///       percentage
    ///       limit
    ///     }
    ///   }
    /// }
    /// ```
    async fn container_stats_stream(
        &self,
        ctx: &Context<'_>,
        container_id: String,
        agent_id: String,
    ) -> Result<impl Stream<Item = Result<ContainerStats>>> {
        let state = ctx.data::<AppState>()?;
        
        // Track subscription metrics with RAII guard
        state.metrics.subscription_started(&agent_id);
        let guard = Arc::new(SubscriptionGuard {
            metrics: state.metrics.clone(),
            agent_id: agent_id.clone(),
        });
        
        // Get agent connection
        let agent_conn = state
            .agent_pool
            .get_agent(&agent_id)
            .ok_or_else(|| {
                state.metrics.subscription_failed();
                ApiError::AgentNotFound(agent_id.clone()).extend()
            })?;
        
        // Check agent health
        if !agent_conn.is_healthy() {
            state.metrics.subscription_failed();
            return Err(ApiError::AgentUnavailable(format!(
                "Agent '{}' is not healthy. Try again later or check agent status.",
                agent_id
            )).extend());
        }
        
        // Clone client to release lock immediately
        let mut client = {
            let guard = agent_conn.client.lock().await;
            guard.clone()
        };
        
        // Build gRPC request
        let request = ContainerStatsRequest {
            container_id: container_id.clone(),
            stream: true, // Enable streaming mode
        };
        
        // Open stats stream
        let grpc_stream = client
            .stream_container_stats(request)
            .await
            .map_err(|e| {
                state.metrics.subscription_failed();
                ApiError::Internal(format!("Failed to open stats stream: {}", e)).extend()
            })?;
        
        // Convert gRPC stream to GraphQL stream using shared helper.
        // The guard is moved into the closure to track metrics on disconnect.
        let stats_stream = grpc_stream.map(move |result| {
            let _guard = &guard;
            match result {
                Ok(response) => Ok(ContainerStats::from_proto(response)),
                Err(e) => Err(ApiError::Internal(format!("Stats stream error: {}", e)).extend()),
            }
        });
        
        Ok(stats_stream)
    }
}
