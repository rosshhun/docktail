//! Health — service coverage, stack health, and restart event streaming.

use std::pin::Pin;
use futures_util::stream::Stream;
use tonic::Status;
use tracing::{info, warn};

use crate::docker::client::DockerClient;
use crate::swarm::map::convert_task_to_proto;
use crate::proto::{
    ServiceCoverage, ServiceCoverageResponse,
    StackHealth, StackHealthResponse, StackHealthStatus,
    ServiceHealth, ServiceHealthStatus,
    SwarmRestartPolicy,
    ServiceRestartEvent, RestartEventType,
};

/// Compute service coverage: which eligible nodes have a running task.
pub(crate) async fn get_coverage(
    docker: &DockerClient,
    service_id: &str,
) -> Result<ServiceCoverageResponse, Status> {
    let services = docker.list_services().await
        .map_err(|e| Status::internal(format!("Failed to list services: {}", e)))?;

    let svc = services.iter()
        .find(|s| s.id.as_deref() == Some(service_id))
        .ok_or_else(|| Status::not_found(format!("Service {} not found", service_id)))?;

    let spec = svc.spec.as_ref();
    let mode_spec = spec.and_then(|sp| sp.mode.as_ref());
    let is_global = mode_spec.map(|m| m.global.is_some() || m.global_job.is_some()).unwrap_or(false);

    let nodes = docker.list_nodes().await
        .map_err(|e| Status::internal(format!("Failed to list nodes: {}", e)))?;

    let eligible_node_ids: Vec<String> = nodes.iter()
        .filter(|n| {
            let avail = n.spec.as_ref()
                .and_then(|s| s.availability.as_ref())
                .map(|a| matches!(a, bollard::models::NodeSpecAvailabilityEnum::ACTIVE))
                .unwrap_or(false);
            let ready = n.status.as_ref()
                .and_then(|s| s.state.as_ref())
                .map(|s| matches!(s, bollard::models::NodeState::READY))
                .unwrap_or(false);
            avail && ready
        })
        .filter_map(|n| n.id.clone())
        .collect();

    let tasks = docker.list_tasks().await
        .map_err(|e| Status::internal(format!("Failed to list tasks: {}", e)))?;

    let covered: std::collections::HashSet<String> = tasks.iter()
        .filter(|t| t.service_id.as_deref() == Some(service_id))
        .filter(|t| {
            t.status.as_ref()
                .and_then(|s| s.state.as_ref())
                .map(|s| matches!(s, bollard::models::TaskState::RUNNING))
                .unwrap_or(false)
        })
        .filter_map(|t| t.node_id.clone())
        .collect();

    let covered_nodes: Vec<String> = eligible_node_ids.iter()
        .filter(|nid| covered.contains(*nid))
        .cloned()
        .collect();

    let uncovered_nodes: Vec<String> = eligible_node_ids.iter()
        .filter(|nid| !covered.contains(*nid))
        .cloned()
        .collect();

    let total = eligible_node_ids.len() as u32;
    let coverage_pct = if total > 0 {
        (covered_nodes.len() as f64 / total as f64) * 100.0
    } else {
        0.0
    };

    info!("Service {} coverage: {}/{} nodes ({:.1}%)", service_id, covered_nodes.len(), total, coverage_pct);

    Ok(ServiceCoverageResponse {
        coverage: Some(ServiceCoverage {
            covered_nodes,
            uncovered_nodes,
            total_nodes: total,
            coverage_percentage: coverage_pct,
            service_id: service_id.to_string(),
            is_global,
        }),
    })
}

