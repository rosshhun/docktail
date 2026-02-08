use async_graphql::{Context, Schema};
use crate::state::AppState;
use crate::error::ApiError;
use super::types::agent::{AgentView, AgentHealthSummary, DiscoveryStatusView, Label, agent_view_from_connection};
use super::types::container::{Container, ContainerFilter, ContainerState, ContainerDetailsCache, ContainerStateInfoGql};
use super::types::stats::ContainerStats;
use super::types::log::{LogEntry, LogStreamOptions, ContainerLookupCache};
use super::types::swarm::*;
use super::subscriptions::SubscriptionRoot;
use super::mutations::MutationRoot;
use crate::agent::client::ContainerListRequest;
use futures::StreamExt;

pub type ClusterSchema = Schema<QueryRoot, MutationRoot, SubscriptionRoot>;

/// Helper to resolve an optional agent_id to a manager agent connection.
/// If `agent_id` is Some, validates it's a manager. If None, auto-selects a
/// healthy manager agent. Returns the agent connection for client cloning.
async fn resolve_swarm_agent(
    state: &AppState,
    agent_id: Option<&str>,
) -> async_graphql::Result<std::sync::Arc<crate::agent::AgentConnection>> {
    use crate::agent::pool::SwarmRole;

    if let Some(id) = agent_id {
        let agent = state.agent_pool.get_agent(id)
            .ok_or_else(|| ApiError::AgentNotFound(id.to_string()).extend())?;

        match agent.swarm_role() {
            SwarmRole::Worker => {
                return Err(ApiError::Internal(format!(
                    "Agent '{}' is a swarm worker node and cannot handle swarm management queries. \
                     Specify a manager agent or omit agentId for auto-selection.",
                    id
                )).extend());
            }
            SwarmRole::None => {
                return Err(ApiError::Internal(format!(
                    "Agent '{}' is not part of any swarm. Specify an agent that is a swarm manager.",
                    id
                )).extend());
            }
            SwarmRole::Manager => { /* ok */ }
        }
        Ok(agent)
    } else {
        // Auto-select: find a healthy manager
        let agents = state.agent_pool.list_agents();
        let manager = agents.iter().find(|a| {
            a.is_healthy() && a.swarm_role() == SwarmRole::Manager
        });

        match manager {
            Some(agent) => Ok(agent.clone()),
            None => {
                // Fallback: any healthy agent
                let fallback = agents.iter().find(|a| a.is_healthy());
                match fallback {
                    Some(agent) => Ok(agent.clone()),
                    None => Err(ApiError::Internal(
                        "No healthy agents available. Cannot route swarm query.".to_string()
                    ).extend()),
                }
            }
        }
    }
}

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

    /// Get agent discovery status and configuration
    async fn discovery_status(&self, ctx: &Context<'_>) -> async_graphql::Result<DiscoveryStatusView> {
        let state = ctx.data::<AppState>()?;
        let pool = &state.agent_pool;

        let agents = pool.list_agents();
        let static_count = agents.iter().filter(|a| a.source == crate::agent::AgentSource::Static).count() as i32;
        let discovered_count = agents.iter().filter(|a| a.source == crate::agent::AgentSource::Discovered).count() as i32;
        let registered_count = agents.iter().filter(|a| a.source == crate::agent::AgentSource::Registered).count() as i32;

        Ok(DiscoveryStatusView {
            swarm_discovery_enabled: state.config.discovery.swarm_discovery,
            registration_enabled: state.config.discovery.registration_enabled,
            discovery_label: state.config.discovery.discovery_label.clone(),
            discovery_interval_secs: state.config.discovery.discovery_interval_secs as i32,
            agent_port: state.config.discovery.agent_port as i32,
            total_agents: pool.count() as i32,
            static_agents: static_count,
            discovered_agents: discovered_count,
            registered_agents: registered_count,
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

    // =========================================================================
    // Swarm Queries
    // =========================================================================

    /// Get swarm information from an agent
    async fn swarm_info(
        &self,
        ctx: &Context<'_>,
        agent_id: Option<String>,
    ) -> async_graphql::Result<Option<SwarmInfoView>> {
        let state = ctx.data::<AppState>()?;
        let agent = resolve_swarm_agent(state, agent_id.as_deref()).await?;

        let mut client = {
            let guard = agent.client.lock().await;
            guard.clone()
        };

        let response = client
            .get_swarm_info(crate::agent::client::SwarmInfoRequest {})
            .await
            .map_err(|e| ApiError::Internal(format!("Failed to get swarm info: {}", e)).extend())?;

        if !response.is_swarm_mode {
            return Ok(None);
        }

        let swarm = response.swarm.map(|s| SwarmInfoView {
            swarm_id: s.swarm_id,
            node_id: s.node_id,
            is_manager: s.is_manager,
            managers: s.managers as i32,
            workers: s.workers as i32,
            is_swarm_mode: true,
        });

        Ok(swarm)
    }

    /// List all nodes in the swarm (from a specific agent)
    async fn nodes(
        &self,
        ctx: &Context<'_>,
        agent_id: Option<String>,
    ) -> async_graphql::Result<Vec<NodeView>> {
        let state = ctx.data::<AppState>()?;
        let agent = resolve_swarm_agent(state, agent_id.as_deref()).await?;

        let mut client = {
            let guard = agent.client.lock().await;
            guard.clone()
        };

        let response = client
            .list_nodes(crate::agent::client::NodeListRequest {})
            .await
            .map_err(|e| ApiError::Internal(format!("Failed to list nodes: {}", e)).extend())?;

        // Build a map of agent addresses for node-to-agent correlation
        let all_agents = state.agent_pool.list_agents();
        let agent_addr_map: std::collections::HashMap<String, String> = all_agents.iter()
            .map(|a| (a.info.address.clone(), a.info.id.clone()))
            .collect();

        let nodes = response.nodes.into_iter().map(|n| {
            let labels = n.labels.into_iter()
                .map(|(k, v)| Label { key: k, value: v })
                .collect();

            // Correlate node address with agent pool to find the agent_id
            let correlated_agent_id = agent_addr_map.get(&n.addr)
                .cloned()
                .or_else(|| {
                    // Also try matching by hostname in agent info
                    all_agents.iter()
                        .find(|a| a.info.address.contains(&n.hostname) || n.addr.contains(&a.info.address))
                        .map(|a| a.info.id.clone())
                });

            NodeView {
                id: n.id,
                hostname: n.hostname,
                role: NodeRoleGql::from_proto(n.role),
                availability: NodeAvailabilityGql::from_proto(n.availability),
                status: NodeStatusGql::from_proto(n.status),
                addr: n.addr,
                engine_version: n.engine_version,
                os: n.os,
                architecture: n.architecture,
                labels,
                manager_status: n.manager_status.map(|ms| ManagerStatusView {
                    leader: ms.leader,
                    reachability: ms.reachability,
                    addr: ms.addr,
                }),
                nano_cpus: n.nano_cpus.to_string(),
                memory_bytes: n.memory_bytes.to_string(),
                agent_id: correlated_agent_id,
            }
        }).collect();

        Ok(nodes)
    }

    /// List all swarm services across agents
    async fn services(
        &self,
        ctx: &Context<'_>,
        agent_ids: Option<Vec<String>>,
    ) -> async_graphql::Result<Vec<ServiceView>> {
        let state = ctx.data::<AppState>()?;

        let agents = if let Some(ids) = agent_ids {
            ids.iter()
                .filter_map(|id| state.agent_pool.get_agent(id))
                .collect::<Vec<_>>()
        } else {
            state.agent_pool.list_agents()
                .into_iter()
                .filter(|a| a.health_status() == crate::agent::HealthStatus::Healthy)
                .collect()
        };

        let futures = agents.into_iter().map(|agent| async move {
            let mut client = {
                let guard = agent.client.lock().await;
                guard.clone()
            };

            match client.list_services(crate::agent::client::ServiceListRequest {}).await {
                Ok(response) => Some((agent.info.id.clone(), response.services)),
                Err(e) => {
                    tracing::warn!("Failed to list services from agent {}: {}", agent.info.id, e);
                    None
                }
            }
        });

        let results = futures::future::join_all(futures).await;

        let mut all_services = Vec::new();
        for (agent_id, services) in results.into_iter().flatten() {
            for s in services {
                all_services.push(convert_service_proto_to_view(s, &agent_id));
            }
        }

        Ok(all_services)
    }

    /// Get a specific swarm service
    async fn service(
        &self,
        ctx: &Context<'_>,
        id: String,
        agent_id: Option<String>,
    ) -> async_graphql::Result<Option<ServiceView>> {
        let state = ctx.data::<AppState>()?;
        let agent = resolve_swarm_agent(state, agent_id.as_deref()).await?;
        let resolved_agent_id = agent.info.id.clone();

        let mut client = {
            let guard = agent.client.lock().await;
            guard.clone()
        };

        let response = client
            .inspect_service(crate::agent::client::ServiceInspectRequest {
                service_id: id,
            })
            .await
            .map_err(|e| ApiError::Internal(format!("Failed to inspect service: {}", e)).extend())?;

        Ok(response.service.map(|s| convert_service_proto_to_view(s, &resolved_agent_id)))
    }

    /// List tasks for a service
    async fn tasks(
        &self,
        ctx: &Context<'_>,
        agent_id: Option<String>,
        service_id: Option<String>,
    ) -> async_graphql::Result<Vec<TaskView>> {
        let state = ctx.data::<AppState>()?;
        let agent = resolve_swarm_agent(state, agent_id.as_deref()).await?;
        let resolved_agent_id = agent.info.id.clone();

        let mut client = {
            let guard = agent.client.lock().await;
            guard.clone()
        };

        let response = client
            .list_tasks(crate::agent::client::TaskListRequest {
                service_id,
            })
            .await
            .map_err(|e| ApiError::Internal(format!("Failed to list tasks: {}", e)).extend())?;

        let tasks = response.tasks.into_iter().map(|t| {
            TaskView {
                id: t.id,
                service_id: t.service_id,
                service_name: t.service_name,
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
                agent_id: resolved_agent_id.clone(),
            }
        }).collect();

        Ok(tasks)
    }

    /// List stacks (services grouped by com.docker.stack.namespace)
    async fn stacks(
        &self,
        ctx: &Context<'_>,
        agent_ids: Option<Vec<String>>,
    ) -> async_graphql::Result<Vec<StackView>> {
        // Get all services first
        let services = self.services(ctx, agent_ids).await?;

        // Group by stack namespace
        let mut stack_map: std::collections::HashMap<(String, String), Vec<ServiceView>> =
            std::collections::HashMap::new();

        let mut non_stack_services = Vec::new();

        for service in services {
            if let Some(ref ns) = service.stack_namespace {
                stack_map
                    .entry((ns.clone(), service.agent_id.clone()))
                    .or_default()
                    .push(service);
            } else {
                non_stack_services.push(service);
            }
        }

        let stacks = stack_map.into_iter().map(|((namespace, agent_id), services)| {
            let service_count = services.len() as i32;
            let replicas_desired: i32 = services.iter().map(|s| s.replicas_desired).sum();
            let replicas_running: i32 = services.iter().map(|s| s.replicas_running).sum();

            StackView {
                namespace,
                service_count,
                replicas_desired,
                replicas_running,
                services,
                agent_id,
            }
        }).collect();

        Ok(stacks)
    }

    /// Get a single stack by namespace
    async fn stack(
        &self,
        ctx: &Context<'_>,
        namespace: String,
        agent_id: Option<String>,
    ) -> async_graphql::Result<Option<StackView>> {
        let state = ctx.data::<AppState>()?;
        let agent = resolve_swarm_agent(state, agent_id.as_deref()).await?;
        let resolved_agent_id = agent.info.id.clone();

        let mut client = {
            let guard = agent.client.lock().await;
            guard.clone()
        };

        let response = client
            .list_services(crate::agent::client::ServiceListRequest {})
            .await
            .map_err(|e| ApiError::Internal(format!("Failed to list services: {}", e)).extend())?;

        let stack_services: Vec<ServiceView> = response.services
            .into_iter()
            .filter(|s| s.stack_namespace.as_deref() == Some(&namespace))
            .map(|s| convert_service_proto_to_view(s, &resolved_agent_id))
            .collect();

        if stack_services.is_empty() {
            return Ok(None);
        }

        let service_count = stack_services.len() as i32;
        let replicas_desired: i32 = stack_services.iter().map(|s| s.replicas_desired).sum();
        let replicas_running: i32 = stack_services.iter().map(|s| s.replicas_running).sum();

        Ok(Some(StackView {
            namespace,
            service_count,
            replicas_desired,
            replicas_running,
            services: stack_services,
            agent_id: resolved_agent_id,
        }))
    }

    // =========================================================================
    // S5: Swarm Networking Queries
    // =========================================================================

    /// List swarm networks across agents
    async fn swarm_networks(
        &self,
        ctx: &Context<'_>,
        agent_ids: Option<Vec<String>>,
        #[graphql(default = true)] swarm_only: bool,
    ) -> async_graphql::Result<Vec<SwarmNetworkView>> {
        let state = ctx.data::<AppState>()?;

        let agents = if let Some(ids) = agent_ids {
            ids.iter()
                .filter_map(|id| state.agent_pool.get_agent(id))
                .collect::<Vec<_>>()
        } else {
            state.agent_pool.list_agents()
                .into_iter()
                .filter(|a| a.health_status() == crate::agent::HealthStatus::Healthy)
                .collect()
        };

        let futures = agents.into_iter().map(|agent| async move {
            let mut client = {
                let guard = agent.client.lock().await;
                guard.clone()
            };

            match client.list_swarm_networks(swarm_only).await {
                Ok(response) => Some((agent.info.id.clone(), response.networks)),
                Err(e) => {
                    tracing::warn!("Failed to list networks from agent {}: {}", agent.info.id, e);
                    None
                }
            }
        });

        let results = futures::future::join_all(futures).await;

        let mut all_networks = Vec::new();
        for (agent_id, networks) in results.into_iter().flatten() {
            for n in networks {
                all_networks.push(convert_network_proto_to_view(n, &agent_id));
            }
        }

        Ok(all_networks)
    }

    /// Inspect a specific swarm network
    async fn swarm_network(
        &self,
        ctx: &Context<'_>,
        id: String,
        agent_id: Option<String>,
    ) -> async_graphql::Result<Option<SwarmNetworkView>> {
        let state = ctx.data::<AppState>()?;
        let agent = resolve_swarm_agent(state, agent_id.as_deref()).await?;
        let resolved_agent_id = agent.info.id.clone();

        let mut client = {
            let guard = agent.client.lock().await;
            guard.clone()
        };

        let response = client
            .inspect_swarm_network(&id)
            .await
            .map_err(|e| ApiError::Internal(format!("Failed to inspect network: {}", e)).extend())?;

        Ok(response.network.map(|n| convert_network_proto_to_view(n, &resolved_agent_id)))
    }

    // =========================================================================
    // S7: Side-by-Side Replica Log Comparison
    // =========================================================================

    /// Get all running replicas of a service as comparison sources.
    ///
    /// Returns a `ComparisonSource` for each running task, ready to be
    /// passed into the `comparisonLogStream` subscription for side-by-side
    /// log comparison.
    ///
    /// # Example
    /// ```graphql
    /// query {
    ///   serviceReplicas(serviceId: "abc123", agentId: "agent-local") {
    ///     containerId
    ///     serviceId
    ///     taskId
    ///     agentId
    ///     label
    ///     slot
    ///     nodeId
    ///     state
    ///   }
    /// }
    /// ```
    #[graphql(name = "serviceReplicas")]
    async fn service_replicas(
        &self,
        ctx: &Context<'_>,
        service_id: String,
        agent_id: Option<String>,
    ) -> async_graphql::Result<Vec<ComparisonSource>> {
        let state = ctx.data::<AppState>()?;
        let agent = resolve_swarm_agent(state, agent_id.as_deref()).await?;
        let resolved_agent_id = agent.info.id.clone();

        let mut client = {
            let guard = agent.client.lock().await;
            guard.clone()
        };

        // Get the service name for building labels
        let service_name = {
            let inspect_resp = client
                .inspect_service(crate::agent::client::ServiceInspectRequest {
                    service_id: service_id.clone(),
                })
                .await
                .map_err(|e| ApiError::Internal(format!("Failed to inspect service: {}", e)).extend())?;

            inspect_resp.service
                .map(|s| s.name)
                .unwrap_or_else(|| service_id.clone())
        };

        // List all tasks for this service
        let response = client
            .list_tasks(crate::agent::client::TaskListRequest {
                service_id: Some(service_id.clone()),
            })
            .await
            .map_err(|e| ApiError::Internal(format!("Failed to list tasks: {}", e)).extend())?;

        // Filter to running tasks and convert to ComparisonSource
        let sources = response.tasks
            .into_iter()
            .filter(|t| t.state == "running" && t.desired_state == "running")
            .map(|t| {
                let slot = t.slot.map(|s| s as i32);
                let label = if let Some(s) = slot {
                    format!("{}.{}", service_name, s)
                } else {
                    format!("{}.{}", service_name, &t.id[..12.min(t.id.len())])
                };
                ComparisonSource {
                    container_id: t.container_id,
                    service_id: service_id.clone(),
                    task_id: t.id,
                    agent_id: resolved_agent_id.clone(),
                    label,
                    slot,
                    node_id: t.node_id,
                    state: t.state,
                }
            })
            .collect();

        Ok(sources)
    }

    // =========================================================================
    // S8: Swarm Secrets & Configs
    // =========================================================================

    /// List swarm secrets (metadata only — never exposes actual secret data)
    ///
    /// # Example
    /// ```graphql
    /// query {
    ///   swarmSecrets(agentIds: ["agent-local"]) {
    ///     id
    ///     name
    ///     createdAt
    ///     labels { key value }
    ///     driver
    ///   }
    /// }
    /// ```
    #[graphql(name = "swarmSecrets")]
    async fn swarm_secrets(
        &self,
        ctx: &Context<'_>,
        agent_ids: Option<Vec<String>>,
    ) -> async_graphql::Result<Vec<SwarmSecretView>> {
        let state = ctx.data::<AppState>()?;

        let agents = if let Some(ids) = agent_ids {
            ids.iter()
                .filter_map(|id| state.agent_pool.get_agent(id))
                .collect::<Vec<_>>()
        } else {
            state.agent_pool.list_agents()
                .into_iter()
                .filter(|a| a.health_status() == crate::agent::HealthStatus::Healthy)
                .collect()
        };

        let futures = agents.into_iter().map(|agent| async move {
            let mut client = {
                let guard = agent.client.lock().await;
                guard.clone()
            };

            match client.list_secrets().await {
                Ok(response) => Some((agent.info.id.clone(), response.secrets)),
                Err(e) => {
                    tracing::warn!("Failed to list secrets from agent {}: {}", agent.info.id, e);
                    None
                }
            }
        });

        let results = futures::future::join_all(futures).await;

        let mut all_secrets = Vec::new();
        for (agent_id, secrets) in results.into_iter().flatten() {
            for s in secrets {
                let labels = s.labels.into_iter()
                    .map(|(k, v)| Label { key: k, value: v })
                    .collect();
                all_secrets.push(SwarmSecretView {
                    id: s.id,
                    name: s.name,
                    created_at: chrono::DateTime::from_timestamp(s.created_at, 0)
                        .unwrap_or_else(chrono::Utc::now),
                    updated_at: chrono::DateTime::from_timestamp(s.updated_at, 0)
                        .unwrap_or_else(chrono::Utc::now),
                    labels,
                    driver: s.driver,
                    agent_id: agent_id.clone(),
                });
            }
        }

        Ok(all_secrets)
    }

    /// List swarm configs (metadata only — data content is omitted)
    ///
    /// # Example
    /// ```graphql
    /// query {
    ///   swarmConfigs(agentIds: ["agent-local"]) {
    ///     id
    ///     name
    ///     createdAt
    ///     labels { key value }
    ///   }
    /// }
    /// ```
    #[graphql(name = "swarmConfigs")]
    async fn swarm_configs(
        &self,
        ctx: &Context<'_>,
        agent_ids: Option<Vec<String>>,
    ) -> async_graphql::Result<Vec<SwarmConfigView>> {
        let state = ctx.data::<AppState>()?;

        let agents = if let Some(ids) = agent_ids {
            ids.iter()
                .filter_map(|id| state.agent_pool.get_agent(id))
                .collect::<Vec<_>>()
        } else {
            state.agent_pool.list_agents()
                .into_iter()
                .filter(|a| a.health_status() == crate::agent::HealthStatus::Healthy)
                .collect()
        };

        let futures = agents.into_iter().map(|agent| async move {
            let mut client = {
                let guard = agent.client.lock().await;
                guard.clone()
            };

            match client.list_configs().await {
                Ok(response) => Some((agent.info.id.clone(), response.configs)),
                Err(e) => {
                    tracing::warn!("Failed to list configs from agent {}: {}", agent.info.id, e);
                    None
                }
            }
        });

        let results = futures::future::join_all(futures).await;

        let mut all_configs = Vec::new();
        for (agent_id, configs) in results.into_iter().flatten() {
            for c in configs {
                let labels = c.labels.into_iter()
                    .map(|(k, v)| Label { key: k, value: v })
                    .collect();
                all_configs.push(SwarmConfigView {
                    id: c.id,
                    name: c.name,
                    created_at: chrono::DateTime::from_timestamp(c.created_at, 0)
                        .unwrap_or_else(chrono::Utc::now),
                    updated_at: chrono::DateTime::from_timestamp(c.updated_at, 0)
                        .unwrap_or_else(chrono::Utc::now),
                    labels,
                    agent_id: agent_id.clone(),
                });
            }
        }

        Ok(all_configs)
    }

    // =========================================================================
    // S9: Node Management & Drain Awareness
    // =========================================================================

    /// Inspect a single node by ID
    async fn node(
        &self,
        ctx: &Context<'_>,
        agent_id: Option<String>,
        node_id: String,
    ) -> async_graphql::Result<Option<NodeView>> {
        let state = ctx.data::<AppState>()?;
        let agent = resolve_swarm_agent(state, agent_id.as_deref()).await?;
        let resolved_agent_id = agent.info.id.clone();

        let mut client = {
            let guard = agent.client.lock().await;
            guard.clone()
        };

        let response = client
            .inspect_node(&node_id)
            .await
            .map_err(|e| ApiError::Internal(format!("Failed to inspect node: {}", e)).extend())?;

        let node = response.node.map(|n| {
            let labels = n.labels.into_iter()
                .map(|(k, v)| Label { key: k, value: v })
                .collect();

            NodeView {
                id: n.id,
                hostname: n.hostname,
                role: NodeRoleGql::from_proto(n.role),
                availability: NodeAvailabilityGql::from_proto(n.availability),
                status: NodeStatusGql::from_proto(n.status),
                addr: n.addr,
                engine_version: n.engine_version,
                os: n.os,
                architecture: n.architecture,
                labels,
                manager_status: n.manager_status.map(|ms| ManagerStatusView {
                    leader: ms.leader,
                    reachability: ms.reachability,
                    addr: ms.addr,
                }),
                nano_cpus: n.nano_cpus.to_string(),
                memory_bytes: n.memory_bytes.to_string(),
                agent_id: Some(resolved_agent_id.clone()),
            }
        });

        Ok(node)
    }

    // =========================================================================
    // S10: Service Coverage
    // =========================================================================

    /// Get coverage info for a service — which nodes have tasks and which don't.
    /// Most useful for global services, but works for any service.
    async fn service_coverage(
        &self,
        ctx: &Context<'_>,
        agent_id: Option<String>,
        service_id: String,
    ) -> async_graphql::Result<ServiceCoverageView> {
        let state = ctx.data::<AppState>()?;
        let agent = resolve_swarm_agent(state, agent_id.as_deref()).await?;
        let resolved_agent_id = agent.info.id.clone();

        let mut client = {
            let guard = agent.client.lock().await;
            guard.clone()
        };

        let response = client
            .get_service_coverage(&service_id)
            .await
            .map_err(|e| ApiError::Internal(format!("Failed to get service coverage: {}", e)).extend())?;

        let coverage = response.coverage
            .ok_or_else(|| ApiError::Internal("No coverage data returned".to_string()).extend())?;

        Ok(ServiceCoverageView {
            covered_nodes: coverage.covered_nodes,
            uncovered_nodes: coverage.uncovered_nodes,
            total_nodes: coverage.total_nodes as i32,
            coverage_percentage: coverage.coverage_percentage,
            service_id: coverage.service_id,
            is_global: coverage.is_global,
            agent_id: resolved_agent_id,
        })
    }

    // =========================================================================
    // S11: Stack Health
    // =========================================================================

    /// Get aggregated health for all services in a stack.
    /// Returns per-service health breakdown, restart policies, and overall stack status.
    async fn stack_health(
        &self,
        ctx: &Context<'_>,
        agent_id: Option<String>,
        namespace: String,
    ) -> async_graphql::Result<StackHealthView> {
        let state = ctx.data::<AppState>()?;
        let agent = resolve_swarm_agent(state, agent_id.as_deref()).await?;
        let resolved_agent_id = agent.info.id.clone();

        let mut client = {
            let guard = agent.client.lock().await;
            guard.clone()
        };

        let response = client
            .get_stack_health(&namespace)
            .await
            .map_err(|e| ApiError::Internal(format!("Failed to get stack health: {}", e)).extend())?;

        let health = response.health
            .ok_or_else(|| ApiError::Internal("No health data returned".to_string()).extend())?;

        Ok(StackHealthView {
            namespace: health.namespace,
            overall_status: StackHealthStatusGql::from_proto(health.overall_status),
            service_healths: health.service_healths.into_iter().map(|sh| {
                ServiceHealthView {
                    service_id: sh.service_id,
                    service_name: sh.service_name,
                    health_status: ServiceHealthStatusGql::from_proto(sh.health_status),
                    replicas_desired: sh.replicas_desired as i32,
                    replicas_running: sh.replicas_running as i32,
                    replicas_failed: sh.replicas_failed as i32,
                    recent_errors: sh.recent_errors,
                    update_in_progress: sh.update_in_progress,
                    restart_policy: sh.restart_policy.map(|rp| RestartPolicyView {
                        condition: rp.condition,
                        delay_ns: rp.delay_ns.to_string(),
                        max_attempts: rp.max_attempts as i32,
                        window_ns: rp.window_ns.to_string(),
                    }),
                }
            }).collect(),
            total_services: health.total_services as i32,
            healthy_services: health.healthy_services as i32,
            degraded_services: health.degraded_services as i32,
            unhealthy_services: health.unhealthy_services as i32,
            total_desired: health.total_desired as i32,
            total_running: health.total_running as i32,
            total_failed: health.total_failed as i32,
            agent_id: resolved_agent_id,
        })
    }

    // =========================================================================
    // B03: Task Inspect
    // =========================================================================

    /// Inspect a single task in detail.
    async fn task_inspect(
        &self,
        ctx: &Context<'_>,
        agent_id: Option<String>,
        task_id: String,
    ) -> async_graphql::Result<Option<TaskInspectView>> {
        let state = ctx.data::<AppState>()?;
        let agent = resolve_swarm_agent(state, agent_id.as_deref()).await?;

        let mut client = {
            let guard = agent.client.lock().await;
            guard.clone()
        };

        let response = client
            .inspect_task(crate::agent::client::TaskInspectRequest {
                task_id: task_id.clone(),
            })
            .await
            .map_err(|e| ApiError::Internal(format!("Failed to inspect task: {}", e)).extend())?;

        Ok(response.task.map(|t| {
            let env: Vec<Label> = t.env.into_iter()
                .map(|(k, v)| Label { key: k, value: v })
                .collect();
            let labels: Vec<Label> = t.labels.into_iter()
                .map(|(k, v)| Label { key: k, value: v })
                .collect();
            let network_attachments = t.network_attachments.into_iter()
                .map(|na| TaskNetworkAttachmentView {
                    network_id: na.network_id,
                    network_name: na.network_name,
                    addresses: na.addresses,
                })
                .collect();
            let ports = t.port_status.into_iter()
                .map(|p| ServicePortView {
                    protocol: p.protocol,
                    target_port: p.target_port as i32,
                    published_port: p.published_port as i32,
                    publish_mode: p.publish_mode,
                })
                .collect();

            TaskInspectView {
                id: t.id,
                service_id: t.service_id,
                service_name: t.service_name,
                node_id: t.node_id,
                slot: t.slot.map(|s| s.to_string()),
                container_id: t.container_id,
                state: t.state,
                desired_state: t.desired_state,
                status_message: t.status_message,
                status_err: t.status_err,
                created_at: t.created_at,
                updated_at: t.updated_at,
                exit_code: t.exit_code,
                image: t.image,
                command: t.command,
                args: t.args,
                env,
                labels,
                network_attachments,
                resource_limits: t.resource_limits.map(|r| ResourceView {
                    nano_cpus: r.nano_cpus.to_string(),
                    memory_bytes: r.memory_bytes.to_string(),
                }),
                resource_reservations: t.resource_reservations.map(|r| ResourceView {
                    nano_cpus: r.nano_cpus.to_string(),
                    memory_bytes: r.memory_bytes.to_string(),
                }),
                restart_policy: t.restart_policy.map(|rp| RestartPolicyView {
                    condition: rp.condition,
                    delay_ns: rp.delay_ns.to_string(),
                    max_attempts: rp.max_attempts as i32,
                    window_ns: rp.window_ns.to_string(),
                }),
                started_at: t.started_at,
                finished_at: t.finished_at,
                ports,
            }
        }))
    }

    // =========================================================================
    // B12: Stack File Viewer
    // =========================================================================

    /// Retrieve the stored compose YAML for a deployed stack.
    async fn stack_file(
        &self,
        ctx: &Context<'_>,
        agent_id: Option<String>,
        stack_name: String,
    ) -> async_graphql::Result<StackFileResult> {
        let state = ctx.data::<AppState>()?;
        let agent = resolve_swarm_agent(state, agent_id.as_deref()).await?;

        let mut client = {
            let guard = agent.client.lock().await;
            guard.clone()
        };

        let response = client
            .get_stack_file(crate::agent::client::GetStackFileRequest {
                stack_name: stack_name.clone(),
            })
            .await
            .map_err(|e| ApiError::Internal(format!("Failed to get stack file: {}", e)).extend())?;

        Ok(StackFileResult {
            found: response.found,
            stack_name: response.stack_name,
            compose_yaml: response.compose_yaml,
        })
    }
}

/// Convert proto ServiceInfo to GraphQL ServiceView
fn convert_service_proto_to_view(
    s: crate::agent::client::proto::ServiceInfo,
    agent_id: &str,
) -> ServiceView {
    let labels = s.labels.into_iter()
        .map(|(k, v)| Label { key: k, value: v })
        .collect();

    let ports = s.ports.into_iter().map(|p| ServicePortView {
        protocol: p.protocol,
        target_port: p.target_port as i32,
        published_port: p.published_port as i32,
        publish_mode: p.publish_mode,
    }).collect();

    let update_status = s.update_status.map(|us| UpdateStatusView {
        state: us.state,
        started_at: us.started_at.and_then(|ts| chrono::DateTime::from_timestamp(ts, 0)),
        completed_at: us.completed_at.and_then(|ts| chrono::DateTime::from_timestamp(ts, 0)),
        message: us.message,
    });

    ServiceView {
        id: s.id,
        name: s.name,
        image: s.image,
        mode: ServiceModeGql::from_proto(s.mode),
        replicas_desired: s.replicas_desired as i32,
        replicas_running: s.replicas_running as i32,
        labels,
        stack_namespace: s.stack_namespace,
        created_at: chrono::DateTime::from_timestamp(s.created_at, 0)
            .unwrap_or_else(chrono::Utc::now),
        updated_at: chrono::DateTime::from_timestamp(s.updated_at, 0)
            .unwrap_or_else(chrono::Utc::now),
        ports,
        update_status,
        placement_constraints: s.placement_constraints,
        networks: s.networks,
        agent_id: agent_id.to_string(),
        // S6: Update/rollback config and placement
        update_config: s.update_config.map(|uc| UpdateConfigView {
            parallelism: uc.parallelism as i32,
            delay_ns: uc.delay_ns.to_string(),
            failure_action: uc.failure_action,
            monitor_ns: uc.monitor_ns.to_string(),
            max_failure_ratio: uc.max_failure_ratio,
            order: uc.order,
        }),
        rollback_config: s.rollback_config.map(|rc| UpdateConfigView {
            parallelism: rc.parallelism as i32,
            delay_ns: rc.delay_ns.to_string(),
            failure_action: rc.failure_action,
            monitor_ns: rc.monitor_ns.to_string(),
            max_failure_ratio: rc.max_failure_ratio,
            order: rc.order,
        }),
        placement: s.placement.map(|p| ServicePlacementView {
            constraints: p.constraints,
            preferences: p.preferences.into_iter().map(|pref| PlacementPreferenceView {
                spread_descriptor: pref.spread_descriptor,
            }).collect(),
            max_replicas_per_node: p.max_replicas_per_node.map(|m| m as i32),
            platforms: p.platforms.into_iter().map(|plat| PlatformView {
                architecture: plat.architecture,
                os: plat.os,
            }).collect(),
        }),
        // S8: Secret and config references
        secret_references: s.secret_references.into_iter().map(|sr| SecretReferenceView {
            secret_id: sr.secret_id,
            secret_name: sr.secret_name,
            file_name: sr.file_name,
            file_uid: sr.file_uid,
            file_gid: sr.file_gid,
            file_mode: sr.file_mode as i32,
        }).collect(),
        config_references: s.config_references.into_iter().map(|cr| ConfigReferenceView {
            config_id: cr.config_id,
            config_name: cr.config_name,
            file_name: cr.file_name,
            file_uid: cr.file_uid,
            file_gid: cr.file_gid,
            file_mode: cr.file_mode as i32,
        }).collect(),
        // S11: Restart policy
        restart_policy: s.restart_policy.map(|rp| RestartPolicyView {
            condition: rp.condition,
            delay_ns: rp.delay_ns.to_string(),
            max_attempts: rp.max_attempts as i32,
            window_ns: rp.window_ns.to_string(),
        }),
    }
}

/// Convert proto SwarmNetworkInfo to GraphQL SwarmNetworkView
fn convert_network_proto_to_view(
    n: crate::agent::client::proto::SwarmNetworkInfo,
    agent_id: &str,
) -> SwarmNetworkView {
    let labels = n.labels.into_iter()
        .map(|(k, v)| Label { key: k, value: v })
        .collect();

    let options = n.options.into_iter()
        .map(|(k, v)| Label { key: k, value: v })
        .collect();

    let ipam_configs = n.ipam_configs.into_iter().map(|c| IpamConfigView {
        subnet: c.subnet.unwrap_or_default(),
        gateway: c.gateway.unwrap_or_default(),
        ip_range: c.ip_range.unwrap_or_default(),
    }).collect();

    let peers = n.peers.into_iter().map(|p| PeerInfoView {
        name: p.name,
        ip: p.ip,
    }).collect();

    let service_attachments = n.service_attachments.into_iter().map(|sa| NetworkServiceAttachmentView {
        service_id: sa.service_id,
        service_name: sa.service_name,
        virtual_ip: sa.virtual_ip,
    }).collect();

    SwarmNetworkView {
        id: n.id,
        name: n.name,
        driver: n.driver,
        scope: n.scope,
        is_internal: n.is_internal,
        is_attachable: n.is_attachable,
        is_ingress: n.is_ingress,
        enable_ipv6: n.enable_ipv6,
        created_at: chrono::DateTime::from_timestamp(n.created_at, 0)
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_default(),
        labels,
        options,
        ipam_configs,
        peers,
        service_attachments,
        agent_id: agent_id.to_string(),
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

    Schema::build(QueryRoot, MutationRoot, SubscriptionRoot)
        .data(state)
        .data(ContainerDetailsCache::new())
        .data(ContainerLookupCache::new())
        .limit_depth(max_depth)
        .limit_complexity(max_complexity)
        .finish()
}
