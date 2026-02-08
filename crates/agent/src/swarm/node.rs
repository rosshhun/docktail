//! Node — swarm node listing, inspection, update, event streaming, and removal.

use tracing::{info, warn};
use tonic::Status;

use crate::docker::client::DockerClient;
use crate::swarm::map::{convert_node_to_proto, convert_task_to_proto};
use crate::proto::{
    NodeListResponse, NodeInspectResponse, NodeInfo,
    NodeUpdateResponse, NodeEvent, NodeEventType,
    RemoveNodeResponse,
};

/// List all swarm nodes.
pub(crate) async fn list(docker: &DockerClient) -> Result<NodeListResponse, Status> {
    let nodes = docker.list_nodes().await
        .map_err(|e| Status::internal(format!("Failed to list nodes: {}", e)))?;
    let node_infos: Vec<NodeInfo> = nodes.iter().map(|n| convert_node_to_proto(n)).collect();
    Ok(NodeListResponse { nodes: node_infos })
}

/// Inspect a single swarm node.
pub(crate) async fn inspect(docker: &DockerClient, node_id: &str) -> Result<NodeInspectResponse, Status> {
    let node = docker.inspect_node(node_id).await
        .map_err(|e| Status::internal(format!("Failed to inspect node: {}", e)))?;
    let node_info = node.map(|n| convert_node_to_proto(&n));
    Ok(NodeInspectResponse { node: node_info })
}

/// Remove a node from the swarm.
pub(crate) async fn remove(docker: &DockerClient, node_id: &str, force: bool) -> Result<RemoveNodeResponse, Status> {
    match docker.remove_node(node_id, force).await {
        Ok(()) => Ok(RemoveNodeResponse {
            success: true,
            message: format!("Node {} removed from swarm", node_id),
        }),
        Err(e) => Err(Status::internal(format!("Failed to remove node: {}", e))),
    }
}

/// Build and apply a node update (availability / role / labels).
///
/// Inspects the current node to obtain its version and spec, merges the
/// requested changes, and calls `docker.update_node()`.
pub(crate) async fn update(
    docker: &DockerClient,
    node_id: &str,
    availability: Option<String>,
    role: Option<String>,
    labels: std::collections::HashMap<String, String>,
) -> Result<NodeUpdateResponse, tonic::Status> {
    let current = docker.inspect_node(node_id).await
        .map_err(|e| tonic::Status::internal(format!("Failed to inspect node for update: {}", e)))?
        .ok_or_else(|| tonic::Status::not_found(format!("Node {} not found", node_id)))?;

    let version = current.version
        .and_then(|v| v.index)
        .map(|i| i as i64)
        .ok_or_else(|| tonic::Status::internal("Node has no version".to_string()))?;

    let current_spec = current.spec.unwrap_or_default();

    let avail = if let Some(ref avail_str) = availability {
        Some(match avail_str.to_lowercase().as_str() {
            "active" => bollard::models::NodeSpecAvailabilityEnum::ACTIVE,
            "pause" => bollard::models::NodeSpecAvailabilityEnum::PAUSE,
            "drain" => bollard::models::NodeSpecAvailabilityEnum::DRAIN,
            other => return Err(tonic::Status::invalid_argument(format!(
                "Invalid availability: '{}'. Must be 'active', 'pause', or 'drain'", other
            ))),
        })
    } else {
        current_spec.availability
    };

    let role_enum = if let Some(ref role_str) = role {
        Some(match role_str.to_lowercase().as_str() {
            "worker" => bollard::models::NodeSpecRoleEnum::WORKER,
            "manager" => bollard::models::NodeSpecRoleEnum::MANAGER,
            other => return Err(tonic::Status::invalid_argument(format!(
                "Invalid role: '{}'. Must be 'worker' or 'manager'", other
            ))),
        })
    } else {
        current_spec.role
    };

    let final_labels = if !labels.is_empty() {
        Some(labels)
    } else {
        current_spec.labels
    };

    let new_spec = bollard::models::NodeSpec {
        name: current_spec.name,
        labels: final_labels,
        role: role_enum,
        availability: avail,
    };

    docker.update_node(node_id, new_spec, version).await
        .map_err(|e| tonic::Status::internal(format!("Failed to update node: {}", e)))?;

    info!("Successfully updated node {}", node_id);
    Ok(NodeUpdateResponse {
        success: true,
        message: format!("Node {} updated successfully", node_id),
    })
}