/// Compute stack-level health by aggregating service health for all
/// services in the given namespace.
pub(crate) async fn get_stack_health(
    docker: &DockerClient,
    namespace: &str,
) -> Result<StackHealthResponse, Status> {
    let services = docker.list_services().await
        .map_err(|e| Status::internal(format!("Failed to list services: {}", e)))?;

    let tasks = docker.list_tasks().await
        .map_err(|e| Status::internal(format!("Failed to list tasks: {}", e)))?;

    let stack_services: Vec<&bollard::models::Service> = services.iter()
        .filter(|s| {
            s.spec.as_ref()
                .and_then(|sp| sp.labels.as_ref())
                .and_then(|l| l.get("com.docker.stack.namespace"))
                .map(|ns| ns == namespace)
                .unwrap_or(false)
        })
        .collect();

    if stack_services.is_empty() {
        return Err(Status::not_found(format!("Stack '{}' not found", namespace)));
    }

    let mut service_healths: Vec<ServiceHealth> = Vec::new();
    let mut total_desired: u64 = 0;
    let mut total_running: u64 = 0;
    let mut total_failed: u64 = 0;
    let mut healthy_count: u32 = 0;
    let mut degraded_count: u32 = 0;
    let mut unhealthy_count: u32 = 0;

    for svc in &stack_services {
        let svc_id = svc.id.as_deref().unwrap_or("");
        let svc_name = svc.spec.as_ref()
            .and_then(|sp| sp.name.clone())
            .unwrap_or_default();

        let spec = svc.spec.as_ref();
        let mode_spec = spec.and_then(|sp| sp.mode.as_ref());
        let task_template = spec.and_then(|sp| sp.task_template.as_ref());

        let desired = if let Some(mode) = mode_spec {
            if let Some(replicated) = &mode.replicated {
                replicated.replicas.unwrap_or(1) as u64
            } else if mode.global.is_some() {
                docker.list_nodes().await
                    .map(|nodes| nodes.iter().filter(|n| {
                        let is_active = n.spec.as_ref()
                            .and_then(|s| s.availability.as_ref())
                            .map(|a| matches!(a, bollard::models::NodeSpecAvailabilityEnum::ACTIVE))
                            .unwrap_or(false);
                        let is_ready = n.status.as_ref()
                            .and_then(|s| s.state.as_ref())
                            .map(|s| matches!(s, bollard::models::NodeState::READY))
                            .unwrap_or(false);
                        is_active && is_ready
                    }).count() as u64)
                    .unwrap_or(0)
            } else {
                0
            }
        } else {
            0
        };

        let svc_tasks: Vec<&bollard::models::Task> = tasks.iter()
            .filter(|t| t.service_id.as_deref() == Some(svc_id))
            .collect();

        let running = svc_tasks.iter()
            .filter(|t| {
                t.status.as_ref()
                    .and_then(|s| s.state.as_ref())
                    .map(|s| matches!(s, bollard::models::TaskState::RUNNING))
                    .unwrap_or(false)
            })
            .count() as u64;

        let failed = svc_tasks.iter()
            .filter(|t| {
                t.status.as_ref()
                    .and_then(|s| s.state.as_ref())
                    .map(|s| matches!(s, bollard::models::TaskState::FAILED | bollard::models::TaskState::REJECTED))
                    .unwrap_or(false)
                    &&
                t.desired_state.as_ref()
                    .map(|d| matches!(d, bollard::models::TaskState::RUNNING))
                    .unwrap_or(true)
            })
            .count() as u64;

        let recent_errors: Vec<String> = svc_tasks.iter()
            .filter(|t| {
                t.status.as_ref()
                    .and_then(|s| s.state.as_ref())
                    .map(|s| matches!(s, bollard::models::TaskState::FAILED | bollard::models::TaskState::REJECTED))
                    .unwrap_or(false)
            })
            .filter_map(|t| {
                t.status.as_ref()
                    .and_then(|s| s.err.clone().or_else(|| s.message.clone()))
            })
            .rev()
            .take(5)
            .collect();

        let update_in_progress = svc.update_status.as_ref()
            .and_then(|us| us.state.as_ref())
            .map(|s| format!("{:?}", s).to_lowercase().contains("updating"))
            .unwrap_or(false);

        let restart_policy = task_template
            .and_then(|tt| tt.restart_policy.as_ref())
            .map(|rp| SwarmRestartPolicy {
                condition: rp.condition.as_ref()
                    .map(|c| format!("{}", c))
                    .unwrap_or_else(|| "any".to_string()),
                delay_ns: rp.delay.unwrap_or(0),
                max_attempts: rp.max_attempts.unwrap_or(0) as u64,
                window_ns: rp.window.unwrap_or(0),
            });

        let health_status = if running >= desired && desired > 0 && failed == 0 {
            ServiceHealthStatus::ServiceHealthHealthy as i32
        } else if running == 0 && desired > 0 {
            ServiceHealthStatus::ServiceHealthUnhealthy as i32
        } else if running < desired || failed > 0 {
            ServiceHealthStatus::ServiceHealthDegraded as i32
        } else {
            ServiceHealthStatus::ServiceHealthUnknown as i32
        };

        match health_status {
            x if x == ServiceHealthStatus::ServiceHealthHealthy as i32 => healthy_count += 1,
            x if x == ServiceHealthStatus::ServiceHealthDegraded as i32 => degraded_count += 1,
            x if x == ServiceHealthStatus::ServiceHealthUnhealthy as i32 => unhealthy_count += 1,
            _ => {}
        }

        total_desired += desired;
        total_running += running;
        total_failed += failed;

        service_healths.push(ServiceHealth {
            service_id: svc_id.to_string(),
            service_name: svc_name,
            health_status,
            replicas_desired: desired,
            replicas_running: running,
            replicas_failed: failed,
            recent_errors,
            update_in_progress,
            restart_policy,
        });
    }

    let overall_status = if unhealthy_count > 0 {
        StackHealthStatus::StackHealthUnhealthy as i32
    } else if degraded_count > 0 {
        StackHealthStatus::StackHealthDegraded as i32
    } else if healthy_count > 0 {
        StackHealthStatus::StackHealthHealthy as i32
    } else {
        StackHealthStatus::StackHealthUnknown as i32
    };

    info!("Stack '{}' health: {} healthy, {} degraded, {} unhealthy ({}/{})",
        namespace, healthy_count, degraded_count, unhealthy_count, total_running, total_desired);

    Ok(StackHealthResponse {
        health: Some(StackHealth {
            namespace: namespace.to_string(),
            overall_status,
            service_healths,
            total_services: stack_services.len() as u32,
            healthy_services: healthy_count,
            degraded_services: degraded_count,
            unhealthy_services: unhealthy_count,
            total_desired,
            total_running,
            total_failed,
        }),
    })
}

