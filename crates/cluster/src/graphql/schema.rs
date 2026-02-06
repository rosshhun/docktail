use async_graphql::{Context, EmptyMutation, Schema};
use crate::state::AppState;
use crate::error::ApiError;
use super::types::agent::{AgentView, AgentHealthSummary, agent_view_from_connection};
use super::types::container::{Container, ContainerFilter, ContainerState, ContainerDetailsCache, ContainerStateInfoGql};
use super::types::stats::ContainerStats;
use super::types::log::{LogEntry, LogStreamOptions, ContainerLookupCache};
use super::subscriptions::SubscriptionRoot;
use crate::agent::client::ContainerListRequest;
use futures::StreamExt;

pub type ClusterSchema = Schema<QueryRoot, EmptyMutation, SubscriptionRoot>;

/// Root Query type
pub struct QueryRoot;

#[async_graphql::Object]
impl QueryRoot {
    /// Health check query
    async fn health(&self) -> HealthStatus {
        HealthStatus {
            status: "healthy".to_string(),
            timestamp: chrono::Utc::now(),
        }
    }

    /// Version information
    async fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }

    /// Get all agents (or specific agents by IDs)
    async fn agents(&self, ctx: &Context<'_>, ids: Option<Vec<String>>) -> async_graphql::Result<Vec<AgentView>> {
        let state = ctx.data::<AppState>()?;
        
        let agents = if let Some(agent_ids) = ids {
            // Get specific agents by ID
            agent_ids
                .iter()
                .filter_map(|id| state.agent_pool.get_agent(id))
                .collect::<Vec<_>>()
        } else {
            // Get all agents
            state.agent_pool.list_agents()
        };

        // Convert to AgentView using shared helper
        let mut views = Vec::new();
        for conn in agents {
            let last_seen_instant = conn.last_seen().await;
            let duration = last_seen_instant.elapsed();
            let last_seen = chrono::Utc::now() - chrono::Duration::from_std(duration).unwrap_or_default();
            views.push(agent_view_from_connection(&conn, last_seen));
        }

        Ok(views)
    }

    /// Get a specific agent by ID
    async fn agent(&self, ctx: &Context<'_>, id: String) -> async_graphql::Result<Option<AgentView>> {
        let state = ctx.data::<AppState>()?;
        
        if let Some(conn) = state.agent_pool.get_agent(&id) {
            let last_seen_instant = conn.last_seen().await;
            let duration = last_seen_instant.elapsed();
            let last_seen = chrono::Utc::now() - chrono::Duration::from_std(duration).unwrap_or_default();
            Ok(Some(agent_view_from_connection(&conn, last_seen)))
        } else {
            Ok(None)
        }
    }

    /// Get agent health summary
    async fn agent_health(&self, ctx: &Context<'_>) -> async_graphql::Result<AgentHealthSummary> {
        let state = ctx.data::<AppState>()?;
        
        Ok(AgentHealthSummary {
            total: state.agent_pool.count() as i32,
            healthy: state.agent_pool.count_healthy() as i32,
            degraded: state.agent_pool.count_degraded() as i32,
            unhealthy: state.agent_pool.count_unhealthy() as i32,
            unknown: state.agent_pool.count_unknown() as i32,
        })
    }

    /// Get containers from one or more agents
    async fn containers(
        &self,
        ctx: &Context<'_>,
        agent_ids: Option<Vec<String>>,
        filter: Option<ContainerFilter>,
    ) -> async_graphql::Result<Vec<Container>> {
        let state = ctx.data::<AppState>()?;
        
        // Determine which agents to query
        let agents = if let Some(ids) = agent_ids {
            // Query specific agents
            ids.iter()
                .filter_map(|id| state.agent_pool.get_agent(id))
                .collect::<Vec<_>>()
        } else {
            // Query all healthy agents
            state.agent_pool.list_agents()
                .into_iter()
                .filter(|a| a.health_status() == crate::agent::HealthStatus::Healthy)
                .collect()
        };

        // Define per-agent tasks - capture filter by reference
        let filter_ref = &filter;
        
        let futures = agents.into_iter().map(|agent| async move {
            // ✅ Clone client to release lock immediately (non-blocking)
            let mut client = {
                let guard = agent.client.lock().await;
                guard.clone()
            };

            // Build the request based on filter
            let request = ContainerListRequest {
                state_filter: filter_ref.as_ref()
                    .and_then(|f| f.state.as_ref())
                    .map(|s| match s {
                        ContainerState::Running => 2,
                        ContainerState::Paused => 3,
                        ContainerState::Exited => 4,
                        ContainerState::Created => 5,
                        _ => 1,
                    }),
                include_stopped: filter_ref.as_ref()
                    .and_then(|f| f.include_stopped)
                    .unwrap_or(false),
                limit: filter_ref.as_ref()
                    .and_then(|f| f.limit)
                    .and_then(|l| if l > 0 { Some(l as u32) } else { None }),
            };

            // Perform network call (lock already released)
            match client.list_containers(request).await {
                Ok(response) => Some((agent.info.id.clone(), response.containers)),
                Err(e) => {
                    tracing::warn!("Failed to list containers from agent {}: {}", agent.info.id, e);
                    None // Skip failed agents
                }
            }
        });

        // ✅ Execute all agent requests in parallel
        let results = futures::future::join_all(futures).await;

        // Flatten and post-process results
        let mut all_containers = Vec::new();

        for (agent_id, containers) in results.into_iter().flatten() {
            for container_info in containers {
                // Convert proto port mappings to GraphQL port mappings
                let ports = container_info.ports.into_iter().map(|p| {
                    super::types::container::PortMapping {
                        container_port: p.container_port as i32,
                        protocol: p.protocol,
                        host_ip: p.host_ip,
                        host_port: p.host_port.map(|p| p as i32),
                    }
                }).collect();
                
                // Convert proto to GraphQL type
                let ts = chrono::DateTime::from_timestamp(container_info.created_at, 0);
                if ts.is_none() {
                    tracing::warn!(
                        container_id = %container_info.id,
                        created_at = container_info.created_at,
                        "Invalid created_at timestamp from agent, substituting current time"
                    );
                }
                let container = Container {
                    id: container_info.id,
                    agent_id: agent_id.clone(),
                    name: container_info.name,
                    image: container_info.image,
                    state: ContainerState::from(container_info.state.as_str()),
                    status: container_info.status,
                    labels_map: container_info.labels,
                    created_at: ts.unwrap_or_else(chrono::Utc::now),
                    log_driver: container_info.log_driver,
                    ports,
                    state_info: container_info.state_info.map(|si| ContainerStateInfoGql {
                        oom_killed: si.oom_killed,
                        pid: si.pid,
                        exit_code: si.exit_code,
                        started_at: si.started_at,
                        finished_at: si.finished_at,
                        restart_count: si.restart_count,
                    }),
                };

                // Apply post-query filters
                if let Some(ref filt) = filter {
                    // Name pattern filter
                    if let Some(ref pattern) = filt.name_pattern {
                        if !container.name.contains(pattern) {
                            continue;
                        }
                    }

                    // Image pattern filter
                    if let Some(ref pattern) = filt.image_pattern {
                        if !container.image.contains(pattern) {
                            continue;
                        }
                    }

                    // Label filters
                    if let Some(ref label_filters) = filt.labels {
                        let mut matches_all = true;
                        for label_filter in label_filters {
                            if let Some(container_value) = container.labels_map.get(&label_filter.key) {
                                if let Some(ref filter_value) = label_filter.value {
                                    if container_value != filter_value {
                                        matches_all = false;
                                        break;
                                    }
                                }
                                // If no value specified, just check key exists
                            } else {
                                matches_all = false;
                                break;
                            }
                        }
                        if !matches_all {
                            continue;
                        }
                    }
                }

                all_containers.push(container);
            }
        }

        Ok(all_containers)
    }

    /// Get a specific container by ID
    async fn container(
        &self,
        ctx: &Context<'_>,
        id: String,
        agent_id: Option<String>,
    ) -> async_graphql::Result<Option<Container>> {
        let state = ctx.data::<AppState>()?;
        
        // Determine which agents to search
        let agents = if let Some(aid) = agent_id {
            // Search specific agent
            state.agent_pool.get_agent(&aid)
                .map(|a| vec![a])
                .unwrap_or_default()
        } else {
            // Search all healthy agents
            state.agent_pool.list_agents()
                .into_iter()
                .filter(|a| a.health_status() == crate::agent::HealthStatus::Healthy)
                .collect()
        };

        // Search agents in parallel for the container
        let id_ref = &id;
        let futures = agents.into_iter().map(|agent| async move {
            // Clone client to release lock immediately
            let mut client = {
                let guard = agent.client.lock().await;
                guard.clone()
            };

            // Try to inspect this container
            match client.inspect_container(crate::agent::client::ContainerInspectRequest {
                container_id: id_ref.clone(),
            }).await {
                Ok(response) => {
                    if let Some(info) = response.info {
                        // Convert proto port mappings to GraphQL port mappings
                        let ports = info.ports.into_iter().map(|p| {
                            super::types::container::PortMapping {
                                container_port: p.container_port as i32,
                                protocol: p.protocol,
                                host_ip: p.host_ip,
                                host_port: p.host_port.map(|p| p as i32),
                            }
                        }).collect();
                        
                        let ts = chrono::DateTime::from_timestamp(info.created_at, 0);
                        if ts.is_none() {
                            tracing::warn!(
                                container_id = %info.id,
                                created_at = info.created_at,
                                "Invalid created_at timestamp, substituting current time"
                            );
                        }
                        
                        Some(Container {
                            id: info.id,
                            agent_id: agent.info.id.clone(),
                            name: info.name,
                            image: info.image,
                            state: ContainerState::from(info.state.as_str()),
                            status: info.status,
                            labels_map: info.labels,
                            created_at: ts.unwrap_or_else(chrono::Utc::now),
                            log_driver: info.log_driver,
                            ports,
                            state_info: info.state_info.map(|si| ContainerStateInfoGql {
                                oom_killed: si.oom_killed,
                                pid: si.pid,
                                exit_code: si.exit_code,
                                started_at: si.started_at,
                                finished_at: si.finished_at,
                                restart_count: si.restart_count,
                            }),
                        })
                    } else {
                        None
                    }
                }
                Err(_) => None,
            }
        });

        // Execute all searches in parallel, return first found
        let results = futures::future::join_all(futures).await;
        Ok(results.into_iter().flatten().next())
    }

    /// Get real-time statistics for a specific container
    async fn container_stats(
        &self,
        ctx: &Context<'_>,
        id: String,
        agent_id: String,
    ) -> async_graphql::Result<Option<ContainerStats>> {
        let state = ctx.data::<AppState>()?;
        
        // Get the specified agent
        let agent = state.agent_pool.get_agent(&agent_id)
            .ok_or_else(|| ApiError::AgentNotFound(agent_id.clone()).extend())?;

        // ✅ Clone client to release lock immediately
        let mut client = {
            let guard = agent.client.lock().await;
            guard.clone()
        };

        // Request stats from the agent
        match client.get_container_stats(crate::agent::client::ContainerStatsRequest {
            container_id: id.clone(),
            stream: false,
        }).await {
            Ok(response) => {
                Ok(Some(ContainerStats::from_proto(response)))
            }
            Err(e) => {
                tracing::warn!("Failed to get stats for container {} on agent {}: {}", id, agent_id, e);
                Err(ApiError::Internal(format!("Failed to get container stats: {}", e)).extend())
            }
        }
    }

    /// Get historical logs from a container (non-streaming, paginated)
    async fn logs(
        &self,
        ctx: &Context<'_>,
        container_id: String,
        agent_id: String,
        options: Option<LogStreamOptions>,
    ) -> async_graphql::Result<Vec<LogEntry>> {
        let state = ctx.data::<AppState>()?;
        
        // Get the specified agent
        let agent = state.agent_pool.get_agent(&agent_id)
            .ok_or_else(|| ApiError::AgentNotFound(agent_id.clone()).extend())?;

        // ✅ Clone client to release lock immediately
        let mut client = {
            let guard = agent.client.lock().await;
            guard.clone()
        };

        // Build the gRPC request from GraphQL options
        let mut opts = options.unwrap_or(LogStreamOptions {
            since: None,
            until: None,
            tail: Some(100), // Default to last 100 lines
            follow: false,   // Never follow for queries (only subscriptions)
            filter: None,
            filter_mode: super::types::log::FilterMode::None,
            timestamps: true,
        });

        // ✅ Enforce maximum limit and validate to prevent OOM and integer overflow
        const MAX_LOG_LINES: i32 = 2000;
        if let Some(tail) = opts.tail {
            if tail <= 0 {
                return Err(ApiError::InvalidRequest(
                    format!("tail must be a positive integer, got {}", tail)
                ).extend());
            }
            if tail > MAX_LOG_LINES {
                tracing::warn!(
                    "Clamping log tail from {} to {} lines to prevent memory issues",
                    tail,
                    MAX_LOG_LINES
                );
                opts.tail = Some(MAX_LOG_LINES);
            }
        } else {
            opts.tail = Some(MAX_LOG_LINES);
        }

        // Convert timestamps to Unix seconds
        let since = opts.since.map(|dt| dt.timestamp());
        let until = opts.until.map(|dt| dt.timestamp());

        let request = crate::agent::client::LogStreamRequest {
            container_id: container_id.clone(),
            since,
            until,
            follow: false, // Never follow for queries
            tail_lines: opts.tail.map(|t| t as u32),
            filter_pattern: opts.filter.clone(),
            filter_mode: {
                let proto_mode: crate::agent::client::FilterMode = opts.filter_mode.into();
                proto_mode as i32
            },
            timestamps: opts.timestamps,
            disable_parsing: false,  // Enable parsing by default
        };

        // Stream logs from the agent and collect them
        let mut stream = client.stream_logs(request).await
            .map_err(|e| ApiError::Internal(format!("Failed to stream logs: {}", e)).extend())?;

        let mut log_entries = Vec::new();
        
        // Collect all log entries from the stream
        while let Some(result) = stream.next().await {
            match result {
                Ok(response) => {
                    // Convert proto response to GraphQL LogEntry
                    let entry = LogEntry::from_proto(response, agent_id.clone())?;
                    log_entries.push(entry);
                }
                Err(e) => {
                    tracing::warn!("Error receiving log entry: {}", e);
                    // Continue receiving other logs even if one fails
                }
            }
        }

        Ok(log_entries)
    }
}

/// Health status type
#[derive(async_graphql::SimpleObject)]
pub struct HealthStatus {
    pub status: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Build the GraphQL schema
pub fn build_schema(state: AppState) -> ClusterSchema {
    let max_depth = state.config.graphql.max_depth;
    let max_complexity = state.config.graphql.max_complexity;

    Schema::build(QueryRoot, EmptyMutation, SubscriptionRoot)
        .data(state)
        .data(ContainerDetailsCache::new())
        .data(ContainerLookupCache::new())
        .limit_depth(max_depth)
        .limit_complexity(max_complexity)
        .finish()
}
