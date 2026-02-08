use async_graphql::{Context, Result, Subscription};
use futures::{Stream, StreamExt};
use std::sync::Arc;

use crate::state::AppState;
use crate::error::ApiError;
use crate::graphql::types::log::{LogEntry, LogStreamOptions};
use crate::graphql::types::agent::{AgentHealthEvent, AgentStatus, MetadataEntry};
use crate::graphql::types::stats::ContainerStats;
use crate::graphql::types::swarm::{ComparisonLogEntry, ComparisonSourceInput, SyncMode, NodeEventView, NodeEventTypeGql, ServiceEventView, ServiceEventTypeGql, ServiceRestartEventView, RestartEventTypeGql};
use crate::agent::client::{LogStreamRequest, HealthCheckRequest, ContainerStatsRequest, ServiceLogStreamRequest};
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

    /// Stream logs from a swarm service in real-time, aggregated across all tasks/replicas
    ///
    /// # Arguments
    /// * `service_id` - The service ID or name
    /// * `agent_id` - The agent ID (must be a swarm manager)
    /// * `follow` - Follow mode (default true)
    /// * `tail` - Number of lines from the end (default 50)
    /// * `since` - Unix timestamp — only show logs since this time
    /// * `until` - Unix timestamp — only show logs until this time
    /// * `timestamps` - Show timestamps (default true)
    ///
    /// # Example
    /// ```graphql
    /// subscription {
    ///   serviceLogStream(
    ///     serviceId: "my-service"
    ///     agentId: "agent-manager"
    ///     follow: true
    ///     tail: 100
    ///   ) {
    ///     timestamp
    ///     level
    ///     content
    ///     swarmContext {
    ///       serviceName
    ///       taskId
    ///       taskSlot
    ///       nodeId
    ///     }
    ///   }
    /// }
    /// ```
    async fn service_log_stream(
        &self,
        ctx: &Context<'_>,
        service_id: String,
        agent_id: String,
        #[graphql(default = true)] follow: bool,
        #[graphql(default = 50)] tail: i32,
        since: Option<i64>,
        until: Option<i64>,
        #[graphql(default = true)] timestamps: bool,
    ) -> Result<impl Stream<Item = Result<LogEntry>>> {
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
        let request = ServiceLogStreamRequest {
            service_id: service_id.clone(),
            follow,
            tail_lines: if tail > 0 { Some(tail as u32) } else { None },
            since,
            until,
            timestamps,
        };

        // Open service log stream
        let grpc_stream = client
            .stream_service_logs(request)
            .await
            .map_err(|e| {
                state.metrics.subscription_failed();
                ApiError::Internal(format!(
                    "Failed to open service log stream for '{}': {}. Is this a swarm manager?",
                    service_id, e
                )).extend()
            })?;

        // Clone metrics and agent_id for use in stream closure
        let metrics = state.metrics.clone();
        let agent_id_for_stream = agent_id.clone();

        // Convert gRPC stream to GraphQL stream.
        // The guard is moved into the closure to track metrics on disconnect.
        let log_stream = grpc_stream.map(move |result| {
            let _guard = &guard;
            match result {
                Ok(response) => {
                    let byte_count = response.raw_content.len();
                    metrics.message_sent(byte_count);
                    LogEntry::from_proto(response, agent_id_for_stream.clone())
                }
                Err(e) => Err(ApiError::Internal(format!("Service log stream error: {}", e)).extend()),
            }
        });

        Ok(log_stream)
    }

    /// Stream logs from ALL services in a stack, aggregated and interleaved by timestamp
    ///
    /// Discovers services with `com.docker.stack.namespace` label matching the given
    /// namespace, opens a service log stream for each, and merges them using the same
    /// `ready_chunks` + intra-chunk sort strategy as `logsFromContainers`.
    ///
    /// # Arguments
    /// * `namespace` - Stack namespace (com.docker.stack.namespace label value)
    /// * `agent_id` - Agent ID (must be a swarm manager)
    /// * `follow` - Follow mode (default true)
    /// * `tail` - Number of tail lines per service (default 50)
    /// * `since` - Unix timestamp — only show logs since this time
    /// * `until` - Unix timestamp — only show logs until this time
    ///
    /// # Example
    /// ```graphql
    /// subscription {
    ///   stackLogStream(namespace: "mystack", agentId: "agent-manager") {
    ///     timestamp
    ///     content
    ///     swarmContext {
    ///       serviceName
    ///       taskSlot
    ///       nodeId
    ///     }
    ///   }
    /// }
    /// ```
    async fn stack_log_stream(
        &self,
        ctx: &Context<'_>,
        namespace: String,
        agent_id: String,
        #[graphql(default = true)] follow: bool,
        #[graphql(default = 50)] tail: i32,
        since: Option<i64>,
        until: Option<i64>,
    ) -> Result<impl Stream<Item = Result<LogEntry>>> {
        let state = ctx.data::<AppState>()?;

        // Track subscription metrics
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

        if !agent_conn.is_healthy() {
            state.metrics.subscription_failed();
            return Err(ApiError::AgentUnavailable(format!(
                "Agent '{}' is not healthy.", agent_id
            )).extend());
        }

        // List all services, filter to this stack's namespace
        let mut client = {
            let g = agent_conn.client.lock().await;
            g.clone()
        };

        let svc_response = client
            .list_services(crate::agent::client::ServiceListRequest {})
            .await
            .map_err(|e| {
                state.metrics.subscription_failed();
                ApiError::Internal(format!("Failed to list services: {}", e)).extend()
            })?;

        let stack_services: Vec<_> = svc_response.services.into_iter()
            .filter(|s| s.stack_namespace.as_deref() == Some(&namespace))
            .collect();

        if stack_services.is_empty() {
            state.metrics.subscription_failed();
            return Err(ApiError::InvalidRequest(format!(
                "No services found in stack '{}'", namespace
            )).extend());
        }

        const MAX_STACK_SERVICES: usize = 20;
        if stack_services.len() > MAX_STACK_SERVICES {
            state.metrics.subscription_failed();
            return Err(ApiError::InvalidRequest(format!(
                "Stack '{}' has {} services, maximum is {}",
                namespace, stack_services.len(), MAX_STACK_SERVICES
            )).extend());
        }

        // Open a service log stream for each service in the stack
        let mut streams = Vec::new();
        let metrics = state.metrics.clone();

        for svc in &stack_services {
            let svc_id = svc.id.clone();
            let svc_name = svc.name.clone();

            // Clone client for each stream (Arc-backed, cheap)
            let mut svc_client = {
                let g = agent_conn.client.lock().await;
                g.clone()
            };

            let request = ServiceLogStreamRequest {
                service_id: svc_id.clone(),
                follow,
                tail_lines: if tail > 0 { Some(tail as u32) } else { None },
                since,
                until,
                timestamps: true,
            };

            match svc_client.stream_service_logs(request).await {
                Ok(grpc_stream) => {
                    let agent_id_clone = agent_id.clone();
                    let metrics_clone = metrics.clone();

                    let log_stream = grpc_stream.map(move |result| {
                        match result {
                            Ok(response) => {
                                metrics_clone.message_sent(response.raw_content.len());
                                LogEntry::from_proto(response, agent_id_clone.clone())
                            }
                            Err(e) => Err(ApiError::Internal(
                                format!("Stream error for service '{}': {}", svc_id, e)
                            ).extend()),
                        }
                    });

                    streams.push(Box::pin(log_stream));
                    tracing::info!(service = %svc_name, stack = %namespace, "Opened stack service log stream");
                }
                Err(e) => {
                    tracing::warn!(
                        service = %svc_name, stack = %namespace,
                        "Failed to open service log stream: {}", e
                    );
                    // Continue with other services
                }
            }
        }

        if streams.is_empty() {
            state.metrics.subscription_failed();
            return Err(ApiError::Internal(format!(
                "Failed to open any service log streams for stack '{}'", namespace
            )).extend());
        }

        tracing::info!(
            stack = %namespace,
            services = streams.len(),
            "Stack log stream opened for {} services",
            streams.len()
        );

        // Merge all service streams with intra-chunk timestamp sorting
        // Same strategy as logsFromContainers — ready_chunks(10) + sort
        let merged_stream = futures::stream::select_all(streams)
            .ready_chunks(10)
            .flat_map(|mut chunk| {
                chunk.sort_by(|a, b| match (a, b) {
                    (Ok(entry_a), Ok(entry_b)) => entry_a.timestamp.cmp(&entry_b.timestamp),
                    _ => std::cmp::Ordering::Equal,
                });
                futures::stream::iter(chunk)
            })
            .map(move |item| {
                let _guard = &guard;
                item
            });

        Ok(merged_stream)
    }

    // =========================================================================
    // S6: Service Update Stream — real-time rolling update progress
    // =========================================================================

    #[graphql(name = "serviceUpdateStream")]
    async fn service_update_stream(
        &self,
        ctx: &Context<'_>,
        service_id: String,
        agent_id: String,
        #[graphql(default = 1000)] poll_interval_ms: i32,
    ) -> Result<impl Stream<Item = Result<crate::graphql::types::swarm::ServiceUpdateEventView>>> {
        let state = ctx.data::<AppState>()?;

        let agent = state.agent_pool.get_agent(&agent_id)
            .ok_or_else(|| ApiError::AgentNotFound(agent_id.clone()).extend())?;

        let mut client = {
            let guard = agent.client.lock().await;
            guard.clone()
        };

        let metrics = state.metrics.clone();
        metrics.subscription_started(&agent_id);

        let grpc_stream = client
            .service_update_stream(&service_id, Some(poll_interval_ms.max(500) as u64))
            .await
            .map_err(|e| ApiError::Internal(format!("Failed to start update stream: {}", e)).extend())?;

        let guard = Arc::new(SubscriptionGuard {
            metrics: metrics.clone(),
            agent_id: agent_id.clone(),
        });

        let stream = grpc_stream.map(move |result| {
            let _guard = &guard;
            match result {
                Ok(event) => {
                    let view = crate::graphql::types::swarm::ServiceUpdateEventView {
                        update_state: event.update_state,
                        started_at: event.started_at.and_then(|ts| chrono::DateTime::from_timestamp(ts, 0)),
                        completed_at: event.completed_at.and_then(|ts| chrono::DateTime::from_timestamp(ts, 0)),
                        message: event.message,
                        tasks_total: event.tasks_total as i32,
                        tasks_running: event.tasks_running as i32,
                        tasks_ready: event.tasks_ready as i32,
                        tasks_failed: event.tasks_failed as i32,
                        tasks_shutdown: event.tasks_shutdown as i32,
                        snapshot_at: chrono::DateTime::from_timestamp(event.snapshot_at, 0)
                            .unwrap_or_else(chrono::Utc::now),
                        recent_changes: event.recent_changes.into_iter().map(|tc| {
                            crate::graphql::types::swarm::TaskStateChangeView {
                                task_id: tc.task_id,
                                service_id: tc.service_id,
                                node_id: tc.node_id,
                                slot: tc.slot.map(|s| s as i32),
                                state: tc.state,
                                desired_state: tc.desired_state,
                                message: tc.message,
                                error: tc.error,
                                updated_at: chrono::DateTime::from_timestamp(tc.updated_at, 0)
                                    .unwrap_or_else(chrono::Utc::now),
                            }
                        }).collect(),
                    };
                    Ok(view)
                }
                Err(e) => Err(ApiError::Internal(format!("Update stream error: {}", e)).extend()),
            }
        });

        Ok(stream)
    }

    // =========================================================================
    // S7: Side-by-Side Replica Log Comparison
    // =========================================================================

    /// Stream logs from multiple sources side-by-side with lane tagging.
    ///
    /// Each source opens a gRPC log stream (container-level or service-level).
    /// Entries are tagged with a lane index and label so the UI can render
    /// them in separate columns. Entries are merged and loosely time-aligned
    /// using `ready_chunks` + intra-chunk timestamp sorting.
    ///
    /// # Arguments
    /// * `sources` — 2+ comparison sources (containers, services, or tasks)
    /// * `options` — Log streaming options (tail, follow, filter, etc.)
    /// * `sync_mode` — How to synchronize entries across lanes (default: TIMESTAMP)
    ///
    /// # Example
    /// ```graphql
    /// subscription {
    ///   comparisonLogStream(
    ///     sources: [
    ///       { containerId: "abc123", agentId: "agent-1", label: "web-1" }
    ///       { containerId: "def456", agentId: "agent-1", label: "web-2" }
    ///     ]
    ///     options: { follow: true, tail: 50 }
    ///   ) {
    ///     laneIndex
    ///     laneLabel
    ///     entry { timestamp content level }
    ///     syncTimestamp
    ///   }
    /// }
    /// ```
    #[graphql(name = "comparisonLogStream")]
    async fn comparison_log_stream(
        &self,
        ctx: &Context<'_>,
        sources: Vec<ComparisonSourceInput>,
        options: Option<LogStreamOptions>,
        #[graphql(default)] sync_mode: SyncMode,
    ) -> Result<impl Stream<Item = Result<ComparisonLogEntry>>> {
        let state = ctx.data::<AppState>()?;

        // Validate: need at least 2 sources for comparison
        if sources.len() < 2 {
            return Err(ApiError::InvalidRequest(
                "At least 2 sources are required for comparison".to_string(),
            ).extend());
        }

        const MAX_COMPARISON_LANES: usize = 10;
        if sources.len() > MAX_COMPARISON_LANES {
            return Err(ApiError::InvalidRequest(format!(
                "Too many comparison sources ({}). Maximum is {}",
                sources.len(),
                MAX_COMPARISON_LANES,
            )).extend());
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

        // Track subscription metrics for each lane
        let mut guards = Vec::new();
        for src in &sources {
            state.metrics.subscription_started(&src.agent_id);
            guards.push(Arc::new(SubscriptionGuard {
                metrics: state.metrics.clone(),
                agent_id: src.agent_id.clone(),
            }));
        }

        // Open a stream for each comparison source and tag with lane info
        let mut streams: Vec<std::pin::Pin<Box<dyn Stream<Item = Result<ComparisonLogEntry>> + Send>>> = Vec::new();
        let mut failed_sources = Vec::new();

        for (lane_index, source) in sources.into_iter().enumerate() {
            let lane_label = source.label.clone().unwrap_or_else(|| {
                if let Some(ref cid) = source.container_id {
                    format!("container-{}", &cid[..12.min(cid.len())])
                } else if let Some(ref sid) = source.service_id {
                    format!("service-{}", &sid[..12.min(sid.len())])
                } else if let Some(ref tid) = source.task_id {
                    format!("task-{}", &tid[..12.min(tid.len())])
                } else {
                    format!("lane-{}", lane_index)
                }
            });

            // Get agent connection
            let agent_conn = match state.agent_pool.get_agent(&source.agent_id) {
                Some(conn) => conn,
                None => {
                    tracing::warn!(
                        agent_id = %source.agent_id,
                        lane = lane_index,
                        "Agent not found, skipping comparison source"
                    );
                    failed_sources.push((lane_index, source.agent_id.clone(), "Agent not found".to_string()));
                    continue;
                }
            };

            if !agent_conn.is_healthy() {
                tracing::warn!(
                    agent_id = %source.agent_id,
                    lane = lane_index,
                    "Agent not healthy, skipping comparison source"
                );
                failed_sources.push((lane_index, source.agent_id.clone(), "Agent not healthy".to_string()));
                continue;
            }

            let mut client = {
                let guard = agent_conn.client.lock().await;
                guard.clone()
            };

            let lane_idx_i32 = lane_index as i32;
            let label_clone = lane_label.clone();
            let metrics = state.metrics.clone();

            // Determine what kind of stream to open based on which IDs are set
            if let Some(ref container_id) = source.container_id {
                // Container-level log stream
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
                    disable_parsing: false,
                };

                match client.stream_logs(request).await {
                    Ok(grpc_stream) => {
                        let tagged_stream = grpc_stream.map(move |result| {
                            match result {
                                Ok(response) => {
                                    metrics.message_sent(response.raw_content.len());
                                    let entry = LogEntry::from_proto(response, label_clone.clone())?;
                                    let sync_ts = entry.timestamp;
                                    Ok(ComparisonLogEntry {
                                        lane_index: lane_idx_i32,
                                        lane_label: label_clone.clone(),
                                        entry,
                                        sync_timestamp: sync_ts,
                                    })
                                }
                                Err(e) => Err(ApiError::Internal(format!(
                                    "Lane {} stream error: {}", lane_idx_i32, e
                                )).extend()),
                            }
                        });
                        streams.push(Box::pin(tagged_stream));
                        tracing::info!(lane = lane_index, label = %lane_label, "Opened container comparison stream");
                    }
                    Err(e) => {
                        tracing::warn!(lane = lane_index, "Failed to open container log stream: {}", e);
                        failed_sources.push((lane_index, source.agent_id.clone(), format!("Stream open failed: {}", e)));
                        continue;
                    }
                }
            } else if let Some(ref service_id) = source.service_id {
                // Service-level log stream (aggregated across tasks)
                let request = ServiceLogStreamRequest {
                    service_id: service_id.clone(),
                    follow: opts.follow,
                    tail_lines: opts.tail.and_then(|t| if t > 0 { Some(t as u32) } else { None }),
                    since: opts.since.map(|dt| dt.timestamp()),
                    until: opts.until.map(|dt| dt.timestamp()),
                    timestamps: opts.timestamps,
                };

                match client.stream_service_logs(request).await {
                    Ok(grpc_stream) => {
                        let tagged_stream = grpc_stream.map(move |result| {
                            match result {
                                Ok(response) => {
                                    metrics.message_sent(response.raw_content.len());
                                    let entry = LogEntry::from_proto(response, label_clone.clone())?;
                                    let sync_ts = entry.timestamp;
                                    Ok(ComparisonLogEntry {
                                        lane_index: lane_idx_i32,
                                        lane_label: label_clone.clone(),
                                        entry,
                                        sync_timestamp: sync_ts,
                                    })
                                }
                                Err(e) => Err(ApiError::Internal(format!(
                                    "Lane {} stream error: {}", lane_idx_i32, e
                                )).extend()),
                            }
                        });
                        streams.push(Box::pin(tagged_stream));
                        tracing::info!(lane = lane_index, label = %lane_label, "Opened service comparison stream");
                    }
                    Err(e) => {
                        tracing::warn!(lane = lane_index, "Failed to open service log stream: {}", e);
                        failed_sources.push((lane_index, source.agent_id.clone(), format!("Stream open failed: {}", e)));
                        continue;
                    }
                }
            } else if let Some(ref task_id) = source.task_id {
                // Task-level: resolve task to container ID, then open container log stream
                let task_response = client
                    .list_tasks(crate::agent::client::TaskListRequest {
                        service_id: None,
                    })
                    .await;

                let container_id = match task_response {
                    Ok(resp) => {
                        resp.tasks
                            .into_iter()
                            .find(|t| t.id == *task_id)
                            .and_then(|t| t.container_id)
                    }
                    Err(e) => {
                        tracing::warn!(lane = lane_index, task_id = %task_id, "Failed to resolve task: {}", e);
                        failed_sources.push((lane_index, source.agent_id.clone(), format!("Task lookup failed: {}", e)));
                        continue;
                    }
                };

                let container_id = match container_id {
                    Some(cid) => cid,
                    None => {
                        tracing::warn!(lane = lane_index, task_id = %task_id, "Task not found or has no container");
                        failed_sources.push((lane_index, source.agent_id.clone(), "Task has no container".to_string()));
                        continue;
                    }
                };

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
                    disable_parsing: false,
                };

                match client.stream_logs(request).await {
                    Ok(grpc_stream) => {
                        let tagged_stream = grpc_stream.map(move |result| {
                            match result {
                                Ok(response) => {
                                    metrics.message_sent(response.raw_content.len());
                                    let entry = LogEntry::from_proto(response, label_clone.clone())?;
                                    let sync_ts = entry.timestamp;
                                    Ok(ComparisonLogEntry {
                                        lane_index: lane_idx_i32,
                                        lane_label: label_clone.clone(),
                                        entry,
                                        sync_timestamp: sync_ts,
                                    })
                                }
                                Err(e) => Err(ApiError::Internal(format!(
                                    "Lane {} stream error: {}", lane_idx_i32, e
                                )).extend()),
                            }
                        });
                        streams.push(Box::pin(tagged_stream));
                        tracing::info!(lane = lane_index, label = %lane_label, container_id = %container_id, "Opened task comparison stream");
                    }
                    Err(e) => {
                        tracing::warn!(lane = lane_index, "Failed to open task log stream: {}", e);
                        failed_sources.push((lane_index, source.agent_id.clone(), format!("Stream open failed: {}", e)));
                        continue;
                    }
                }
            } else {
                failed_sources.push((lane_index, source.agent_id.clone(), "No container_id, service_id, or task_id specified".to_string()));
                continue;
            }
        }

        // Need at least 2 streams for comparison to be meaningful
        if streams.len() < 2 {
            let error_msg = failed_sources
                .iter()
                .map(|(idx, aid, err)| format!("lane{}@{}: {}", idx, aid, err))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(ApiError::Internal(format!(
                "Need at least 2 working streams for comparison, only got {}. Errors: {}",
                streams.len(),
                error_msg,
            )).extend());
        }

        if !failed_sources.is_empty() {
            tracing::warn!(
                "Comparison stream: {}/{} lanes active (failed: {:?})",
                streams.len(),
                streams.len() + failed_sources.len(),
                failed_sources,
            );
        }

        // Merge all lane streams. Strategy depends on sync_mode:
        // - TIMESTAMP: ready_chunks + sort by sync_timestamp (best-effort alignment)
        // - SEQUENCE: ready_chunks + sort by sequence number
        // - NONE: simple interleave with no sorting
        let merged_stream = match sync_mode {
            SyncMode::Timestamp => {
                let stream = futures::stream::select_all(streams)
                    .ready_chunks(20) // Larger chunks for better cross-lane alignment
                    .flat_map(|mut chunk| {
                        chunk.sort_by(|a, b| match (a, b) {
                            (Ok(entry_a), Ok(entry_b)) => entry_a.sync_timestamp.cmp(&entry_b.sync_timestamp),
                            _ => std::cmp::Ordering::Equal,
                        });
                        futures::stream::iter(chunk)
                    })
                    .map(move |item| {
                        let _guards = &guards;
                        item
                    });
                Box::pin(stream) as std::pin::Pin<Box<dyn Stream<Item = Result<ComparisonLogEntry>> + Send>>
            }
            SyncMode::Sequence => {
                let stream = futures::stream::select_all(streams)
                    .ready_chunks(20)
                    .flat_map(|mut chunk| {
                        chunk.sort_by(|a, b| match (a, b) {
                            (Ok(entry_a), Ok(entry_b)) => entry_a.entry.sequence.cmp(&entry_b.entry.sequence),
                            _ => std::cmp::Ordering::Equal,
                        });
                        futures::stream::iter(chunk)
                    })
                    .map(move |item| {
                        let _guards = &guards;
                        item
                    });
                Box::pin(stream) as std::pin::Pin<Box<dyn Stream<Item = Result<ComparisonLogEntry>> + Send>>
            }
            SyncMode::None => {
                let stream = futures::stream::select_all(streams)
                    .map(move |item| {
                        let _guards = &guards;
                        item
                    });
                Box::pin(stream) as std::pin::Pin<Box<dyn Stream<Item = Result<ComparisonLogEntry>> + Send>>
            }
        };

        Ok(merged_stream)
    }

    // =========================================================================
    // S9: Node Event Stream — drain awareness & node state changes
    // =========================================================================

    /// Stream node events — availability changes (drain/active/pause), state changes
    /// (ready/down/disconnected), role changes, and drain completion.
    ///
    /// # Arguments
    /// * `agent_id` — The agent to monitor nodes on
    /// * `node_id` — Optional: filter events to a specific node (empty = all nodes)
    /// * `poll_interval_ms` — Polling interval in ms (default: 2000)
    async fn node_event_stream(
        &self,
        ctx: &Context<'_>,
        agent_id: String,
        #[graphql(default)] node_id: Option<String>,
        #[graphql(default = 2000)] poll_interval_ms: i32,
    ) -> Result<impl Stream<Item = Result<NodeEventView>> + '_> {
        let state = ctx.data::<AppState>()?;

        let agent = state.agent_pool.get_agent(&agent_id)
            .ok_or_else(|| ApiError::AgentNotFound(agent_id.clone()).extend())?;

        let mut client = {
            let guard = agent.client.lock().await;
            guard.clone()
        };

        let metrics = state.metrics.clone();
        metrics.subscription_started(&agent_id);

        let request = crate::agent::client::NodeEventStreamRequest {
            node_id: node_id.unwrap_or_default(),
            poll_interval_ms: poll_interval_ms.max(500) as u64,
        };

        let grpc_stream = client
            .node_event_stream(request)
            .await
            .map_err(|e| ApiError::Internal(format!("Failed to start node event stream: {}", e)).extend())?;

        let guard = Arc::new(SubscriptionGuard {
            metrics: metrics.clone(),
            agent_id: agent_id.clone(),
        });

        let stream = grpc_stream.map(move |item| {
            let _guard = &guard;
            match item {
                Ok(event) => {
                    let affected_tasks: Vec<crate::graphql::types::swarm::TaskView> = event.affected_tasks.into_iter().map(|t| {
                        crate::graphql::types::swarm::TaskView {
                            id: t.id,
                            service_id: t.service_id.clone(),
                            node_id: t.node_id,
                            slot: t.slot.map(|s| s as i32),
                            container_id: t.container_id,
                            state: t.state,
                            desired_state: t.desired_state,
                            status_message: t.status_message,
                            status_err: t.status_err,
                            created_at: chrono::DateTime::from_timestamp(t.created_at, 0)
                                .unwrap_or_else(chrono::Utc::now),
                            updated_at: chrono::DateTime::from_timestamp(t.updated_at, 0)
                                .unwrap_or_else(chrono::Utc::now),
                            exit_code: t.exit_code,
                            service_name: t.service_name,
                            agent_id: String::new(),
                        }
                    }).collect();

                    Ok(NodeEventView {
                        node_id: event.node_id,
                        hostname: event.hostname,
                        event_type: NodeEventTypeGql::from_proto(event.event_type),
                        previous_value: event.previous_value,
                        current_value: event.current_value,
                        affected_tasks,
                        timestamp: event.timestamp,
                    })
                }
                Err(e) => Err(async_graphql::Error::new(format!(
                    "Node event stream error: {}",
                    e
                ))),
            }
        });

        Ok(stream)
    }

    // =========================================================================
    // S10: Service Scaling Events
    // =========================================================================

    /// Stream service scaling and lifecycle events (scale up/down, update start/complete/rollback, task failures/recoveries).
    async fn service_events(
        &self,
        ctx: &Context<'_>,
        service_id: String,
        agent_id: String,
        #[graphql(default = 2000)] poll_interval_ms: i32,
    ) -> Result<impl Stream<Item = Result<ServiceEventView>>> {
        let state = ctx.data::<AppState>()?;

        let agent = state.agent_pool.get_agent(&agent_id)
            .ok_or_else(|| ApiError::AgentNotFound(agent_id.clone()).extend())?;

        let mut client = {
            let guard = agent.client.lock().await;
            guard.clone()
        };

        let metrics = state.metrics.clone();
        metrics.subscription_started(&agent_id);

        let grpc_stream = client
            .service_event_stream(&service_id, Some(poll_interval_ms.max(500) as u64))
            .await
            .map_err(|e| ApiError::Internal(format!("Failed to start service event stream: {}", e)).extend())?;

        let guard = Arc::new(SubscriptionGuard {
            metrics: metrics.clone(),
            agent_id: agent_id.clone(),
        });

        let stream = grpc_stream.map(move |result| {
            let _guard = &guard;
            match result {
                Ok(event) => {
                    let affected_tasks: Vec<crate::graphql::types::swarm::TaskView> = event.affected_tasks.into_iter().map(|t| {
                        crate::graphql::types::swarm::TaskView {
                            id: t.id,
                            service_id: t.service_id.clone(),
                            node_id: t.node_id,
                            slot: t.slot.map(|s| s as i32),
                            container_id: t.container_id,
                            state: t.state,
                            desired_state: t.desired_state,
                            status_message: t.status_message,
                            status_err: t.status_err,
                            created_at: chrono::DateTime::from_timestamp(t.created_at, 0)
                                .unwrap_or_else(chrono::Utc::now),
                            updated_at: chrono::DateTime::from_timestamp(t.updated_at, 0)
                                .unwrap_or_else(chrono::Utc::now),
                            exit_code: t.exit_code,
                            service_name: t.service_name,
                            agent_id: String::new(),
                        }
                    }).collect();

                    Ok(ServiceEventView {
                        service_id: event.service_id,
                        event_type: ServiceEventTypeGql::from_proto(event.event_type),
                        previous_replicas: event.previous_replicas.map(|r| r as i32),
                        current_replicas: event.current_replicas.map(|r| r as i32),
                        timestamp: event.timestamp,
                        message: event.message,
                        affected_tasks,
                    })
                }
                Err(e) => Err(ApiError::Internal(format!("Service event stream error: {}", e)).extend()),
            }
        });

        Ok(stream)
    }

    // =========================================================================
    // S11: Service Restart Events
    // =========================================================================

    /// Stream service restart events — task replacements, crash loops, OOM kills.
    /// If serviceId is not provided, monitors all services.
    async fn service_restart_events(
        &self,
        ctx: &Context<'_>,
        agent_id: String,
        service_id: Option<String>,
        #[graphql(default = 2000)] poll_interval_ms: i32,
    ) -> Result<impl Stream<Item = Result<ServiceRestartEventView>>> {
        let state = ctx.data::<AppState>()?;

        let agent = state.agent_pool.get_agent(&agent_id)
            .ok_or_else(|| ApiError::AgentNotFound(agent_id.clone()).extend())?;

        let mut client = {
            let guard = agent.client.lock().await;
            guard.clone()
        };

        let metrics = state.metrics.clone();
        metrics.subscription_started(&agent_id);

        let grpc_stream = client
            .service_restart_event_stream(
                service_id.as_deref(),
                Some(poll_interval_ms.max(500) as u64),
            )
            .await
            .map_err(|e| ApiError::Internal(format!("Failed to start restart event stream: {}", e)).extend())?;

        let guard = Arc::new(SubscriptionGuard {
            metrics: metrics.clone(),
            agent_id: agent_id.clone(),
        });

        let stream = grpc_stream.map(move |result| {
            let _guard = &guard;
            match result {
                Ok(event) => {
                    let convert_task = |t: crate::agent::client::proto::TaskInfo| -> crate::graphql::types::swarm::TaskView {
                        crate::graphql::types::swarm::TaskView {
                            id: t.id,
                            service_id: t.service_id.clone(),
                            node_id: t.node_id,
                            slot: t.slot.map(|s| s as i32),
                            container_id: t.container_id,
                            state: t.state,
                            desired_state: t.desired_state,
                            status_message: t.status_message,
                            status_err: t.status_err,
                            created_at: chrono::DateTime::from_timestamp(t.created_at, 0)
                                .unwrap_or_else(chrono::Utc::now),
                            updated_at: chrono::DateTime::from_timestamp(t.updated_at, 0)
                                .unwrap_or_else(chrono::Utc::now),
                            exit_code: t.exit_code,
                            service_name: t.service_name,
                            agent_id: String::new(),
                        }
                    };

                    Ok(ServiceRestartEventView {
                        service_id: event.service_id,
                        service_name: event.service_name,
                        event_type: RestartEventTypeGql::from_proto(event.event_type),
                        new_task: event.new_task.map(&convert_task),
                        old_task: event.old_task.map(&convert_task),
                        slot: event.slot.map(|s| s as i32),
                        restart_count: event.restart_count as i32,
                        timestamp: event.timestamp,
                        message: event.message,
                    })
                }
                Err(e) => Err(ApiError::Internal(format!("Restart event stream error: {}", e)).extend()),
            }
        });

        Ok(stream)
    }

    /// Stream Docker engine events (container lifecycle, image pulls, network changes, etc.)
    ///
    /// # Arguments
    /// * `agent_id` - The agent ID to stream events from
    /// * `event_types` - Optional filter for specific event types (e.g. "container", "image", "network", "volume")
    async fn docker_events(
        &self,
        ctx: &Context<'_>,
        agent_id: String,
        event_types: Option<Vec<String>>,
    ) -> Result<impl Stream<Item = Result<crate::graphql::types::swarm::DockerEventView>>> {
        let state = ctx.data::<AppState>()?;

        state.metrics.subscription_started(&agent_id);
        let metrics = state.metrics.clone();

        let agent_conn = state
            .agent_pool
            .get_agent(&agent_id)
            .ok_or_else(|| {
                state.metrics.subscription_failed();
                ApiError::AgentNotFound(agent_id.clone()).extend()
            })?;

        if !agent_conn.is_healthy() {
            state.metrics.subscription_failed();
            return Err(ApiError::AgentUnavailable(format!(
                "Agent '{}' is not healthy.",
                agent_id
            )).extend());
        }

        let mut client = {
            let guard = agent_conn.client.lock().await;
            guard.clone()
        };

        let grpc_stream = client
            .stream_docker_events(crate::agent::client::DockerEventsRequest {
                type_filters: event_types.unwrap_or_default(),
                since: None,
                until: None,
            })
            .await
            .map_err(|e| {
                metrics.subscription_failed();
                ApiError::Internal(format!("Failed to start docker events stream: {}", e)).extend()
            })?;

        let guard = Arc::new(SubscriptionGuard {
            metrics: metrics.clone(),
            agent_id: agent_id.clone(),
        });

        let aid = agent_id.clone();
        let stream = grpc_stream.map(move |result| {
            let _guard = &guard;
            match result {
                Ok(event) => {
                    let attributes: Vec<crate::graphql::types::agent::Label> = event.actor_attributes
                        .into_iter()
                        .map(|(k, v)| crate::graphql::types::agent::Label {
                            key: k,
                            value: v,
                        })
                        .collect();

                    // Extract actor name from attributes if available
                    let actor_name = attributes.iter()
                        .find(|l| l.key == "name")
                        .map(|l| l.value.clone())
                        .unwrap_or_default();

                    Ok(crate::graphql::types::swarm::DockerEventView {
                        agent_id: aid.clone(),
                        event_type: event.event_type,
                        action: event.action,
                        actor_id: event.actor_id,
                        actor_name,
                        attributes,
                        timestamp: event.timestamp,
                    })
                }
                Err(e) => Err(ApiError::Internal(format!("Docker event stream error: {}", e)).extend()),
            }
        });

        Ok(stream)
    }

    // =========================================================================
    // B02: Task Log Streaming
    // =========================================================================

    /// Stream logs from a specific swarm task.
    ///
    /// Similar to service log streaming but targets a single task by ID.
    /// Useful for debugging individual task failures or container issues.
    async fn task_log_stream(
        &self,
        ctx: &Context<'_>,
        task_id: String,
        agent_id: String,
        #[graphql(default = true)] follow: bool,
        #[graphql(default = 50)] tail: i32,
        since: Option<i64>,
        until: Option<i64>,
        #[graphql(default = true)] timestamps: bool,
    ) -> Result<impl Stream<Item = Result<LogEntry>>> {
        let state = ctx.data::<AppState>()?;

        // Track subscription metrics
        state.metrics.subscription_started(&agent_id);
        let guard = Arc::new(SubscriptionGuard {
            metrics: state.metrics.clone(),
            agent_id: agent_id.clone(),
        });

        let agent_conn = state
            .agent_pool
            .get_agent(&agent_id)
            .ok_or_else(|| {
                state.metrics.subscription_failed();
                ApiError::AgentNotFound(agent_id.clone()).extend()
            })?;

        if !agent_conn.is_healthy() {
            state.metrics.subscription_failed();
            return Err(ApiError::AgentUnavailable(format!(
                "Agent '{}' is not healthy.",
                agent_id
            )).extend());
        }

        let mut client = {
            let guard = agent_conn.client.lock().await;
            guard.clone()
        };

        let request = crate::agent::client::TaskLogStreamRequest {
            task_id: task_id.clone(),
            follow,
            tail_lines: if tail > 0 { Some(tail as u32) } else { None },
            since,
            until,
            timestamps,
        };

        let grpc_stream = client
            .stream_task_logs(request)
            .await
            .map_err(|e| {
                state.metrics.subscription_failed();
                ApiError::Internal(format!(
                    "Failed to open task log stream for '{}': {}",
                    task_id, e
                )).extend()
            })?;

        let metrics = state.metrics.clone();
        let agent_id_for_stream = agent_id.clone();

        let log_stream = grpc_stream.map(move |result| {
            let _guard = &guard;
            match result {
                Ok(response) => {
                    let byte_count = response.raw_content.len();
                    metrics.message_sent(byte_count);
                    LogEntry::from_proto(response, agent_id_for_stream.clone())
                }
                Err(e) => Err(ApiError::Internal(format!("Task log stream error: {}", e)).extend()),
            }
        });

        Ok(log_stream)
    }
}
