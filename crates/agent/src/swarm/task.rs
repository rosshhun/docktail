//! Task — swarm task listing, inspection, and log streaming.

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio_stream::StreamExt;
use futures_util::stream::Stream;
use tonic::Status;

use crate::docker::client::DockerClient;
use crate::proto::{
    TaskInfo, TaskInspectInfo, TaskNetworkAttachment,
    NormalizedLogEntry, ParseMetadata, LogLevel, SwarmContext,
    SwarmRestartPolicy,
};

/// Build a `Vec<TaskInfo>` from raw bollard tasks, optionally filtering
/// by service id and resolving service names.
pub(crate) async fn list(
    docker: &DockerClient,
    filter_service_id: Option<String>,
) -> Result<Vec<TaskInfo>, Status> {
    let all_tasks = docker.list_tasks().await
        .map_err(|e| Status::internal(format!("Failed to list tasks: {}", e)))?;

    let services = docker.list_services().await.unwrap_or_default();
    let service_name_map: std::collections::HashMap<String, String> = services.iter()
        .filter_map(|s| {
            let id = s.id.as_ref()?.clone();
            let name = s.spec.as_ref()?.name.as_ref()?.clone();
            Some((id, name))
        })
        .collect();

    let task_infos: Vec<TaskInfo> = all_tasks.into_iter()
        .filter(|t| {
            if let Some(ref filter) = filter_service_id {
                t.service_id.as_deref() == Some(filter.as_str())
            } else {
                true
            }
        })
        .map(|t| {
            let status = t.status.as_ref();
            let container_status = status.and_then(|s| s.container_status.as_ref());

            TaskInfo {
                id: t.id.unwrap_or_default(),
                service_id: t.service_id.clone().unwrap_or_default(),
                node_id: t.node_id.unwrap_or_default(),
                slot: t.slot.map(|s| s as u64),
                container_id: container_status.and_then(|cs| cs.container_id.clone()),
                state: status
                    .and_then(|s| s.state.as_ref())
                    .map(|s| format!("{:?}", s).to_lowercase())
                    .unwrap_or_else(|| "unknown".to_string()),
                desired_state: t.desired_state
                    .as_ref()
                    .map(|s| format!("{:?}", s).to_lowercase())
                    .unwrap_or_else(|| "unknown".to_string()),
                status_message: status
                    .and_then(|s| s.message.clone())
                    .unwrap_or_default(),
                status_err: status
                    .and_then(|s| s.err.clone()),
                created_at: t.created_at.as_ref()
                    .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
                    .map(|dt| dt.timestamp())
                    .unwrap_or(0),
                updated_at: t.updated_at.as_ref()
                    .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
                    .map(|dt| dt.timestamp())
                    .unwrap_or(0),
                exit_code: container_status.and_then(|cs| cs.exit_code.map(|c| c as i32)),
                service_name: t.service_id.as_deref()
                    .and_then(|sid| service_name_map.get(sid))
                    .cloned()
                    .unwrap_or_default(),
            }
        })
        .collect();

    Ok(task_infos)
}