/// Build a polling stream that emits `ServiceRestartEvent` items when
/// a task slot gets a replacement (restart / OOM / crash-loop).
pub(crate) fn restart_event_stream(
    docker: DockerClient,
    filter_service_id: Option<String>,
    poll_ms: u64,
) -> Pin<Box<dyn Stream<Item = Result<ServiceRestartEvent, Status>> + Send>> {
    let stream = async_stream::try_stream! {
        let mut slot_tasks: std::collections::HashMap<(String, u64), (String, String, i64)> = std::collections::HashMap::new();
        let mut restart_counts: std::collections::HashMap<(String, u64), Vec<i64>> = std::collections::HashMap::new();
        let mut service_names: std::collections::HashMap<String, String> = std::collections::HashMap::new();

        // Seed initial state
        if let Ok(services) = docker.list_services().await {
            for svc in &services {
                let svc_id = svc.id.clone().unwrap_or_default();
                let svc_name = svc.spec.as_ref()
                    .and_then(|sp| sp.name.clone())
                    .unwrap_or_default();
                service_names.insert(svc_id, svc_name);
            }
        }

        if let Ok(tasks) = docker.list_tasks().await {
            for t in &tasks {
                let svc_id = t.service_id.clone().unwrap_or_default();
                if let Some(ref filter) = filter_service_id {
                    if &svc_id != filter { continue; }
                }
                if let Some(slot) = t.slot {
                    let task_id = t.id.clone().unwrap_or_default();
                    let task_state = t.status.as_ref()
                        .and_then(|s| s.state.as_ref())
                        .map(|s| format!("{:?}", s).to_lowercase())
                        .unwrap_or_else(|| "unknown".to_string());
                    let updated = t.updated_at.as_ref()
                        .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
                        .map(|dt| dt.timestamp())
                        .unwrap_or(0);
                    let key = (svc_id, slot as u64);
                    if let Some(existing) = slot_tasks.get(&key) {
                        if updated > existing.2 {
                            slot_tasks.insert(key, (task_id, task_state, updated));
                        }
                    } else {
                        slot_tasks.insert(key, (task_id, task_state, updated));
                    }
                }
            }
        }

        loop {
            tokio::time::sleep(std::time::Duration::from_millis(poll_ms)).await;

            let now = chrono::Utc::now().timestamp();

            if let Ok(services) = docker.list_services().await {
                for svc in &services {
                    let svc_id = svc.id.clone().unwrap_or_default();
                    let svc_name = svc.spec.as_ref()
                        .and_then(|sp| sp.name.clone())
                        .unwrap_or_default();
                    service_names.insert(svc_id, svc_name);
                }
            }

            let tasks = match docker.list_tasks().await {
                Ok(t) => t,
                Err(e) => {
                    warn!("Failed to list tasks in restart event stream: {}", e);
                    continue;
                }
            };

            let mut current_slot_tasks: std::collections::HashMap<(String, u64), &bollard::models::Task> = std::collections::HashMap::new();
            for t in &tasks {
                let svc_id = t.service_id.clone().unwrap_or_default();
                if let Some(ref filter) = filter_service_id {
                    if &svc_id != filter { continue; }
                }
                if let Some(slot) = t.slot {
                    let key = (svc_id, slot as u64);
                    let updated = t.updated_at.as_ref()
                        .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
                        .map(|dt| dt.timestamp())
                        .unwrap_or(0);
                    if let Some(existing) = current_slot_tasks.get(&key) {
                        let existing_ts = existing.updated_at.as_ref()
                            .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
                            .map(|dt| dt.timestamp())
                            .unwrap_or(0);
                        if updated > existing_ts {
                            current_slot_tasks.insert(key, t);
                        }
                    } else {
                        current_slot_tasks.insert(key, t);
                    }
                }
            }

            for (key, task) in &current_slot_tasks {
                let task_id = task.id.clone().unwrap_or_default();
                let task_state = task.status.as_ref()
                    .and_then(|s| s.state.as_ref())
                    .map(|s| format!("{:?}", s).to_lowercase())
                    .unwrap_or_else(|| "unknown".to_string());
                let updated = task.updated_at.as_ref()
                    .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
                    .map(|dt| dt.timestamp())
                    .unwrap_or(0);

                let svc_name = service_names.get(&key.0).cloned().unwrap_or_default();

                if let Some((prev_task_id, _prev_state, _prev_ts)) = slot_tasks.get(key) {
                    if &task_id != prev_task_id {
                        let restarts = restart_counts.entry(key.clone()).or_default();
                        restarts.push(now);
                        restarts.retain(|&ts| now - ts < 300);
                        let count = restarts.len() as u32;

                        let old_task = tasks.iter()
                            .find(|t| t.id.as_deref() == Some(prev_task_id));
                        let old_task_proto = old_task.map(|t| convert_task_to_proto(t));

                        let is_oom = old_task
                            .and_then(|t| t.status.as_ref())
                            .and_then(|s| s.err.as_ref())
                            .map(|e| e.to_lowercase().contains("oom") || e.to_lowercase().contains("out of memory"))
                            .unwrap_or(false);

                        let event_type = if is_oom {
                            RestartEventType::RestartEventOomKilled as i32
                        } else if count >= 3 {
                            RestartEventType::RestartEventCrashLoop as i32
                        } else {
                            RestartEventType::RestartEventTaskRestarted as i32
                        };

                        let message = if is_oom {
                            format!("{} slot {} OOM killed — restarting (#{} in 5min)", svc_name, key.1, count)
                        } else if count >= 3 {
                            format!("{} slot {} crash looping — {} restarts in 5min", svc_name, key.1, count)
                        } else {
                            format!("{} slot {} restarted (task {} → {})", svc_name, key.1, prev_task_id, task_id)
                        };

                        yield ServiceRestartEvent {
                            service_id: key.0.clone(),
                            service_name: svc_name,
                            event_type,
                            new_task: Some(convert_task_to_proto(task)),
                            old_task: old_task_proto,
                            slot: Some(key.1),
                            restart_count: count,
                            timestamp: now,
                            message,
                        };
                    }
                }

                slot_tasks.insert(key.clone(), (task_id, task_state, updated));
            }

            slot_tasks.retain(|k, _| current_slot_tasks.contains_key(k));
            restart_counts.retain(|k, _| current_slot_tasks.contains_key(k));
        }
    };

    Box::pin(stream)
}
