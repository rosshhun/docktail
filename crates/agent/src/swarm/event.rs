//! Event — service update/event streaming, network inspection, and network operations.

use std::pin::Pin;
use futures_util::stream::Stream;
use tonic::Status;
use tracing::{info, warn};

use crate::docker::client::DockerClient;
use crate::swarm::map::{convert_network_to_proto, convert_task_to_proto};
use crate::proto::{
    SwarmNetworkInfo, SwarmNetworkListResponse,
    NetworkConnectResponse, NetworkDisconnectResponse,
    ServiceUpdateEvent, TaskStateChange,
    ServiceEvent, ServiceEventType,
};

/// Convert a `NetworkInspect` (from inspect_network) into a
/// `SwarmNetworkInfo` proto, reusing the `convert_network_to_proto` helper.
pub(crate) async fn inspect_network(
    docker: &DockerClient,
    network_id: &str,
) -> Result<SwarmNetworkInfo, Status> {
    let net = docker.inspect_network(network_id).await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("404") || msg.to_lowercase().contains("not found") {
                Status::not_found(format!("Network not found: {}", e))
            } else if msg.contains("403") || msg.to_lowercase().contains("permission") {
                Status::permission_denied(format!("Permission denied inspecting network: {}", e))
            } else {
                Status::internal(format!("Failed to inspect network: {}", e))
            }
        })?;

    let services = docker.list_services().await.unwrap_or_default();

    let network_as_model = bollard::models::Network {
        name: net.name,
        id: net.id,
        created: net.created,
        scope: net.scope,
        driver: net.driver,
        enable_ipv6: net.enable_ipv6,
        ipam: net.ipam,
        internal: net.internal,
        attachable: net.attachable,
        ingress: net.ingress,
        options: net.options,
        labels: net.labels,
        peers: net.peers,
        ..Default::default()
    };

    Ok(convert_network_to_proto(&network_as_model, &services))
}