/// Build a polling stream that emits `NodeEvent` items whenever node state,
/// availability or role changes (including drain start / completion).
pub(crate) fn event_stream(
    docker: DockerClient,
    filter_node_id: Option<String>,
    poll_ms: u64,
) -> std::pin::Pin<Box<dyn futures_util::Stream<Item = Result<NodeEvent, tonic::Status>> + Send>> {
    let stream = async_stream::try_stream! {
        let mut prev_states: std::collections::HashMap<String, (String, String, String)> = std::collections::HashMap::new();
        let mut draining_nodes: std::collections::HashMap<String, bool> = std::collections::HashMap::new();

        // Seed initial state
        let initial_nodes = docker.list_nodes().await.unwrap_or_default();
        for n in &initial_nodes {
            let node_id = n.id.clone().unwrap_or_default();
            if let Some(ref filter) = filter_node_id {
                if &node_id != filter { continue; }
            }
            let spec = n.spec.as_ref();
            let availability = spec.and_then(|s| s.availability.as_ref())
                .map(|a| format!("{}", a)).unwrap_or_else(|| "unknown".to_string());
            let node_state = n.status.as_ref()
                .and_then(|s| s.state.as_ref())
                .map(|s| format!("{:?}", s).to_lowercase())
                .unwrap_or_else(|| "unknown".to_string());
            let role = spec.and_then(|s| s.role.as_ref())
                .map(|r| format!("{}", r)).unwrap_or_else(|| "unknown".to_string());

            if availability == "drain" {
                draining_nodes.insert(node_id.clone(), false);
            }
            prev_states.insert(node_id, (node_state, availability, role));
        }

        loop {
            tokio::time::sleep(std::time::Duration::from_millis(poll_ms)).await;

            let nodes = match docker.list_nodes().await {
                Ok(n) => n,
                Err(e) => {
                    warn!("Failed to list nodes in event stream: {}", e);
                    continue;
                }
            };

            let now = chrono::Utc::now().timestamp();

            for n in &nodes {
                let node_id = n.id.clone().unwrap_or_default();
                if let Some(ref filter) = filter_node_id {
                    if &node_id != filter { continue; }
                }

                let spec = n.spec.as_ref();
                let hostname = n.description.as_ref()
                    .and_then(|d| d.hostname.clone())
                    .unwrap_or_default();
                let availability = spec.and_then(|s| s.availability.as_ref())
                    .map(|a| format!("{}", a)).unwrap_or_else(|| "unknown".to_string());
                let node_state = n.status.as_ref()
                    .and_then(|s| s.state.as_ref())
                    .map(|s| format!("{:?}", s).to_lowercase())
                    .unwrap_or_else(|| "unknown".to_string());
                let role = spec.and_then(|s| s.role.as_ref())
                    .map(|r| format!("{}", r)).unwrap_or_else(|| "unknown".to_string());

                let prev = prev_states.get(&node_id);

                // Detect state change
                if let Some((prev_state, _, _)) = prev {
                    if prev_state != &node_state {
                        let event_type = match node_state.as_str() {
                            "down" => NodeEventType::NodeEventNodeDown as i32,
                            "ready" => NodeEventType::NodeEventNodeReady as i32,
                            _ => NodeEventType::NodeEventStateChange as i32,
                        };
                        yield NodeEvent {
                            node_id: node_id.clone(),
                            hostname: hostname.clone(),
                            event_type,
                            previous_value: prev_state.clone(),
                            current_value: node_state.clone(),
                            affected_tasks: Vec::new(),
                            timestamp: now,
                        };
                    }
                }

                // Detect availability change
                if let Some((_, prev_avail, _)) = prev {
                    if prev_avail != &availability {
                        let event_type = if availability == "drain" {
                            draining_nodes.insert(node_id.clone(), false);
                            NodeEventType::NodeEventDrainStarted as i32
                        } else if prev_avail == "drain" {
                            draining_nodes.remove(&node_id);
                            NodeEventType::NodeEventAvailabilityChange as i32
                        } else {
                            NodeEventType::NodeEventAvailabilityChange as i32
                        };

                        let affected_tasks = if availability == "drain" {
                            match docker.list_tasks().await {
                                Ok(tasks) => tasks.into_iter()
                                    .filter(|t| t.node_id.as_deref() == Some(&node_id))
                                    .filter(|t| {
                                        t.status.as_ref()
                                            .and_then(|s| s.state.as_ref())
                                            .map(|s| !matches!(format!("{:?}", s).to_lowercase().as_str(), "shutdown" | "complete" | "failed" | "rejected" | "remove" | "orphaned"))
                                            .unwrap_or(false)
                                    })
                                    .map(|t| convert_task_to_proto(&t))
                                    .collect(),
                                Err(e) => {
                                    tracing::warn!(node_id = %node_id, error = %e, "Failed to list tasks for drain event — affected_tasks will be empty");
                                    Vec::new()
                                }
                            }
                        } else {
                            Vec::new()
                        };

                        yield NodeEvent {
                            node_id: node_id.clone(),
                            hostname: hostname.clone(),
                            event_type,
                            previous_value: prev_avail.clone(),
                            current_value: availability.clone(),
                            affected_tasks,
                            timestamp: now,
                        };
                    }
                }

                // Detect role change
                if let Some((_, _, prev_role)) = prev {
                    if prev_role != &role {
                        yield NodeEvent {
                            node_id: node_id.clone(),
                            hostname: hostname.clone(),
                            event_type: NodeEventType::NodeEventRoleChange as i32,
                            previous_value: prev_role.clone(),
                            current_value: role.clone(),
                            affected_tasks: Vec::new(),
                            timestamp: now,
                        };
                    }
                }

                // Check drain completion
                if availability == "drain" {
                    let was_draining = draining_nodes.get(&node_id).copied().unwrap_or(false);
                    if !was_draining {
                        let tasks = match docker.list_tasks().await {
                            Ok(t) => t,
                            Err(e) => {
                                tracing::warn!(node_id = %node_id, error = %e, "Failed to list tasks for drain completion check — skipping");
                                continue;
                            }
                        };
                        let running_on_node = tasks.iter()
                            .filter(|t| t.node_id.as_deref() == Some(&node_id))
                            .any(|t| {
                                t.status.as_ref()
                                    .and_then(|s| s.state.as_ref())
                                    .map(|s| matches!(format!("{:?}", s).to_lowercase().as_str(), "running" | "starting" | "preparing" | "assigned" | "accepted" | "ready"))
                                    .unwrap_or(false)
                            });

                        if !running_on_node {
                            draining_nodes.insert(node_id.clone(), true);
                            yield NodeEvent {
                                node_id: node_id.clone(),
                                hostname: hostname.clone(),
                                event_type: NodeEventType::NodeEventDrainCompleted as i32,
                                previous_value: "draining".to_string(),
                                current_value: "drained".to_string(),
                                affected_tasks: Vec::new(),
                                timestamp: now,
                            };
                        }
                    }
                }

                // New node appeared
                if prev.is_none() {
                    yield NodeEvent {
                        node_id: node_id.clone(),
                        hostname: hostname.clone(),
                        event_type: NodeEventType::NodeEventNodeReady as i32,
                        previous_value: String::new(),
                        current_value: node_state.clone(),
                        affected_tasks: Vec::new(),
                        timestamp: now,
                    };
                }

                prev_states.insert(node_id.clone(), (node_state, availability, role));
            }

            // Prune state for removed nodes
            let current_node_ids: std::collections::HashSet<String> = nodes.iter()
                .filter_map(|n| n.id.clone())
                .collect();
            prev_states.retain(|id, _| current_node_ids.contains(id));
            draining_nodes.retain(|id, _| current_node_ids.contains(id));
        }
    };

    Box::pin(stream)
}