/// Build a streaming response for service-level logs (S3).
pub(crate) async fn stream_service_logs(
    docker: &DockerClient,
    service_id: &str,
    follow: bool,
    tail_lines: Option<u64>,
    since: i64,
    until: i64,
    timestamps: bool,
) -> Result<Pin<Box<dyn Stream<Item = Result<NormalizedLogEntry, Status>> + Send>>, Status> {
    let service = docker.inspect_service(service_id).await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("404") || msg.to_lowercase().contains("not found") {
                Status::not_found(format!("Service not found: {}", e))
            } else if msg.contains("403") || msg.to_lowercase().contains("permission") {
                Status::permission_denied(format!("Permission denied: {}", e))
            } else {
                Status::internal(format!("Failed to inspect service: {}", e))
            }
        })?;
    let service_name = service.spec.as_ref()
        .and_then(|s| s.name.clone())
        .unwrap_or_else(|| service_id.to_string());
    let resolved_service_id = service.id.clone().unwrap_or_else(|| service_id.to_string());

    let since_i32 = since.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    let until_i32 = until.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    let tail = tail_lines.map(|n| n.to_string());

    let log_stream = docker.stream_service_logs(
        service_id, follow, tail, since_i32, until_i32, timestamps,
    );

    let tasks = docker.list_tasks().await.unwrap_or_default();
    let service_tasks: Vec<_> = tasks.into_iter()
        .filter(|t| t.service_id.as_deref() == Some(&*resolved_service_id) ||
                    t.service_id.as_deref() == Some(service_id))
        .collect();

    let service_name_clone = service_name.clone();
    let service_id_clone = resolved_service_id.clone();
    let sequence = Arc::new(AtomicU64::new(0));
    let timestamps_enabled = timestamps;

    let output_stream = log_stream.map(move |result| {
        match result {
            Ok(output) => {
                let (stream_type, raw_bytes) = match output {
                    bollard::container::LogOutput::StdOut { message } => (LogLevel::Stdout, message),
                    bollard::container::LogOutput::StdErr { message } => (LogLevel::Stderr, message),
                    bollard::container::LogOutput::StdIn { message } => (LogLevel::Stdout, message),
                    bollard::container::LogOutput::Console { message } => (LogLevel::Stdout, message),
                };

                let raw_str = std::str::from_utf8(&raw_bytes).unwrap_or("");
                let (task_prefix, after_prefix) = if let Some(pipe_idx) = raw_str.find(" | ") {
                    (Some(&raw_str[..pipe_idx]), &raw_str[pipe_idx + 3..])
                } else {
                    (None, raw_str)
                };

                let (timestamp, message_str) = if timestamps_enabled {
                    let first_space = after_prefix.find(' ');
                    match first_space {
                        Some(idx) => {
                            match chrono::DateTime::parse_from_rfc3339(&after_prefix[..idx]) {
                                Ok(dt) => (dt.timestamp_nanos_opt().unwrap_or(0), &after_prefix[idx + 1..]),
                                Err(_) => (chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0), after_prefix),
                            }
                        }
                        None => (chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0), after_prefix),
                    }
                } else {
                    (chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0), after_prefix)
                };
                let content = bytes::Bytes::from(message_str.as_bytes().to_vec());

                let swarm_ctx = {
                    let mut ctx_task_id = String::new();
                    let mut ctx_slot: u64 = 0;
                    let mut ctx_node_id = String::new();

                    if let Some(prefix) = task_prefix {
                        let trimmed = prefix.trim();
                        if let Some(at_idx) = trimmed.rfind('@') {
                            ctx_node_id = trimmed[at_idx + 1..].to_string();
                            let task_part = &trimmed[..at_idx];
                            let segments: Vec<&str> = task_part.rsplitn(3, '.').collect();
                            if segments.len() >= 3 {
                                ctx_task_id = segments[0].to_string();
                                ctx_slot = segments[1].parse::<u64>().unwrap_or(0);
                            } else if segments.len() == 2 {
                                ctx_task_id = segments[0].to_string();
                                ctx_slot = segments[1].parse::<u64>().unwrap_or(0);
                            }
                        }
                    }

                    if ctx_task_id.is_empty() && !service_tasks.is_empty() {
                        if ctx_slot > 0 {
                            if let Some(t) = service_tasks.iter().find(|t| t.slot == Some(ctx_slot as i64)) {
                                ctx_task_id = t.id.clone().unwrap_or_default();
                                if ctx_node_id.is_empty() {
                                    ctx_node_id = t.node_id.clone().unwrap_or_default();
                                }
                            }
                        }
                        if ctx_task_id.is_empty() {
                            let first_task = &service_tasks[0];
                            ctx_task_id = first_task.id.clone().unwrap_or_default();
                            if ctx_slot == 0 {
                                ctx_slot = first_task.slot.unwrap_or(0) as u64;
                            }
                            if ctx_node_id.is_empty() {
                                ctx_node_id = first_task.node_id.clone().unwrap_or_default();
                            }
                        }
                    }

                    Some(SwarmContext {
                        service_id: service_id_clone.clone(),
                        service_name: service_name_clone.clone(),
                        task_id: ctx_task_id,
                        task_slot: ctx_slot,
                        node_id: ctx_node_id,
                    })
                };

                let seq = sequence.fetch_add(1, Ordering::Relaxed);

                Ok(NormalizedLogEntry {
                    container_id: String::new(),
                    timestamp_nanos: timestamp,
                    log_level: stream_type as i32,
                    sequence: seq,
                    raw_content: content.to_vec(),
                    parsed: None,
                    metadata: Some(ParseMetadata {
                        detected_format: 0,
                        parse_success: true,
                        parse_error: None,
                        parse_time_nanos: 0,
                    }),
                    grouped_lines: Vec::new(),
                    line_count: 1,
                    is_grouped: false,
                    swarm_context: swarm_ctx,
                })
            }
            Err(e) => Err(Status::internal(format!("Service log stream error: {}", e))),
        }
    });

    Ok(Box::pin(output_stream))
}