/// Build a polling stream that tracks rolling-update progress for a
/// single service (S6).
pub(crate) fn update_stream(
    docker: DockerClient,
    service_id: String,
    poll_ms: u64,
) -> Pin<Box<dyn Stream<Item = Result<ServiceUpdateEvent, Status>> + Send>> {
    let stream = async_stream::try_stream! {
        let mut prev_task_states: std::collections::HashMap<String, (String, i64)> = std::collections::HashMap::new();

        loop {
            let service: bollard::models::Service = docker.inspect_service(&service_id).await
                .map_err(|e| Status::internal(format!("Failed to inspect service: {}", e)))?;

            let update_status = service.update_status.as_ref();
            let update_state = update_status
                .and_then(|us| us.state.as_ref())
                .map(|s| format!("{}", s))
                .unwrap_or_else(|| "none".to_string());
            let started_at: Option<i64> = update_status
                .and_then(|us| us.started_at.as_ref())
                .and_then(|d: &String| chrono::DateTime::parse_from_rfc3339(d).ok())
                .map(|dt: chrono::DateTime<chrono::FixedOffset>| dt.timestamp());
            let completed_at: Option<i64> = update_status
                .and_then(|us| us.completed_at.as_ref())
                .and_then(|d: &String| chrono::DateTime::parse_from_rfc3339(d).ok())
                .map(|dt: chrono::DateTime<chrono::FixedOffset>| dt.timestamp());
            let message = update_status
                .and_then(|us| us.message.clone())
                .unwrap_or_default();

            let tasks: Vec<bollard::models::Task> = match docker.list_tasks().await {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!(service_id = %service_id, error = %e, "Failed to list tasks in service update stream — skipping this poll cycle");
                    tokio::time::sleep(std::time::Duration::from_millis(poll_ms)).await;
                    continue;
                }
            };
            let service_tasks: Vec<_> = tasks.into_iter()
                .filter(|t| t.service_id.as_deref() == Some(&service_id))
                .collect();

            let mut total = 0u64;
            let mut running = 0u64;
            let mut ready = 0u64;
            let mut failed = 0u64;
            let mut shutdown = 0u64;
            let mut recent_changes = Vec::new();

            for t in &service_tasks {
                total += 1;
                let task_state = t.status.as_ref()
                    .and_then(|s| s.state.as_ref())
                    .map(|s| format!("{:?}", s).to_lowercase())
                    .unwrap_or_else(|| "unknown".to_string());

                match task_state.as_str() {
                    "running" => running += 1,
                    "ready" | "starting" | "assigned" | "accepted" | "preparing" => ready += 1,
                    "failed" | "rejected" => failed += 1,
                    "shutdown" | "complete" | "remove" | "orphaned" => shutdown += 1,
                    _ => {}
                }

                let task_id = t.id.as_deref().unwrap_or("");
                let updated_at: i64 = t.updated_at.as_ref()
                    .and_then(|d: &String| chrono::DateTime::parse_from_rfc3339(d).ok())
                    .map(|dt: chrono::DateTime<chrono::FixedOffset>| dt.timestamp())
                    .unwrap_or(0);

                let changed = match prev_task_states.get(task_id) {
                    Some((prev_state, prev_ts)) => &task_state != prev_state || updated_at != *prev_ts,
                    None => true,
                };

                if changed {
                    recent_changes.push(TaskStateChange {
                        task_id: task_id.to_string(),
                        service_id: service_id.clone(),
                        node_id: t.node_id.clone().unwrap_or_default(),
                        slot: t.slot.map(|s| s as u64),
                        state: task_state.clone(),
                        desired_state: t.desired_state.as_ref()
                            .map(|s| format!("{:?}", s).to_lowercase())
                            .unwrap_or_default(),
                        message: t.status.as_ref()
                            .and_then(|s| s.message.clone())
                            .unwrap_or_default(),
                        error: t.status.as_ref()
                            .and_then(|s| s.err.clone()),
                        updated_at,
                    });
                }

                prev_task_states.insert(task_id.to_string(), (task_state, updated_at));
            }

            let current_task_ids: std::collections::HashSet<&str> = service_tasks.iter()
                .filter_map(|t| t.id.as_deref())
                .collect();
            prev_task_states.retain(|id, _| current_task_ids.contains(id.as_str()));

            let now = chrono::Utc::now().timestamp();

            yield ServiceUpdateEvent {
                update_state: update_state.clone(),
                started_at,
                completed_at,
                message,
                tasks_total: total,
                tasks_running: running,
                tasks_ready: ready,
                tasks_failed: failed,
                tasks_shutdown: shutdown,
                snapshot_at: now,
                recent_changes,
            };

            if matches!(update_state.as_str(), "completed" | "rollback_completed") {
                break;
            }

            tokio::time::sleep(std::time::Duration::from_millis(poll_ms)).await;
        }
    };

    Box::pin(stream)
}

/// List swarm networks, optionally filtered to swarm-scope only.
pub(crate) async fn list_networks(
    docker: &DockerClient,
    swarm_only: bool,
) -> Result<SwarmNetworkListResponse, Status> {
    let networks = docker.list_networks().await
        .map_err(|e| Status::internal(format!("Failed to list networks: {}", e)))?;
    let services = docker.list_services().await.unwrap_or_default();
    let networks: Vec<SwarmNetworkInfo> = networks.iter()
        .filter(|n| {
            if swarm_only { n.scope.as_deref() == Some("swarm") } else { true }
        })
        .map(|n| convert_network_to_proto(n, &services))
        .collect();
    Ok(SwarmNetworkListResponse { networks })
}

/// Connect a container to a network.
pub(crate) async fn connect_network(
    docker: &DockerClient,
    network_id: &str,
    container_id: &str,
) -> Result<NetworkConnectResponse, Status> {
    info!(network_id = %network_id, container_id = %container_id, "Connecting container to network");
    match docker.network_connect(network_id, container_id).await {
        Ok(()) => Ok(NetworkConnectResponse {
            success: true,
            message: format!("Container {} connected to network {}", container_id, network_id),
        }),
        Err(e) => Err(Status::internal(format!("Failed to connect to network: {}", e))),
    }
}

/// Disconnect a container from a network.
pub(crate) async fn disconnect_network(
    docker: &DockerClient,
    network_id: &str,
    container_id: &str,
    force: bool,
) -> Result<NetworkDisconnectResponse, Status> {
    info!(network_id = %network_id, container_id = %container_id, "Disconnecting container from network");
    match docker.network_disconnect(network_id, container_id, force).await {
        Ok(()) => Ok(NetworkDisconnectResponse {
            success: true,
            message: format!("Container {} disconnected from network {}", container_id, network_id),
        }),
        Err(e) => Err(Status::internal(format!("Failed to disconnect from network: {}", e))),
    }
}