/// Build a streaming response for task-level logs (B02).
pub(crate) async fn stream_task_logs(
    docker: DockerClient,
    task_id: &str,
    follow: bool,
    tail_lines: Option<u64>,
    since: i64,
    until: i64,
    timestamps: bool,
) -> Pin<Box<dyn Stream<Item = Result<NormalizedLogEntry, Status>> + Send>> {
    let since_i32 = since.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    let until_i32 = until.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    let tail = tail_lines.map(|n| n.to_string());

    let raw_stream = docker.stream_task_logs(
        task_id, follow, tail, since_i32, until_i32, timestamps,
    );

    let (real_container_id, service_id, node_id, task_slot, service_name) =
        match docker.inspect_task(task_id).await {
            Ok(Some(task)) => {
                let cid = task.status.as_ref()
                    .and_then(|s| s.container_status.as_ref())
                    .and_then(|cs| cs.container_id.clone())
                    .unwrap_or_default();
                let sid = task.service_id.clone().unwrap_or_default();
                let nid = task.node_id.clone().unwrap_or_default();
                let slot = task.slot.unwrap_or(0) as u64;
                let sname = task.spec.as_ref()
                    .and_then(|s| s.container_spec.as_ref())
                    .and_then(|cs| cs.labels.as_ref())
                    .and_then(|l| l.get("com.docker.swarm.service.name").cloned())
                    .unwrap_or_else(|| sid.clone());
                (cid, sid, nid, slot, sname)
            }
            _ => (String::new(), String::new(), String::new(), 0, String::new()),
        };

    let task_id_clone = task_id.to_string();
    let timestamps_enabled = timestamps;
    let sequence_counter = Arc::new(AtomicU64::new(0));
    let output_stream = raw_stream.map(move |result| {
        match result {
            Ok(log_output) => {
                let (stream_type, raw_bytes) = match log_output {
                    bollard::container::LogOutput::StdOut { message } => (LogLevel::Stdout as i32, message),
                    bollard::container::LogOutput::StdErr { message } => (LogLevel::Stderr as i32, message),
                    bollard::container::LogOutput::StdIn { message } => (LogLevel::Stdout as i32, message),
                    bollard::container::LogOutput::Console { message } => (LogLevel::Stdout as i32, message),
                };

                let (ts, content) = if timestamps_enabled {
                    let split_idx = raw_bytes.iter().position(|&b| b == b' ');
                    match split_idx {
                        Some(idx) => {
                            match std::str::from_utf8(&raw_bytes[..idx]) {
                                Ok(ts_str) => match chrono::DateTime::parse_from_rfc3339(ts_str) {
                                    Ok(dt) => (
                                        dt.timestamp_nanos_opt().unwrap_or_else(|| chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)),
                                        raw_bytes.slice((idx + 1)..),
                                    ),
                                    Err(_) => (chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0), raw_bytes),
                                },
                                Err(_) => (chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0), raw_bytes),
                            }
                        }
                        None => (chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0), raw_bytes),
                    }
                } else {
                    (chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0), raw_bytes)
                };

                let seq = sequence_counter.fetch_add(1, Ordering::Relaxed);

                Ok(NormalizedLogEntry {
                    container_id: real_container_id.clone(),
                    timestamp_nanos: ts,
                    log_level: stream_type,
                    sequence: seq,
                    raw_content: content.to_vec(),
                    parsed: None,
                    metadata: Some(ParseMetadata {
                        detected_format: 0,
                        parse_success: false,
                        parse_error: None,
                        parse_time_nanos: 0,
                    }),
                    grouped_lines: Vec::new(),
                    line_count: 1,
                    is_grouped: false,
                    swarm_context: Some(SwarmContext {
                        service_id: service_id.clone(),
                        service_name: service_name.clone(),
                        task_id: task_id_clone.clone(),
                        task_slot,
                        node_id: node_id.clone(),
                    }),
                })
            }
            Err(e) => Err(Status::internal(format!("Task log stream error: {}", e))),
        }
    });

    Box::pin(output_stream)
}

/// Build a `TaskInspectInfo` from a raw bollard task (B03).
pub(crate) async fn inspect(
    docker: &DockerClient,
    task_id: &str,
) -> Result<Option<TaskInspectInfo>, Status> {
    match docker.inspect_task(task_id).await {
        Ok(Some(task)) => {
            let spec = task.spec.as_ref();
            let container_spec = spec.and_then(|s| s.container_spec.as_ref());
            let status = task.status.as_ref();

            let service_id = task.service_id.clone().unwrap_or_default();
            let service_name = container_spec
                .and_then(|cs| cs.labels.as_ref())
                .and_then(|l| l.get("com.docker.swarm.service.name").cloned())
                .unwrap_or_default();

            let env: std::collections::HashMap<String, String> = container_spec
                .and_then(|cs| cs.env.as_ref())
                .map(|vars| {
                    vars.iter().filter_map(|v| {
                        let parts: Vec<&str> = v.splitn(2, '=').collect();
                        if parts.len() == 2 { Some((parts[0].to_string(), parts[1].to_string())) } else { None }
                    }).collect()
                })
                .unwrap_or_default();

            let labels: std::collections::HashMap<String, String> = container_spec
                .and_then(|cs| cs.labels.as_ref())
                .cloned()
                .unwrap_or_default();

            let network_attachments: Vec<TaskNetworkAttachment> = spec
                .and_then(|s| s.networks.as_ref())
                .map(|nets: &Vec<bollard::models::NetworkAttachmentConfig>| nets.iter().map(|na| {
                    TaskNetworkAttachment {
                        network_id: na.target.clone().unwrap_or_default(),
                        network_name: na.target.clone().unwrap_or_default(),
                        addresses: na.aliases.clone().unwrap_or_default(),
                    }
                }).collect())
                .unwrap_or_default();

            let resource_limits = spec
                .and_then(|s| s.resources.as_ref())
                .and_then(|r| r.limits.as_ref())
                .map(|l| crate::proto::ServiceResourceLimits {
                    nano_cpus: l.nano_cpus.unwrap_or(0),
                    memory_bytes: l.memory_bytes.unwrap_or(0),
                });

            let resource_reservations = spec
                .and_then(|s| s.resources.as_ref())
                .and_then(|r| r.reservations.as_ref())
                .map(|r| crate::proto::ServiceResourceReservations {
                    nano_cpus: r.nano_cpus.unwrap_or(0),
                    memory_bytes: r.memory_bytes.unwrap_or(0),
                });

            let restart_policy = spec
                .and_then(|s| s.restart_policy.as_ref())
                .map(|rp| SwarmRestartPolicy {
                    condition: rp.condition.as_ref()
                        .map(|c| format!("{:?}", c).to_lowercase())
                        .unwrap_or_else(|| "any".to_string()),
                    delay_ns: rp.delay.unwrap_or(0),
                    max_attempts: rp.max_attempts.unwrap_or(0) as u64,
                    window_ns: rp.window.unwrap_or(0),
                });

            let created_at = task.created_at.as_ref()
                .and_then(|ts| chrono::DateTime::parse_from_rfc3339(ts).ok())
                .map(|dt| dt.timestamp())
                .unwrap_or(0);

            let updated_at = task.updated_at.as_ref()
                .and_then(|ts| chrono::DateTime::parse_from_rfc3339(ts).ok())
                .map(|dt| dt.timestamp())
                .unwrap_or(0);

            let started_at = status
                .and_then(|s| s.timestamp.as_ref())
                .cloned()
                .unwrap_or_default();

            let state_str = status
                .and_then(|s| s.state.as_ref())
                .map(|s| format!("{:?}", s).to_lowercase())
                .unwrap_or_else(|| "unknown".to_string());

            let desired_state = task.desired_state.as_ref()
                .map(|s| format!("{:?}", s).to_lowercase())
                .unwrap_or_else(|| "unknown".to_string());

            let info = TaskInspectInfo {
                id: task.id.unwrap_or_default(),
                service_id,
                service_name,
                node_id: task.node_id.unwrap_or_default(),
                slot: task.slot.map(|s| s as u64),
                container_id: status.and_then(|s| s.container_status.as_ref())
                    .and_then(|cs| cs.container_id.clone()),
                state: state_str,
                desired_state,
                status_message: status.and_then(|s| s.message.clone()).unwrap_or_default(),
                status_err: status.and_then(|s| s.err.clone()),
                created_at,
                updated_at,
                exit_code: status.and_then(|s| s.container_status.as_ref())
                    .and_then(|cs| cs.exit_code.map(|c| c as i32)),
                image: container_spec.and_then(|cs| cs.image.clone()).unwrap_or_default(),
                command: container_spec.and_then(|cs| cs.command.clone()).unwrap_or_default(),
                args: container_spec.and_then(|cs| cs.args.clone()).unwrap_or_default(),
                env,
                labels,
                network_attachments,
                resource_limits,
                resource_reservations,
                restart_policy,
                started_at,
                finished_at: String::new(),
                port_status: Vec::new(),
            };

            Ok(Some(info))
        }
        Ok(None) => Ok(None),
        Err(e) => Err(Status::internal(format!("Failed to inspect task: {}", e))),
    }
}