/// Build a polling stream that emits `ServiceEvent` items when service
/// scaling, update status, or task states change (S10).
pub(crate) fn service_event_stream(
    docker: DockerClient,
    service_id: String,
    poll_ms: u64,
) -> Pin<Box<dyn Stream<Item = Result<ServiceEvent, Status>> + Send>> {
    let stream = async_stream::try_stream! {
        let mut prev_replicas_desired: Option<u64> = None;
        let mut prev_replicas_running: Option<u64> = None;
        let mut prev_update_state: Option<String> = None;
        let mut prev_task_states: std::collections::HashMap<String, (String, String)> = std::collections::HashMap::new();

        // Seed initial state
        if let Ok(services) = docker.list_services().await {
            if let Some(svc) = services.iter().find(|s| s.id.as_deref() == Some(&service_id)) {
                let spec = svc.spec.as_ref();
                let mode_spec = spec.and_then(|sp| sp.mode.as_ref());
                if let Some(mode) = mode_spec {
                    if let Some(replicated) = &mode.replicated {
                        prev_replicas_desired = Some(replicated.replicas.unwrap_or(1) as u64);
                    }
                }
                prev_update_state = svc.update_status.as_ref()
                    .and_then(|us| us.state.as_ref())
                    .map(|s| format!("{:?}", s).to_lowercase());
            }
        }

        if let Ok(tasks) = docker.list_tasks().await {
            for t in tasks.iter().filter(|t| t.service_id.as_deref() == Some(&service_id)) {
                let task_id = t.id.clone().unwrap_or_default();
                let task_state = t.status.as_ref()
                    .and_then(|s| s.state.as_ref())
                    .map(|s| format!("{:?}", s).to_lowercase())
                    .unwrap_or_else(|| "unknown".to_string());
                let desired = t.desired_state.as_ref()
                    .map(|s| format!("{:?}", s).to_lowercase())
                    .unwrap_or_else(|| "unknown".to_string());
                prev_task_states.insert(task_id, (task_state, desired));
            }
            prev_replicas_running = Some(tasks.iter()
                .filter(|t| t.service_id.as_deref() == Some(&service_id))
                .filter(|t| {
                    t.status.as_ref()
                        .and_then(|s| s.state.as_ref())
                        .map(|s| matches!(s, bollard::models::TaskState::RUNNING))
                        .unwrap_or(false)
                })
                .count() as u64);
        }

        loop {
            tokio::time::sleep(std::time::Duration::from_millis(poll_ms)).await;

            let now = chrono::Utc::now().timestamp();

            let services = match docker.list_services().await {
                Ok(s) => s,
                Err(e) => {
                    warn!("Failed to list services in event stream: {}", e);
                    continue;
                }
            };

            let svc = match services.iter().find(|s| s.id.as_deref() == Some(&service_id)) {
                Some(s) => s,
                None => {
                    Err(Status::not_found(format!("Service {} no longer exists", service_id)))?;
                    unreachable!()
                }
            };

            let spec = svc.spec.as_ref();
            let mode_spec = spec.and_then(|sp| sp.mode.as_ref());

            let current_desired = mode_spec.and_then(|m| {
                m.replicated.as_ref().map(|r| r.replicas.unwrap_or(1) as u64)
            });

            // Detect scaling events
            if let (Some(prev), Some(curr)) = (prev_replicas_desired, current_desired) {
                if prev != curr {
                    let event_type = if curr > prev {
                        ServiceEventType::ServiceEventScaledUp as i32
                    } else {
                        ServiceEventType::ServiceEventScaledDown as i32
                    };
                    yield ServiceEvent {
                        service_id: service_id.clone(),
                        event_type,
                        previous_replicas: Some(prev),
                        current_replicas: Some(curr),
                        timestamp: now,
                        message: format!("Service scaled from {} to {} replicas", prev, curr),
                        affected_tasks: Vec::new(),
                    };
                }
            }
            if let Some(d) = current_desired {
                prev_replicas_desired = Some(d);
            }

            // Detect update state changes
            let current_update_state = svc.update_status.as_ref()
                .and_then(|us| us.state.as_ref())
                .map(|s| format!("{:?}", s).to_lowercase());

            if current_update_state != prev_update_state {
                if let Some(ref curr_state) = current_update_state {
                    let event_type = match curr_state.as_str() {
                        "updating" => Some(ServiceEventType::ServiceEventUpdateStarted as i32),
                        "completed" => Some(ServiceEventType::ServiceEventUpdateCompleted as i32),
                        "rolledback" | "rollback_completed" => Some(ServiceEventType::ServiceEventUpdateRolledBack as i32),
                        _ => None,
                    };
                    if let Some(et) = event_type {
                        let message = svc.update_status.as_ref()
                            .and_then(|us| us.message.clone())
                            .unwrap_or_default();
                        yield ServiceEvent {
                            service_id: service_id.clone(),
                            event_type: et,
                            previous_replicas: prev_replicas_desired,
                            current_replicas: current_desired,
                            timestamp: now,
                            message,
                            affected_tasks: Vec::new(),
                        };
                    }
                }
            }
            prev_update_state = current_update_state;

            // Fetch current tasks for task-level events
            let tasks = match docker.list_tasks().await {
                Ok(t) => t,
                Err(e) => {
                    warn!("Failed to list tasks in service event stream: {}", e);
                    continue;
                }
            };

            let service_tasks: Vec<&bollard::models::Task> = tasks.iter()
                .filter(|t| t.service_id.as_deref() == Some(&service_id))
                .collect();

            let current_running = service_tasks.iter()
                .filter(|t| {
                    t.status.as_ref()
                        .and_then(|s| s.state.as_ref())
                        .map(|s| matches!(s, bollard::models::TaskState::RUNNING))
                        .unwrap_or(false)
                })
                .count() as u64;

            let mut new_task_states: std::collections::HashMap<String, (String, String)> = std::collections::HashMap::new();
            for t in &service_tasks {
                let task_id = t.id.clone().unwrap_or_default();
                let task_state = t.status.as_ref()
                    .and_then(|s| s.state.as_ref())
                    .map(|s| format!("{:?}", s).to_lowercase())
                    .unwrap_or_else(|| "unknown".to_string());
                let desired = t.desired_state.as_ref()
                    .map(|s| format!("{:?}", s).to_lowercase())
                    .unwrap_or_else(|| "unknown".to_string());

                if let Some((prev_state, _)) = prev_task_states.get(&task_id) {
                    if prev_state != &task_state {
                        if matches!(task_state.as_str(), "failed" | "rejected") {
                            yield ServiceEvent {
                                service_id: service_id.clone(),
                                event_type: ServiceEventType::ServiceEventTaskFailed as i32,
                                previous_replicas: prev_replicas_running,
                                current_replicas: Some(current_running),
                                timestamp: now,
                                message: format!(
                                    "Task {} failed: {}",
                                    task_id,
                                    t.status.as_ref()
                                        .and_then(|s| s.err.as_ref())
                                        .or_else(|| t.status.as_ref().and_then(|s| s.message.as_ref()))
                                        .unwrap_or(&"unknown error".to_string())
                                ),
                                affected_tasks: vec![convert_task_to_proto(t)],
                            };
                        }
                    }
                } else {
                    if task_state == "running" && prev_replicas_running.is_some() {
                        let had_prior_failure = prev_task_states.values()
                            .any(|(s, _)| matches!(s.as_str(), "failed" | "rejected"));
                        if had_prior_failure && current_running > prev_replicas_running.unwrap_or(0) {
                            yield ServiceEvent {
                                service_id: service_id.clone(),
                                event_type: ServiceEventType::ServiceEventTaskRecovered as i32,
                                previous_replicas: prev_replicas_running,
                                current_replicas: Some(current_running),
                                timestamp: now,
                                message: format!("New task {} is running (recovery)", task_id),
                                affected_tasks: vec![convert_task_to_proto(t)],
                            };
                        }
                    }
                }

                new_task_states.insert(task_id, (task_state, desired));
            }

            prev_task_states = new_task_states;
            prev_replicas_running = Some(current_running);
        }
    };

    Box::pin(stream)
}
