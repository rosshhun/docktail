use tonic::{Request, Response, Status};
use tracing::{warn, debug, info};
use tokio_stream::StreamExt;
use std::pin::Pin;
use futures_util::stream::Stream;

use crate::state::SharedState;

use super::proto::{
    swarm_service_server::SwarmService,
    SwarmInfoRequest, SwarmInfoResponse, SwarmInfo,
    NodeListRequest, NodeListResponse, NodeInfo,
    NodeRole, NodeAvailability, NodeState, ManagerStatus,
    ServiceListRequest, ServiceListResponse, ServiceInfo,
    ServiceInspectRequest, ServiceInspectResponse,
    ServiceMode, ServicePort, UpdateStatus, VirtualIp,
    TaskListRequest, TaskListResponse, TaskInfo,
    // S3: Service log streaming
    ServiceLogStreamRequest, NormalizedLogEntry, SwarmContext,
    ParseMetadata, LogLevel,
    // M6: Service management
    CreateServiceRequest, CreateServiceResponse,
    DeleteServiceRequest, DeleteServiceResponse,
    UpdateServiceRequest, UpdateServiceResponse,
    // S5: Swarm networking
    SwarmNetworkListRequest, SwarmNetworkListResponse,
    SwarmNetworkInspectRequest, SwarmNetworkInspectResponse,
    SwarmNetworkInfo, IpamConfigEntry, NetworkPeerInfo, NetworkServiceAttachment,
    // S6: Orchestration observability
    UpdateConfig, ServicePlacement, PlacementPreference, Platform,
    ServiceUpdateStreamRequest, ServiceUpdateEvent, TaskStateChange,
    // S8: Secrets & configs
    SecretListRequest, SecretListResponse, SwarmSecretInfo,
    ConfigListRequest, ConfigListResponse, SwarmConfigInfo,
    SecretReferenceInfo, ConfigReferenceInfo,
    // S9: Node management & drain awareness
    NodeInspectRequest, NodeInspectResponse,
    NodeUpdateRequest, NodeUpdateResponse,
    NodeEventStreamRequest, NodeEvent, NodeEventType,
    // S10: Service scaling insights & coverage
    ServiceEventStreamRequest, ServiceEvent, ServiceEventType,
    ServiceCoverageRequest, ServiceCoverageResponse, ServiceCoverage,
    // S11: Stack health & restart policies
    SwarmRestartPolicy,
    StackHealthRequest, StackHealthResponse, StackHealth, StackHealthStatus,
    ServiceHealth, ServiceHealthStatus,
    ServiceRestartEventStreamRequest, ServiceRestartEvent, RestartEventType,
    // B06: Service rollback
    RollbackServiceRequest, RollbackServiceResponse,
    // B08/B09: Secret/Config CRUD
    CreateSecretRequest, CreateSecretResponse,
    DeleteSecretRequest, DeleteSecretResponse,
    CreateConfigRequest, CreateConfigResponse,
    DeleteConfigRequest, DeleteConfigResponse,
    // B04/B05: Swarm init/join/leave
    SwarmInitRequest, SwarmInitResponse,
    SwarmJoinRequest, SwarmJoinResponse,
    SwarmLeaveRequest, SwarmLeaveResponse,
    // B07: Node remove
    RemoveNodeRequest, RemoveNodeResponse,
    // B14: Network connect/disconnect
    NetworkConnectRequest, NetworkConnectResponse,
    NetworkDisconnectRequest, NetworkDisconnectResponse,
    // B02: Task log streaming
    TaskLogStreamRequest,
    // B03: Task inspect
    TaskInspectRequest, TaskInspectResponse, TaskInspectInfo, TaskNetworkAttachment,
    // B05: Swarm update/unlock
    SwarmUpdateRequest, SwarmUpdateResponse,
    SwarmUnlockKeyRequest, SwarmUnlockKeyResponse,
    SwarmUnlockRequest, SwarmUnlockResponse,
    // B11: Compose stack deploy
    DeployComposeStackRequest, DeployComposeStackResponse,
    // B12: Stack file viewer
    GetStackFileRequest, GetStackFileResponse,
};

/// Implementation of the SwarmService gRPC service.
/// Provides Docker Swarm detection, node listing, service/task discovery.
pub struct SwarmServiceImpl {
    state: SharedState,
}

impl SwarmServiceImpl {
    pub fn new(state: SharedState) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl SwarmService for SwarmServiceImpl {
    /// Streaming type for StreamServiceLogs
    type StreamServiceLogsStream = Pin<Box<dyn Stream<Item = Result<NormalizedLogEntry, Status>> + Send>>;
    /// Streaming type for ServiceUpdateStream (S6)
    type ServiceUpdateStreamStream = Pin<Box<dyn Stream<Item = Result<ServiceUpdateEvent, Status>> + Send>>;
    /// Streaming type for NodeEventStream (S9)
    type NodeEventStreamStream = Pin<Box<dyn Stream<Item = Result<NodeEvent, Status>> + Send>>;
    type ServiceEventStreamStream = Pin<Box<dyn Stream<Item = Result<ServiceEvent, Status>> + Send>>;
    /// Streaming type for ServiceRestartEventStream (S11)
    type ServiceRestartEventStreamStream = Pin<Box<dyn Stream<Item = Result<ServiceRestartEvent, Status>> + Send>>;
    /// Streaming type for StreamTaskLogs (B02)
    type StreamTaskLogsStream = Pin<Box<dyn Stream<Item = Result<NormalizedLogEntry, Status>> + Send>>;

    async fn get_swarm_info(
        &self,
        _request: Request<SwarmInfoRequest>,
    ) -> Result<Response<SwarmInfoResponse>, Status> {
        debug!("Getting swarm info");

        use crate::docker::client::SwarmInspectResult;

        match self.state.docker.swarm_inspect().await {
            Ok(SwarmInspectResult::Manager(swarm)) => {
                let swarm_id = swarm.id.unwrap_or_default();

                let created_at = swarm.created_at.as_ref()
                    .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
                    .map(|dt| dt.timestamp())
                    .unwrap_or(0);
                let updated_at = swarm.updated_at.as_ref()
                    .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
                    .map(|dt| dt.timestamp())
                    .unwrap_or(0);

                // Get join tokens to determine manager/worker counts
                // We need to list nodes for accurate counts
                let nodes = self.state.docker.list_nodes().await
                    .unwrap_or_default();

                let managers = nodes.iter().filter(|n| {
                    n.spec.as_ref()
                        .and_then(|s| s.role.as_ref())
                        .map(|r| matches!(r, bollard::models::NodeSpecRoleEnum::MANAGER))
                        .unwrap_or(false)
                }).count() as u32;

                let workers = nodes.iter().filter(|n| {
                    n.spec.as_ref()
                        .and_then(|s| s.role.as_ref())
                        .map(|r| matches!(r, bollard::models::NodeSpecRoleEnum::WORKER))
                        .unwrap_or(false)
                }).count() as u32;

                // Get local node identity from Docker system info
                let sys_info = self.state.docker.system_info().await.ok();
                let (node_id, node_addr) = sys_info
                    .as_ref()
                    .and_then(|info| info.swarm.as_ref())
                    .map(|swarm_info| {
                        (
                            swarm_info.node_id.clone().unwrap_or_default(),
                            swarm_info.node_addr.clone().unwrap_or_default(),
                        )
                    })
                    .unwrap_or_default();

                let info = SwarmInfo {
                    swarm_id,
                    node_id,
                    node_addr,
                    is_manager: true, // If we can inspect swarm, we're a manager
                    managers,
                    workers,
                    created_at,
                    updated_at,
                };

                Ok(Response::new(SwarmInfoResponse {
                    is_swarm_mode: true,
                    swarm: Some(info),
                }))
            }
            Ok(SwarmInspectResult::Worker) => {
                // This node IS in a swarm but is a worker (not a manager).
                // Use system_info to retrieve local node identity.
                debug!("Node is a swarm worker (not a manager)");
                let sys_info = self.state.docker.system_info().await.ok();
                let swarm_info_field = sys_info.as_ref().and_then(|info| info.swarm.as_ref());

                let (node_id, node_addr) = swarm_info_field
                    .map(|si| (
                        si.node_id.clone().unwrap_or_default(),
                        si.node_addr.clone().unwrap_or_default(),
                    ))
                    .unwrap_or_default();

                // Workers can't list nodes or inspect the swarm object, so counts stay 0.
                let info = SwarmInfo {
                    swarm_id: String::new(), // not available to workers
                    node_id,
                    node_addr,
                    is_manager: false,
                    managers: 0,
                    workers: 0,
                    created_at: 0,
                    updated_at: 0,
                };

                Ok(Response::new(SwarmInfoResponse {
                    is_swarm_mode: true,
                    swarm: Some(info),
                }))
            }
            Ok(SwarmInspectResult::NotInSwarm) => {
                debug!("Not in swarm mode");
                Ok(Response::new(SwarmInfoResponse {
                    is_swarm_mode: false,
                    swarm: None,
                }))
            }
            Err(e) => {
                warn!("Failed to get swarm info: {}", e);
                Err(Status::internal(format!("Failed to get swarm info: {}", e)))
            }
        }
    }

    async fn list_nodes(
        &self,
        _request: Request<NodeListRequest>,
    ) -> Result<Response<NodeListResponse>, Status> {
        debug!("Listing swarm nodes");

        let nodes = self.state.docker.list_nodes().await
            .map_err(|e| Status::internal(format!("Failed to list nodes: {}", e)))?;

        let node_infos: Vec<NodeInfo> = nodes.iter().map(|n| convert_node_to_proto(n)).collect();

        Ok(Response::new(NodeListResponse { nodes: node_infos }))
    }

    async fn list_services(
        &self,
        _request: Request<ServiceListRequest>,
    ) -> Result<Response<ServiceListResponse>, Status> {
        debug!("Listing swarm services");

        let services = self.state.docker.list_services().await
            .map_err(|e| Status::internal(format!("Failed to list services: {}", e)))?;

        // Also get tasks for replica counting
        let tasks = match self.state.docker.list_tasks().await {
            Ok(t) => t,
            Err(e) => {
                warn!("Failed to list tasks for replica counting: {}", e);
                return Err(Status::internal(format!("Failed to list tasks: {}", e)));
            }
        };

        let service_infos: Vec<ServiceInfo> = services.into_iter().map(|s| {
            convert_service_to_proto(&s, &tasks)
        }).collect();

        Ok(Response::new(ServiceListResponse { services: service_infos }))
    }

    async fn inspect_service(
        &self,
        request: Request<ServiceInspectRequest>,
    ) -> Result<Response<ServiceInspectResponse>, Status> {
        let service_id = &request.into_inner().service_id;
        debug!(service_id = %service_id, "Inspecting swarm service");

        match self.state.docker.inspect_service(service_id).await {
            Ok(service) => {
                let tasks = match self.state.docker.list_tasks().await {
                    Ok(t) => t,
                    Err(e) => {
                        warn!(service_id = %service_id, "Failed to list tasks: {}", e);
                        return Err(Status::internal(format!("Failed to list tasks: {}", e)));
                    }
                };
                let info = convert_service_to_proto(&service, &tasks);
                Ok(Response::new(ServiceInspectResponse { service: Some(info) }))
            }
            Err(e) => {
                warn!(service_id = %service_id, "Failed to inspect service: {}", e);
                let msg = e.to_string();
                if msg.contains("404") || msg.to_lowercase().contains("not found") {
                    Err(Status::not_found(format!("Service not found: {}", service_id)))
                } else if msg.contains("403") || msg.to_lowercase().contains("permission") {
                    Err(Status::permission_denied(format!("Permission denied inspecting service: {}", e)))
                } else {
                    Err(Status::internal(format!("Failed to inspect service: {}", e)))
                }
            }
        }
    }

    async fn list_tasks(
        &self,
        request: Request<TaskListRequest>,
    ) -> Result<Response<TaskListResponse>, Status> {
        let req = request.into_inner();
        debug!(service_filter = ?req.service_id, "Listing tasks");

        let all_tasks = self.state.docker.list_tasks().await
            .map_err(|e| Status::internal(format!("Failed to list tasks: {}", e)))?;

        // Fetch services to resolve service_name for each task
        let services = self.state.docker.list_services().await.unwrap_or_default();
        let service_name_map: std::collections::HashMap<String, String> = services.iter()
            .filter_map(|s| {
                let id = s.id.as_ref()?.clone();
                let name = s.spec.as_ref()?.name.as_ref()?.clone();
                Some((id, name))
            })
            .collect();

        let task_infos: Vec<TaskInfo> = all_tasks.into_iter()
            .filter(|t| {
                if let Some(ref filter_service_id) = req.service_id {
                    t.service_id.as_deref() == Some(filter_service_id.as_str())
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

        Ok(Response::new(TaskListResponse { tasks: task_infos }))
    }

    // =========================================================================
    // S3: Service Log Aggregation — stream logs from all tasks of a service
    // =========================================================================

    async fn stream_service_logs(
        &self,
        request: Request<ServiceLogStreamRequest>,
    ) -> Result<Response<Self::StreamServiceLogsStream>, Status> {
        let req = request.into_inner();
        let service_id = req.service_id.clone();
        debug!(service_id = %service_id, "Streaming service logs");

        // First, resolve service name for SwarmContext enrichment
        let service = self.state.docker.inspect_service(&service_id).await
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
            .unwrap_or_else(|| service_id.clone());
        let resolved_service_id = service.id.clone().unwrap_or_else(|| service_id.clone());

        // Clamp timestamps like we do for container logs
        let since_raw = req.since.unwrap_or(0);
        let until_raw = req.until.unwrap_or(0);
        let since = since_raw.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        let until = until_raw.clamp(i32::MIN as i64, i32::MAX as i64) as i32;

        let tail = req.tail_lines.map(|n| n.to_string());

        let log_stream = self.state.docker.stream_service_logs(
            &service_id,
            req.follow,
            tail,
            since,
            until,
            req.timestamps,
        );

        // Get tasks for slot/node mapping
        let tasks = self.state.docker.list_tasks().await.unwrap_or_default();
        let service_tasks: Vec<_> = tasks.into_iter()
            .filter(|t| t.service_id.as_deref() == Some(&resolved_service_id) ||
                        t.service_id.as_deref() == Some(&service_id))
            .collect();

        let service_name_clone = service_name.clone();
        let service_id_clone = resolved_service_id.clone();

        use std::sync::atomic::{AtomicU64, Ordering};
        let sequence = std::sync::Arc::new(AtomicU64::new(0));
        let timestamps_enabled = req.timestamps;

        let output_stream = log_stream.map(move |result| {
            match result {
                Ok(output) => {
                    let (stream_type, raw_bytes) = match output {
                        bollard::container::LogOutput::StdOut { message } => (LogLevel::Stdout, message),
                        bollard::container::LogOutput::StdErr { message } => (LogLevel::Stderr, message),
                        bollard::container::LogOutput::StdIn { message } => (LogLevel::Stdout, message),
                        bollard::container::LogOutput::Console { message } => (LogLevel::Stdout, message),
                    };

                    // Docker service logs format:
                    //   "service.slot.taskid@nodeid    | <timestamp> <message>"
                    // First, try to split off the task prefix at " | ".
                    let raw_str = std::str::from_utf8(&raw_bytes).unwrap_or("");
                    let (task_prefix, after_prefix) = if let Some(pipe_idx) = raw_str.find(" | ") {
                        (Some(&raw_str[..pipe_idx]), &raw_str[pipe_idx + 3..])
                    } else {
                        (None, raw_str)
                    };

                    // Parse timestamp from the remainder (after prefix).
                    // Only attempt when timestamps were requested — otherwise
                    // Docker does not prepend timestamps and stripping the
                    // first space-delimited token would corrupt log content
                    // (especially for apps that emit ISO timestamps themselves).
                    let (timestamp, message_str) = if timestamps_enabled {
                        let first_space = after_prefix.find(' ');
                        match first_space {
                            Some(idx) => {
                                match chrono::DateTime::parse_from_rfc3339(&after_prefix[..idx]) {
                                    Ok(dt) => (
                                        dt.timestamp_nanos_opt().unwrap_or(0),
                                        &after_prefix[idx + 1..],
                                    ),
                                    Err(_) => (
                                        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0),
                                        after_prefix,
                                    ),
                                }
                            }
                            None => (
                                chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0),
                                after_prefix,
                            ),
                        }
                    } else {
                        (
                            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0),
                            after_prefix,
                        )
                    };
                    let content = bytes::Bytes::from(message_str.as_bytes().to_vec());

                    // Extract per-line task context from the prefix.
                    // Prefix format: "service_name.slot.task_id@node_id"
                    // e.g. "myapp.1.abc123def456@node1"
                    let swarm_ctx = {
                        let mut ctx_task_id = String::new();
                        let mut ctx_slot: u64 = 0;
                        let mut ctx_node_id = String::new();

                        if let Some(prefix) = task_prefix {
                            let trimmed = prefix.trim();
                            // Split at '@' to separate task part from node_id
                            if let Some(at_idx) = trimmed.rfind('@') {
                                ctx_node_id = trimmed[at_idx + 1..].to_string();
                                let task_part = &trimmed[..at_idx];
                                // task_part = "service_name.slot.task_id"
                                // Split from the RIGHT so that dotted service
                                // names (e.g. "my.app.1.abc123") are handled
                                // correctly: last segment = task_id,
                                // second-to-last = slot, rest = service_name.
                                let segments: Vec<&str> = task_part.rsplitn(3, '.').collect();
                                if segments.len() >= 3 {
                                    // rsplitn yields [task_id, slot, service_name]
                                    ctx_task_id = segments[0].to_string();
                                    ctx_slot = segments[1].parse::<u64>().unwrap_or(0);
                                    // segments[2] = service_name (unused here)
                                } else if segments.len() == 2 {
                                    ctx_task_id = segments[0].to_string();
                                    ctx_slot = segments[1].parse::<u64>().unwrap_or(0);
                                }
                            }
                        }

                        // If prefix parsing didn't yield a task_id, fall back
                        // to looking up the task from the pre-fetched task list
                        // by matching slot or just using the first task.
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
                        container_id: String::new(), // Service logs don't have a single container
                        timestamp_nanos: timestamp,
                        log_level: stream_type as i32,
                        sequence: seq,
                        raw_content: content.to_vec(),
                        parsed: None,
                        metadata: Some(ParseMetadata {
                            detected_format: 0, // Unknown format for service logs
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

        Ok(Response::new(Box::pin(output_stream)))
    }

    // =========================================================================
    // M6: Service Management — create/delete/update swarm services
    // =========================================================================

    async fn create_service(
        &self,
        request: Request<CreateServiceRequest>,
    ) -> Result<Response<CreateServiceResponse>, Status> {
        let req = request.into_inner();
        info!(name = %req.name, image = %req.image, "Creating swarm service");

        // Build ServiceSpec from request
        let mode = if req.global {
            Some(bollard::models::ServiceSpecMode {
                global: Some(Default::default()),
                ..Default::default()
            })
        } else {
            Some(bollard::models::ServiceSpecMode {
                replicated: Some(bollard::models::ServiceSpecModeReplicated {
                    replicas: Some(if req.replicas > 0 { req.replicas as i64 } else { 1 }),
                }),
                ..Default::default()
            })
        };

        // Build port configs
        let endpoint_spec = if !req.ports.is_empty() {
            Some(bollard::models::EndpointSpec {
                ports: Some(req.ports.iter().map(|p| {
                    bollard::models::EndpointPortConfig {
                        protocol: if p.protocol.is_empty() { 
                            Some(bollard::models::EndpointPortConfigProtocolEnum::TCP) 
                        } else {
                            match p.protocol.to_lowercase().as_str() {
                                "udp" => Some(bollard::models::EndpointPortConfigProtocolEnum::UDP),
                                "sctp" => Some(bollard::models::EndpointPortConfigProtocolEnum::SCTP),
                                _ => Some(bollard::models::EndpointPortConfigProtocolEnum::TCP),
                            }
                        },
                        target_port: Some(p.target_port as i64),
                        published_port: if p.published_port > 0 { Some(p.published_port as i64) } else { None },
                        publish_mode: if p.publish_mode == "host" {
                            Some(bollard::models::EndpointPortConfigPublishModeEnum::HOST)
                        } else {
                            Some(bollard::models::EndpointPortConfigPublishModeEnum::INGRESS)
                        },
                        ..Default::default()
                    }
                }).collect()),
                ..Default::default()
            })
        } else {
            None
        };

        // Build environment variables
        let env: Vec<String> = req.env.iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();

        // Build networks
        let networks = if !req.networks.is_empty() {
            Some(req.networks.iter().map(|n| {
                bollard::models::NetworkAttachmentConfig {
                    target: Some(n.clone()),
                    ..Default::default()
                }
            }).collect::<Vec<_>>())
        } else {
            None
        };

        // Build mounts (B13)
        let mounts = if !req.mounts.is_empty() {
            Some(req.mounts.iter().map(|m| {
                bollard::models::Mount {
                    target: Some(m.target.clone()),
                    source: Some(m.source.clone()),
                    typ: Some(match m.r#type.as_str() {
                        "volume" => bollard::models::MountTypeEnum::VOLUME,
                        "tmpfs" => bollard::models::MountTypeEnum::TMPFS,
                        _ => bollard::models::MountTypeEnum::BIND,
                    }),
                    read_only: Some(m.read_only),
                    ..Default::default()
                }
            }).collect::<Vec<_>>())
        } else {
            None
        };

        // Build resource limits and reservations (B13)
        let resources = {
            let limits = req.resource_limits.as_ref().map(|rl| {
                bollard::models::Limit {
                    nano_cpus: if rl.nano_cpus > 0 { Some(rl.nano_cpus) } else { None },
                    memory_bytes: if rl.memory_bytes > 0 { Some(rl.memory_bytes) } else { None },
                    pids: None,
                }
            });
            let reservations = req.resource_reservations.as_ref().map(|rr| {
                bollard::models::ResourceObject {
                    nano_cpus: if rr.nano_cpus > 0 { Some(rr.nano_cpus) } else { None },
                    memory_bytes: if rr.memory_bytes > 0 { Some(rr.memory_bytes) } else { None },
                    ..Default::default()
                }
            });
            if limits.is_some() || reservations.is_some() {
                Some(bollard::models::TaskSpecResources {
                    limits,
                    reservations,
                    swap_bytes: None,
                    memory_swappiness: None,
                })
            } else {
                None
            }
        };

        // Build restart policy (B13)
        let restart_policy = req.restart_policy.as_ref().map(|rp| {
            bollard::models::TaskSpecRestartPolicy {
                condition: Some(match rp.condition.as_str() {
                    "none" => bollard::models::TaskSpecRestartPolicyConditionEnum::NONE,
                    "on-failure" => bollard::models::TaskSpecRestartPolicyConditionEnum::ON_FAILURE,
                    _ => bollard::models::TaskSpecRestartPolicyConditionEnum::ANY,
                }),
                delay: if rp.delay_ns > 0 { Some(rp.delay_ns) } else { None },
                max_attempts: if rp.max_attempts > 0 { Some(rp.max_attempts as i64) } else { None },
                window: if rp.window_ns > 0 { Some(rp.window_ns) } else { None },
            }
        });

        // Build update config (B13)
        let update_config = req.update_config.as_ref().map(|uc| {
            bollard::models::ServiceSpecUpdateConfig {
                parallelism: Some(uc.parallelism as i64),
                delay: if uc.delay_ns > 0 { Some(uc.delay_ns) } else { None },
                failure_action: Some(match uc.failure_action.as_str() {
                    "continue" => bollard::models::ServiceSpecUpdateConfigFailureActionEnum::CONTINUE,
                    "rollback" => bollard::models::ServiceSpecUpdateConfigFailureActionEnum::ROLLBACK,
                    _ => bollard::models::ServiceSpecUpdateConfigFailureActionEnum::PAUSE,
                }),
                monitor: if uc.monitor_ns > 0 { Some(uc.monitor_ns) } else { None },
                max_failure_ratio: Some(uc.max_failure_ratio),
                order: Some(match uc.order.as_str() {
                    "start-first" => bollard::models::ServiceSpecUpdateConfigOrderEnum::START_FIRST,
                    _ => bollard::models::ServiceSpecUpdateConfigOrderEnum::STOP_FIRST,
                }),
            }
        });

        // Build rollback config (B13)
        let rollback_config = req.rollback_config.as_ref().map(|rc| {
            bollard::models::ServiceSpecRollbackConfig {
                parallelism: Some(rc.parallelism as i64),
                delay: if rc.delay_ns > 0 { Some(rc.delay_ns) } else { None },
                failure_action: Some(match rc.failure_action.as_str() {
                    "continue" => bollard::models::ServiceSpecRollbackConfigFailureActionEnum::CONTINUE,
                    _ => bollard::models::ServiceSpecRollbackConfigFailureActionEnum::PAUSE,
                }),
                monitor: if rc.monitor_ns > 0 { Some(rc.monitor_ns) } else { None },
                max_failure_ratio: Some(rc.max_failure_ratio),
                order: Some(match rc.order.as_str() {
                    "start-first" => bollard::models::ServiceSpecRollbackConfigOrderEnum::START_FIRST,
                    _ => bollard::models::ServiceSpecRollbackConfigOrderEnum::STOP_FIRST,
                }),
            }
        });

        // Build health check (B13)
        let health_check = req.health_check.as_ref().map(|hc| {
            bollard::models::HealthConfig {
                test: if hc.test.is_empty() { None } else { Some(hc.test.clone()) },
                interval: if hc.interval_ns > 0 { Some(hc.interval_ns) } else { None },
                timeout: if hc.timeout_ns > 0 { Some(hc.timeout_ns) } else { None },
                start_period: if hc.start_period_ns > 0 { Some(hc.start_period_ns) } else { None },
                retries: if hc.retries > 0 { Some(hc.retries as i64) } else { None },
                ..Default::default()
            }
        });

        // Build secret references (B13)
        let secrets = if !req.secrets.is_empty() {
            Some(req.secrets.iter().map(|s| {
                bollard::models::TaskSpecContainerSpecSecrets {
                    file: Some(bollard::models::TaskSpecContainerSpecFile {
                        name: Some(s.file_name.clone()),
                        uid: if s.file_uid.is_empty() { None } else { Some(s.file_uid.clone()) },
                        gid: if s.file_gid.is_empty() { None } else { Some(s.file_gid.clone()) },
                        mode: Some(if s.file_mode > 0 { s.file_mode } else { 0o444 }),
                    }),
                    secret_id: Some(s.secret_id.clone()),
                    secret_name: Some(s.secret_name.clone()),
                }
            }).collect::<Vec<_>>())
        } else {
            None
        };

        // Build config references (B13)
        let configs = if !req.configs.is_empty() {
            Some(req.configs.iter().map(|c| {
                bollard::models::TaskSpecContainerSpecConfigs {
                    file: Some(bollard::models::TaskSpecContainerSpecFile1 {
                        name: Some(c.file_name.clone()),
                        uid: if c.file_uid.is_empty() { None } else { Some(c.file_uid.clone()) },
                        gid: if c.file_gid.is_empty() { None } else { Some(c.file_gid.clone()) },
                        mode: Some(if c.file_mode > 0 { c.file_mode } else { 0o444 }),
                    }),
                    config_id: Some(c.config_id.clone()),
                    config_name: Some(c.config_name.clone()),
                    ..Default::default()
                }
            }).collect::<Vec<_>>())
        } else {
            None
        };

        // Build log driver
        let log_driver = if !req.log_driver.is_empty() {
            Some(bollard::models::TaskSpecLogDriver {
                name: Some(req.log_driver.clone()),
                options: if req.log_driver_opts.is_empty() { None } else { Some(req.log_driver_opts.clone()) },
            })
        } else {
            None
        };

        let spec = bollard::models::ServiceSpec {
            name: Some(req.name.clone()),
            mode,
            task_template: Some(bollard::models::TaskSpec {
                container_spec: Some(bollard::models::TaskSpecContainerSpec {
                    image: Some(req.image.clone()),
                    env: if env.is_empty() { None } else { Some(env) },
                    command: if req.command.is_empty() { None } else { Some(req.command.clone()) },
                    mounts,
                    health_check,
                    secrets,
                    configs,
                    ..Default::default()
                }),
                networks,
                placement: if req.constraints.is_empty() {
                    None
                } else {
                    Some(bollard::models::TaskSpecPlacement {
                        constraints: Some(req.constraints.clone()),
                        ..Default::default()
                    })
                },
                resources,
                restart_policy,
                log_driver,
                ..Default::default()
            }),
            labels: if req.labels.is_empty() { None } else { Some(req.labels.clone()) },
            endpoint_spec,
            update_config,
            rollback_config,
            ..Default::default()
        };

        let registry_auth_opt = if req.registry_auth.is_empty() { None } else { Some(req.registry_auth.as_str()) };

        match self.state.docker.create_service(spec, registry_auth_opt).await {
            Ok(service_id) => {
                info!(service_id = %service_id, name = %req.name, "Service created");
                Ok(Response::new(CreateServiceResponse {
                    service_id,
                    success: true,
                    message: format!("Service '{}' created successfully", req.name),
                }))
            }
            Err(e) => {
                warn!(name = %req.name, "Failed to create service: {}", e);
                Ok(Response::new(CreateServiceResponse {
                    service_id: String::new(),
                    success: false,
                    message: format!("Failed to create service: {}", e),
                }))
            }
        }
    }

    async fn delete_service(
        &self,
        request: Request<DeleteServiceRequest>,
    ) -> Result<Response<DeleteServiceResponse>, Status> {
        let service_id = request.into_inner().service_id;
        info!(service_id = %service_id, "Deleting swarm service");

        match self.state.docker.delete_service(&service_id).await {
            Ok(()) => {
                info!(service_id = %service_id, "Service deleted");
                Ok(Response::new(DeleteServiceResponse {
                    success: true,
                    message: format!("Service '{}' deleted successfully", service_id),
                }))
            }
            Err(e) => {
                warn!(service_id = %service_id, "Failed to delete service: {}", e);
                Ok(Response::new(DeleteServiceResponse {
                    success: false,
                    message: format!("Failed to delete service: {}", e),
                }))
            }
        }
    }

    async fn update_service(
        &self,
        request: Request<UpdateServiceRequest>,
    ) -> Result<Response<UpdateServiceResponse>, Status> {
        let req = request.into_inner();
        let service_id = req.service_id.clone();
        info!(service_id = %service_id, "Updating swarm service");

        // Inspect current service to get version and spec
        let current = self.state.docker.inspect_service(&service_id).await
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

        let version = current.version.as_ref()
            .and_then(|v| v.index)
            .unwrap_or(0);

        let mut spec = current.spec.unwrap_or_default();

        // Apply updates
        if let Some(image) = req.image {
            if let Some(ref mut tt) = spec.task_template {
                if let Some(ref mut cs) = tt.container_spec {
                    cs.image = Some(image);
                }
            }
        }

        if let Some(replicas) = req.replicas {
            if let Some(ref mut mode) = spec.mode {
                if let Some(ref mut replicated) = mode.replicated {
                    replicated.replicas = Some(replicas as i64);
                }
            }
        }

        // Apply environment variables (replaces all if set, or clear if clear_env)
        if !req.env.is_empty() || req.clear_env {
            if let Some(ref mut tt) = spec.task_template {
                if let Some(ref mut cs) = tt.container_spec {
                    cs.env = if req.env.is_empty() {
                        Some(Vec::new())
                    } else {
                        Some(req.env.iter().map(|(k, v)| format!("{}={}", k, v)).collect())
                    };
                }
            }
        }

        // Apply labels (replaces all if set, or clear if clear_labels)
        if !req.labels.is_empty() || req.clear_labels {
            spec.labels = Some(req.labels.clone());
        }

        // Apply networks (replaces all if set, or clear if clear_networks)
        if !req.networks.is_empty() || req.clear_networks {
            if let Some(ref mut tt) = spec.task_template {
                tt.networks = if req.networks.is_empty() {
                    Some(Vec::new())
                } else {
                    Some(req.networks.iter().map(|n| {
                        bollard::models::NetworkAttachmentConfig {
                            target: Some(n.clone()),
                            ..Default::default()
                        }
                    }).collect())
                };
            }
        }

        // Apply ports (replaces all if set, or clear if clear_ports)
        if !req.ports.is_empty() || req.clear_ports {
            spec.endpoint_spec = if req.ports.is_empty() {
                Some(bollard::models::EndpointSpec {
                    ports: Some(Vec::new()),
                    ..Default::default()
                })
            } else {
                Some(bollard::models::EndpointSpec {
                    ports: Some(req.ports.iter().map(|p| {
                        bollard::models::EndpointPortConfig {
                            protocol: if p.protocol.is_empty() {
                                Some(bollard::models::EndpointPortConfigProtocolEnum::TCP)
                            } else {
                                match p.protocol.to_lowercase().as_str() {
                                    "udp" => Some(bollard::models::EndpointPortConfigProtocolEnum::UDP),
                                    "sctp" => Some(bollard::models::EndpointPortConfigProtocolEnum::SCTP),
                                    _ => Some(bollard::models::EndpointPortConfigProtocolEnum::TCP),
                                }
                            },
                            target_port: Some(p.target_port as i64),
                            published_port: if p.published_port > 0 { Some(p.published_port as i64) } else { None },
                            publish_mode: if p.publish_mode == "host" {
                                Some(bollard::models::EndpointPortConfigPublishModeEnum::HOST)
                            } else {
                                Some(bollard::models::EndpointPortConfigPublishModeEnum::INGRESS)
                            },
                            ..Default::default()
                        }
                    }).collect()),
                    ..Default::default()
                })
            };
        }

        // Apply resource limits (B13)
        if req.resource_limits.is_some() || req.resource_reservations.is_some() {
            if let Some(ref mut tt) = spec.task_template {
                let mut resources = tt.resources.take().unwrap_or_default();
                if let Some(ref rl) = req.resource_limits {
                    resources.limits = Some(bollard::models::Limit {
                        nano_cpus: if rl.nano_cpus > 0 { Some(rl.nano_cpus) } else { None },
                        memory_bytes: if rl.memory_bytes > 0 { Some(rl.memory_bytes) } else { None },
                        pids: None,
                    });
                }
                if let Some(ref rr) = req.resource_reservations {
                    resources.reservations = Some(bollard::models::ResourceObject {
                        nano_cpus: if rr.nano_cpus > 0 { Some(rr.nano_cpus) } else { None },
                        memory_bytes: if rr.memory_bytes > 0 { Some(rr.memory_bytes) } else { None },
                        ..Default::default()
                    });
                }
                tt.resources = Some(resources);
            }
        }

        // Apply mounts (replaces all if set, or clear if clear_mounts, B13)
        if !req.mounts.is_empty() || req.clear_mounts {
            if let Some(ref mut tt) = spec.task_template {
                if let Some(ref mut cs) = tt.container_spec {
                    cs.mounts = if req.mounts.is_empty() {
                        Some(Vec::new())
                    } else {
                        Some(req.mounts.iter().map(|m| {
                        bollard::models::Mount {
                            target: Some(m.target.clone()),
                            source: Some(m.source.clone()),
                            typ: Some(match m.r#type.as_str() {
                                "volume" => bollard::models::MountTypeEnum::VOLUME,
                                "tmpfs" => bollard::models::MountTypeEnum::TMPFS,
                                _ => bollard::models::MountTypeEnum::BIND,
                            }),
                            read_only: Some(m.read_only),
                            ..Default::default()
                        }
                    }).collect())
                    };
                }
            }
        }

        // Apply restart policy (B13)
        if let Some(ref rp) = req.restart_policy {
            if let Some(ref mut tt) = spec.task_template {
                tt.restart_policy = Some(bollard::models::TaskSpecRestartPolicy {
                    condition: Some(match rp.condition.as_str() {
                        "none" => bollard::models::TaskSpecRestartPolicyConditionEnum::NONE,
                        "on-failure" => bollard::models::TaskSpecRestartPolicyConditionEnum::ON_FAILURE,
                        _ => bollard::models::TaskSpecRestartPolicyConditionEnum::ANY,
                    }),
                    delay: if rp.delay_ns > 0 { Some(rp.delay_ns) } else { None },
                    max_attempts: if rp.max_attempts > 0 { Some(rp.max_attempts as i64) } else { None },
                    window: if rp.window_ns > 0 { Some(rp.window_ns) } else { None },
                });
            }
        }

        // Apply update config (B13)
        if let Some(ref uc) = req.update_config {
            spec.update_config = Some(bollard::models::ServiceSpecUpdateConfig {
                parallelism: Some(uc.parallelism as i64),
                delay: if uc.delay_ns > 0 { Some(uc.delay_ns) } else { None },
                failure_action: Some(match uc.failure_action.as_str() {
                    "continue" => bollard::models::ServiceSpecUpdateConfigFailureActionEnum::CONTINUE,
                    "rollback" => bollard::models::ServiceSpecUpdateConfigFailureActionEnum::ROLLBACK,
                    _ => bollard::models::ServiceSpecUpdateConfigFailureActionEnum::PAUSE,
                }),
                monitor: if uc.monitor_ns > 0 { Some(uc.monitor_ns) } else { None },
                max_failure_ratio: Some(uc.max_failure_ratio),
                order: Some(match uc.order.as_str() {
                    "start-first" => bollard::models::ServiceSpecUpdateConfigOrderEnum::START_FIRST,
                    _ => bollard::models::ServiceSpecUpdateConfigOrderEnum::STOP_FIRST,
                }),
            });
        }

        // Apply rollback config (B13)
        if let Some(ref rc) = req.rollback_config {
            spec.rollback_config = Some(bollard::models::ServiceSpecRollbackConfig {
                parallelism: Some(rc.parallelism as i64),
                delay: if rc.delay_ns > 0 { Some(rc.delay_ns) } else { None },
                failure_action: Some(match rc.failure_action.as_str() {
                    "continue" => bollard::models::ServiceSpecRollbackConfigFailureActionEnum::CONTINUE,
                    _ => bollard::models::ServiceSpecRollbackConfigFailureActionEnum::PAUSE,
                }),
                monitor: if rc.monitor_ns > 0 { Some(rc.monitor_ns) } else { None },
                max_failure_ratio: Some(rc.max_failure_ratio),
                order: Some(match rc.order.as_str() {
                    "start-first" => bollard::models::ServiceSpecRollbackConfigOrderEnum::START_FIRST,
                    _ => bollard::models::ServiceSpecRollbackConfigOrderEnum::STOP_FIRST,
                }),
            });
        }

        // Apply constraints (replaces all if set, or clear if clear_constraints)
        if !req.constraints.is_empty() || req.clear_constraints {
            if let Some(ref mut tt) = spec.task_template {
                let mut placement = tt.placement.take().unwrap_or_default();
                placement.constraints = Some(req.constraints.clone());
                tt.placement = Some(placement);
            }
        }

        // Apply command (replaces if set, or clear if clear_command)
        if !req.command.is_empty() || req.clear_command {
            if let Some(ref mut tt) = spec.task_template {
                if let Some(ref mut cs) = tt.container_spec {
                    cs.command = if req.command.is_empty() {
                        None // clear = remove override, use image default
                    } else {
                        Some(req.command.clone())
                    };
                }
            }
        }

        // Force update by incrementing force_new_deployment counter
        if req.force {
            if let Some(ref mut tt) = spec.task_template {
                let current_force = tt.force_update.unwrap_or(0);
                tt.force_update = Some(current_force + 1);
            }
        }

        let registry_auth_opt = if req.registry_auth.is_empty() { None } else { Some(req.registry_auth.as_str()) };

        match self.state.docker.update_service(&service_id, spec, version, req.force, registry_auth_opt).await {
            Ok(()) => {
                info!(service_id = %service_id, "Service updated");
                Ok(Response::new(UpdateServiceResponse {
                    success: true,
                    message: format!("Service '{}' updated successfully", service_id),
                }))
            }
            Err(e) => {
                warn!(service_id = %service_id, "Failed to update service: {}", e);
                Ok(Response::new(UpdateServiceResponse {
                    success: false,
                    message: format!("Failed to update service: {}", e),
                }))
            }
        }
    }

    // ── S5: Swarm Networking ──────────────────────────────────────

    async fn list_swarm_networks(
        &self,
        request: Request<SwarmNetworkListRequest>,
    ) -> Result<Response<SwarmNetworkListResponse>, Status> {
        let req = request.into_inner();
        debug!(swarm_only = req.swarm_only, "Listing swarm networks");

        // Fetch all networks
        let networks = self.state.docker.list_networks().await
            .map_err(|e| Status::internal(format!("Failed to list networks: {}", e)))?;

        // Fetch services for cross-referencing VIPs
        let services = self.state.docker.list_services().await.unwrap_or_default();

        let networks: Vec<SwarmNetworkInfo> = networks.iter()
            .filter(|n| {
                if req.swarm_only {
                    n.scope.as_deref() == Some("swarm")
                } else {
                    true
                }
            })
            .map(|n| convert_network_to_proto(n, &services))
            .collect();

        debug!(count = networks.len(), "Listed swarm networks");
        Ok(Response::new(SwarmNetworkListResponse { networks }))
    }

    async fn inspect_swarm_network(
        &self,
        request: Request<SwarmNetworkInspectRequest>,
    ) -> Result<Response<SwarmNetworkInspectResponse>, Status> {
        let network_id = request.into_inner().network_id;
        debug!(network_id = %network_id, "Inspecting swarm network");

        let net = self.state.docker.inspect_network(&network_id).await
            .map_err(|e| {
                // Classify the error: only 404 is "not found"; other failures
                // (transport, permission, internal) should not be masked.
                let msg = e.to_string();
                if msg.contains("404") || msg.to_lowercase().contains("not found") {
                    Status::not_found(format!("Network not found: {}", e))
                } else if msg.contains("403") || msg.to_lowercase().contains("permission") {
                    Status::permission_denied(format!("Permission denied inspecting network: {}", e))
                } else {
                    Status::internal(format!("Failed to inspect network: {}", e))
                }
            })?;

        // Convert NetworkInspect → Network-like for reuse
        // NetworkInspect and Network share the same field layout in bollard
        let services = self.state.docker.list_services().await.unwrap_or_default();

        // bollard::models::Network and NetworkInspect have overlapping fields
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

        let network = convert_network_to_proto(&network_as_model, &services);

        Ok(Response::new(SwarmNetworkInspectResponse {
            network: Some(network),
        }))
    }

    // =========================================================================
    // S6: Swarm Orchestration Observability — rolling update stream
    // =========================================================================

    async fn service_update_stream(
        &self,
        request: Request<ServiceUpdateStreamRequest>,
    ) -> Result<Response<Self::ServiceUpdateStreamStream>, Status> {
        let req = request.into_inner();
        let service_id = req.service_id.clone();
        let poll_ms = req.poll_interval_ms.unwrap_or(1000).max(500); // min 500ms
        debug!(service_id = %service_id, poll_ms, "Starting service update stream");

        let state = self.state.clone();

        let stream = async_stream::try_stream! {
            let mut prev_task_states: std::collections::HashMap<String, (String, i64)> = std::collections::HashMap::new();

            loop {
                // Inspect service for update_status
                let service: bollard::models::Service = state.docker.inspect_service(&service_id).await
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

                // List tasks for this service (includes historical/failed tasks)
                let tasks: Vec<bollard::models::Task> = match state.docker.list_tasks().await {
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

                // Task state breakdown
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

                    // Detect changes since last poll
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

                // Prune prev_task_states for tasks no longer present to avoid unbounded growth
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

                // If update completed or rolled back, send final and stop
                // Note: "none" means no update in progress — keep polling so the
                // subscriber can observe when a new update starts.
                if matches!(update_state.as_str(), "completed" | "rollback_completed") {
                    break;
                }

                tokio::time::sleep(std::time::Duration::from_millis(poll_ms)).await;
            }
        };

        Ok(Response::new(Box::pin(stream)))
    }

    // =========================================================================
    // S8: Swarm Secrets & Configs (metadata only)
    // =========================================================================

    async fn list_secrets(
        &self,
        _request: Request<SecretListRequest>,
    ) -> Result<Response<SecretListResponse>, Status> {
        debug!("Listing swarm secrets (metadata only)");

        let secrets = self.state.docker.list_secrets().await
            .map_err(|e| {
                if matches!(e, crate::docker::client::DockerError::NotSwarmManager) {
                    Status::permission_denied(format!("{}", e))
                } else {
                    Status::internal(format!("Failed to list secrets: {}", e))
                }
            })?;

        let secret_infos: Vec<SwarmSecretInfo> = secrets.iter().map(|s| {
            let spec = s.spec.as_ref();
            let created_at = s.created_at.as_ref()
                .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
                .map(|dt| dt.timestamp())
                .unwrap_or(0);
            let updated_at = s.updated_at.as_ref()
                .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
                .map(|dt| dt.timestamp())
                .unwrap_or(0);

            SwarmSecretInfo {
                id: s.id.clone().unwrap_or_default(),
                name: spec.and_then(|sp| sp.name.clone()).unwrap_or_default(),
                created_at,
                updated_at,
                labels: spec.and_then(|sp| sp.labels.clone()).unwrap_or_default(),
                driver: spec.and_then(|sp| sp.driver.as_ref())
                    .map(|d| d.name.clone())
                    .unwrap_or_default(),
            }
        }).collect();

        info!("Listed {} swarm secrets", secret_infos.len());
        Ok(Response::new(SecretListResponse { secrets: secret_infos }))
    }

    async fn list_configs(
        &self,
        _request: Request<ConfigListRequest>,
    ) -> Result<Response<ConfigListResponse>, Status> {
        debug!("Listing swarm configs (metadata only)");

        let configs = self.state.docker.list_configs().await
            .map_err(|e| {
                if matches!(e, crate::docker::client::DockerError::NotSwarmManager) {
                    Status::permission_denied(format!("{}", e))
                } else {
                    Status::internal(format!("Failed to list configs: {}", e))
                }
            })?;

        let config_infos: Vec<SwarmConfigInfo> = configs.iter().map(|c| {
            let spec = c.spec.as_ref();
            let created_at = c.created_at.as_ref()
                .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
                .map(|dt| dt.timestamp())
                .unwrap_or(0);
            let updated_at = c.updated_at.as_ref()
                .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
                .map(|dt| dt.timestamp())
                .unwrap_or(0);

            SwarmConfigInfo {
                id: c.id.clone().unwrap_or_default(),
                name: spec.and_then(|sp| sp.name.clone()).unwrap_or_default(),
                created_at,
                updated_at,
                labels: spec.and_then(|sp| sp.labels.clone()).unwrap_or_default(),
            }
        }).collect();

        info!("Listed {} swarm configs", config_infos.len());
        Ok(Response::new(ConfigListResponse { configs: config_infos }))
    }

    // =========================================================================
    // S9: Node Management & Drain Awareness
    // =========================================================================

    async fn inspect_node(
        &self,
        request: Request<NodeInspectRequest>,
    ) -> Result<Response<NodeInspectResponse>, Status> {
        let node_id = request.into_inner().node_id;
        debug!("Inspecting node {}", node_id);

        let node = self.state.docker.inspect_node(&node_id).await
            .map_err(|e| Status::internal(format!("Failed to inspect node: {}", e)))?;

        let node_info = node.map(|n| convert_node_to_proto(&n));

        Ok(Response::new(NodeInspectResponse { node: node_info }))
    }

    async fn update_node(
        &self,
        request: Request<NodeUpdateRequest>,
    ) -> Result<Response<NodeUpdateResponse>, Status> {
        let req = request.into_inner();
        info!("Updating node {} — availability={:?}, role={:?}", req.node_id, req.availability, req.role);

        // First, inspect to get the current version and spec
        let current = self.state.docker.inspect_node(&req.node_id).await
            .map_err(|e| Status::internal(format!("Failed to inspect node for update: {}", e)))?
            .ok_or_else(|| Status::not_found(format!("Node {} not found", req.node_id)))?;

        let version = current.version
            .and_then(|v| v.index)
            .map(|i| i as i64)
            .ok_or_else(|| Status::internal("Node has no version".to_string()))?;

        let current_spec = current.spec.unwrap_or_default();

        // Build updated spec, keeping existing values where not overridden
        let availability = if let Some(ref avail_str) = req.availability {
            Some(match avail_str.to_lowercase().as_str() {
                "active" => bollard::models::NodeSpecAvailabilityEnum::ACTIVE,
                "pause" => bollard::models::NodeSpecAvailabilityEnum::PAUSE,
                "drain" => bollard::models::NodeSpecAvailabilityEnum::DRAIN,
                other => return Err(Status::invalid_argument(format!(
                    "Invalid availability: '{}'. Must be 'active', 'pause', or 'drain'", other
                ))),
            })
        } else {
            current_spec.availability
        };

        let role = if let Some(ref role_str) = req.role {
            Some(match role_str.to_lowercase().as_str() {
                "worker" => bollard::models::NodeSpecRoleEnum::WORKER,
                "manager" => bollard::models::NodeSpecRoleEnum::MANAGER,
                other => return Err(Status::invalid_argument(format!(
                    "Invalid role: '{}'. Must be 'worker' or 'manager'", other
                ))),
            })
        } else {
            current_spec.role
        };

        let labels = if !req.labels.is_empty() {
            Some(req.labels)
        } else {
            current_spec.labels
        };

        let new_spec = bollard::models::NodeSpec {
            name: current_spec.name,
            labels,
            role,
            availability,
        };

        self.state.docker.update_node(&req.node_id, new_spec, version).await
            .map_err(|e| Status::internal(format!("Failed to update node: {}", e)))?;

        info!("Successfully updated node {}", req.node_id);
        Ok(Response::new(NodeUpdateResponse {
            success: true,
            message: format!("Node {} updated successfully", req.node_id),
        }))
    }

    async fn node_event_stream(
        &self,
        request: Request<NodeEventStreamRequest>,
    ) -> Result<Response<Self::NodeEventStreamStream>, Status> {
        let req = request.into_inner();
        let filter_node_id = if req.node_id.is_empty() { None } else { Some(req.node_id) };
        let poll_ms = if req.poll_interval_ms == 0 { 2000 } else { req.poll_interval_ms };
        let state = self.state.clone();

        info!("Starting node event stream (filter={:?}, poll={}ms)", filter_node_id, poll_ms);

        let stream = async_stream::try_stream! {
            // Track previous node states for diff detection
            let mut prev_states: std::collections::HashMap<String, (String, String, String)> = std::collections::HashMap::new();
            // Track nodes in drain mode to detect drain completion
            let mut draining_nodes: std::collections::HashMap<String, bool> = std::collections::HashMap::new();

            // Seed initial state
            let initial_nodes = state.docker.list_nodes().await.unwrap_or_default();
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

                let nodes = match state.docker.list_nodes().await {
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

                    // Detect state change (ready/down/disconnected)
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

                    // Detect availability change (active/pause/drain)
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

                            // Get affected tasks for drain events
                            let affected_tasks = if availability == "drain" {
                                match state.docker.list_tasks().await {
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

                    // Detect role change (worker/manager)
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

                    // Check for drain completion: node is draining and has no more running tasks
                    if availability == "drain" {
                        let was_draining = draining_nodes.get(&node_id).copied().unwrap_or(false);
                        if !was_draining {
                            let tasks = match state.docker.list_tasks().await {
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

                    // New node appeared (not in previous state)
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

                // Prune state for nodes that no longer appear in the API
                // response, preventing unbounded memory growth on long-lived
                // streams.
                let current_node_ids: std::collections::HashSet<String> = nodes.iter()
                    .filter_map(|n| n.id.clone())
                    .collect();
                prev_states.retain(|id, _| current_node_ids.contains(id));
                draining_nodes.retain(|id, _| current_node_ids.contains(id));
            }
        };

        Ok(Response::new(Box::pin(stream)))
    }

    // =========================================================================
    // S10: Global & Replicated Service Scaling Insights
    // =========================================================================

    async fn service_event_stream(
        &self,
        request: Request<ServiceEventStreamRequest>,
    ) -> Result<Response<Self::ServiceEventStreamStream>, Status> {
        let req = request.into_inner();
        let service_id = req.service_id.trim().to_string();
        if service_id.is_empty() {
            return Err(Status::invalid_argument("service_id must not be empty"));
        }
        let poll_ms = if req.poll_interval_ms == 0 { 2000 } else { req.poll_interval_ms };
        let state = self.state.clone();

        info!("Starting service event stream for service {} (poll={}ms)", service_id, poll_ms);

        let stream = async_stream::try_stream! {
            // Track previous state for diff detection
            let mut prev_replicas_desired: Option<u64> = None;
            let mut prev_replicas_running: Option<u64> = None;
            let mut prev_update_state: Option<String> = None;
            // Track task states: task_id -> (state, desired_state)
            let mut prev_task_states: std::collections::HashMap<String, (String, String)> = std::collections::HashMap::new();

            // Seed initial state
            if let Ok(services) = state.docker.list_services().await {
                if let Some(svc) = services.iter().find(|s| s.id.as_deref() == Some(&service_id)) {
                    let spec = svc.spec.as_ref();
                    let mode_spec = spec.and_then(|sp| sp.mode.as_ref());
                    if let Some(mode) = mode_spec {
                        if let Some(replicated) = &mode.replicated {
                            prev_replicas_desired = Some(replicated.replicas.unwrap_or(1) as u64);
                        }
                    }
                    // Seed update state
                    prev_update_state = svc.update_status.as_ref()
                        .and_then(|us| us.state.as_ref())
                        .map(|s| format!("{:?}", s).to_lowercase());
                }
            }

            // Seed task states
            if let Ok(tasks) = state.docker.list_tasks().await {
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

                // Fetch current service state
                let services = match state.docker.list_services().await {
                    Ok(s) => s,
                    Err(e) => {
                        warn!("Failed to list services in event stream: {}", e);
                        continue;
                    }
                };

                let svc = match services.iter().find(|s| s.id.as_deref() == Some(&service_id)) {
                    Some(s) => s,
                    None => {
                        // Service not found — may have been deleted. Emit a final
                        // message and terminate instead of idling silently forever.
                        Err(Status::not_found(format!("Service {} no longer exists", service_id)))?;
                        unreachable!()
                    }
                };

                let spec = svc.spec.as_ref();
                let mode_spec = spec.and_then(|sp| sp.mode.as_ref());

                // Current desired replicas
                let current_desired = mode_spec.and_then(|m| {
                    m.replicated.as_ref().map(|r| r.replicas.unwrap_or(1) as u64)
                });

                // Detect scaling events (replicas_desired changed)
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
                let tasks = match state.docker.list_tasks().await {
                    Ok(t) => t,
                    Err(e) => {
                        warn!("Failed to list tasks in service event stream: {}", e);
                        continue;
                    }
                };

                let service_tasks: Vec<&bollard::models::Task> = tasks.iter()
                    .filter(|t| t.service_id.as_deref() == Some(&service_id))
                    .collect();

                // Current running count
                let current_running = service_tasks.iter()
                    .filter(|t| {
                        t.status.as_ref()
                            .and_then(|s| s.state.as_ref())
                            .map(|s| matches!(s, bollard::models::TaskState::RUNNING))
                            .unwrap_or(false)
                    })
                    .count() as u64;

                // Build new task state map
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

                    // Check for task failure: new state is "failed" or "rejected"
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
                        // New task that wasn't in previous state — check if it's
                        // a genuine recovery from a prior failure (not a normal
                        // scale-up or rolling update).  We only emit
                        // TASK_RECOVERED when:
                        //   1. The task is running,
                        //   2. We have prior state (not the very first poll), AND
                        //   3. There was at least one failed/rejected task in
                        //      the previous snapshot (evidence of a failure that
                        //      this new task is recovering from).
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

        Ok(Response::new(Box::pin(stream)))
    }

    async fn get_service_coverage(
        &self,
        request: Request<ServiceCoverageRequest>,
    ) -> Result<Response<ServiceCoverageResponse>, Status> {
        let service_id = request.into_inner().service_id;
        info!("Getting service coverage for {}", service_id);

        // Get service to check if it's global
        let services = self.state.docker.list_services().await
            .map_err(|e| Status::internal(format!("Failed to list services: {}", e)))?;

        let svc = services.iter()
            .find(|s| s.id.as_deref() == Some(&service_id))
            .ok_or_else(|| Status::not_found(format!("Service {} not found", service_id)))?;

        let spec = svc.spec.as_ref();
        let mode_spec = spec.and_then(|sp| sp.mode.as_ref());
        let is_global = mode_spec.map(|m| m.global.is_some() || m.global_job.is_some()).unwrap_or(false);

        // Get all nodes (only "active" + "ready" nodes are eligible for scheduling)
        let nodes = self.state.docker.list_nodes().await
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

        // Get tasks for this service that are running
        let tasks = self.state.docker.list_tasks().await
            .map_err(|e| Status::internal(format!("Failed to list tasks: {}", e)))?;

        let covered: std::collections::HashSet<String> = tasks.iter()
            .filter(|t| t.service_id.as_deref() == Some(&service_id))
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

        Ok(Response::new(ServiceCoverageResponse {
            coverage: Some(ServiceCoverage {
                covered_nodes,
                uncovered_nodes,
                total_nodes: total,
                coverage_percentage: coverage_pct,
                service_id,
                is_global,
            }),
        }))
    }

    // =========================================================================
    // S11: Stack-Level Health & Restart Policies
    // =========================================================================

    async fn get_stack_health(
        &self,
        request: Request<StackHealthRequest>,
    ) -> Result<Response<StackHealthResponse>, Status> {
        let namespace = request.into_inner().namespace;
        info!("Computing stack health for namespace '{}'", namespace);

        let services = self.state.docker.list_services().await
            .map_err(|e| Status::internal(format!("Failed to list services: {}", e)))?;

        let tasks = self.state.docker.list_tasks().await
            .map_err(|e| Status::internal(format!("Failed to list tasks: {}", e)))?;

        // Filter services belonging to this stack
        let stack_services: Vec<&bollard::models::Service> = services.iter()
            .filter(|s| {
                s.spec.as_ref()
                    .and_then(|sp| sp.labels.as_ref())
                    .and_then(|l| l.get("com.docker.stack.namespace"))
                    .map(|ns| ns == &namespace)
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

            // Desired replicas
            let desired = if let Some(mode) = mode_spec {
                if let Some(replicated) = &mode.replicated {
                    replicated.replicas.unwrap_or(1) as u64
                } else if mode.global.is_some() {
                    // For global services, count nodes that are both ACTIVE
                    // and READY.  Nodes that are down, disconnected, or
                    // draining will not receive a task so they should not
                    // count towards the desired replica total.
                    self.state.docker.list_nodes().await
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

            // Count running and failed tasks for this service
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

            // Recent error messages (last 5 failures)
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

            // Update in progress?
            let update_in_progress = svc.update_status.as_ref()
                .and_then(|us| us.state.as_ref())
                .map(|s| format!("{:?}", s).to_lowercase().contains("updating"))
                .unwrap_or(false);

            // Restart policy
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

            // Determine health status
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

        // Overall stack health
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

        Ok(Response::new(StackHealthResponse {
            health: Some(StackHealth {
                namespace,
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
        }))
    }

    async fn service_restart_event_stream(
        &self,
        request: Request<ServiceRestartEventStreamRequest>,
    ) -> Result<Response<Self::ServiceRestartEventStreamStream>, Status> {
        let req = request.into_inner();
        let filter_service_id = if req.service_id.is_empty() { None } else { Some(req.service_id) };
        let poll_ms = if req.poll_interval_ms == 0 { 2000 } else { req.poll_interval_ms };
        let state = self.state.clone();

        info!("Starting service restart event stream (filter={:?}, poll={}ms)", filter_service_id, poll_ms);

        let stream = async_stream::try_stream! {
            // Track tasks by (service_id, slot) -> (task_id, state, timestamp)
            let mut slot_tasks: std::collections::HashMap<(String, u64), (String, String, i64)> = std::collections::HashMap::new();
            // Track restart counts per (service_id, slot) within a sliding window
            let mut restart_counts: std::collections::HashMap<(String, u64), Vec<i64>> = std::collections::HashMap::new();
            // Cache service names
            let mut service_names: std::collections::HashMap<String, String> = std::collections::HashMap::new();

            // Seed initial state
            if let Ok(services) = state.docker.list_services().await {
                for svc in &services {
                    let svc_id = svc.id.clone().unwrap_or_default();
                    let svc_name = svc.spec.as_ref()
                        .and_then(|sp| sp.name.clone())
                        .unwrap_or_default();
                    service_names.insert(svc_id, svc_name);
                }
            }

            if let Ok(tasks) = state.docker.list_tasks().await {
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
                        // Only track the most recent task per slot (running or preparing)
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

                // Refresh service names periodically
                if let Ok(services) = state.docker.list_services().await {
                    for svc in &services {
                        let svc_id = svc.id.clone().unwrap_or_default();
                        let svc_name = svc.spec.as_ref()
                            .and_then(|sp| sp.name.clone())
                            .unwrap_or_default();
                        service_names.insert(svc_id, svc_name);
                    }
                }

                let tasks = match state.docker.list_tasks().await {
                    Ok(t) => t,
                    Err(e) => {
                        warn!("Failed to list tasks in restart event stream: {}", e);
                        continue;
                    }
                };

                // Group latest tasks by (service_id, slot)
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
                        // Different task in same slot = restart
                        if &task_id != prev_task_id {
                            // Record restart timestamp
                            let restarts = restart_counts.entry(key.clone()).or_default();
                            restarts.push(now);
                            // Prune restarts older than 5 minutes
                            restarts.retain(|&ts| now - ts < 300);
                            let count = restarts.len() as u32;

                            // Find the old (failed) task for context
                            let old_task = tasks.iter()
                                .find(|t| t.id.as_deref() == Some(prev_task_id));
                            let old_task_proto = old_task.map(|t| convert_task_to_proto(t));

                            // Check for OOM on the OLD (failed) task — Docker records
                            // OOM information on the task that was killed, not the
                            // replacement task that was just scheduled.
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

                // Prune slots that no longer exist in current tasks to prevent
                // stale entries from triggering false restart events after
                // scale-down followed by scale-up (slot ID reuse).
                slot_tasks.retain(|k, _| current_slot_tasks.contains_key(k));
                restart_counts.retain(|k, _| current_slot_tasks.contains_key(k));
            }
        };

        Ok(Response::new(Box::pin(stream)))
    }

    // =========================================================================
    // B06: Service Rollback
    // =========================================================================

    async fn rollback_service(
        &self,
        request: Request<RollbackServiceRequest>,
    ) -> Result<Response<RollbackServiceResponse>, Status> {
        let req = request.into_inner();
        info!(service_id = %req.service_id, "Rolling back service");

        match self.state.docker.rollback_service(&req.service_id).await {
            Ok(()) => {
                info!(service_id = %req.service_id, "Service rollback initiated");
                Ok(Response::new(RollbackServiceResponse {
                    success: true,
                    message: format!("Service {} rollback initiated", req.service_id),
                }))
            }
            Err(e) => Err(Status::internal(format!("Failed to rollback service: {}", e))),
        }
    }

    // =========================================================================
    // B08: Secret CRUD
    // =========================================================================

    async fn create_secret(
        &self,
        request: Request<CreateSecretRequest>,
    ) -> Result<Response<CreateSecretResponse>, Status> {
        let req = request.into_inner();
        info!(name = %req.name, "Creating swarm secret");

        match self.state.docker.create_secret(&req.name, &req.data, req.labels).await {
            Ok(secret_id) => {
                info!(name = %req.name, secret_id = %secret_id, "Secret created");
                Ok(Response::new(CreateSecretResponse {
                    success: true,
                    message: format!("Secret {} created", req.name),
                    secret_id,
                }))
            }
            Err(e) => Err(Status::internal(format!("Failed to create secret: {}", e))),
        }
    }

    async fn delete_secret(
        &self,
        request: Request<DeleteSecretRequest>,
    ) -> Result<Response<DeleteSecretResponse>, Status> {
        let req = request.into_inner();
        info!(secret_id = %req.secret_id, "Deleting swarm secret");

        match self.state.docker.delete_secret(&req.secret_id).await {
            Ok(()) => Ok(Response::new(DeleteSecretResponse {
                success: true,
                message: format!("Secret {} deleted", req.secret_id),
            })),
            Err(e) => Err(Status::internal(format!("Failed to delete secret: {}", e))),
        }
    }

    // =========================================================================
    // B09: Config CRUD
    // =========================================================================

    async fn create_config(
        &self,
        request: Request<CreateConfigRequest>,
    ) -> Result<Response<CreateConfigResponse>, Status> {
        let req = request.into_inner();
        info!(name = %req.name, "Creating swarm config");

        match self.state.docker.create_config(&req.name, &req.data, req.labels).await {
            Ok(config_id) => {
                info!(name = %req.name, config_id = %config_id, "Config created");
                Ok(Response::new(CreateConfigResponse {
                    success: true,
                    message: format!("Config {} created", req.name),
                    config_id,
                }))
            }
            Err(e) => Err(Status::internal(format!("Failed to create config: {}", e))),
        }
    }

    async fn delete_config(
        &self,
        request: Request<DeleteConfigRequest>,
    ) -> Result<Response<DeleteConfigResponse>, Status> {
        let req = request.into_inner();
        info!(config_id = %req.config_id, "Deleting swarm config");

        match self.state.docker.delete_config(&req.config_id).await {
            Ok(()) => Ok(Response::new(DeleteConfigResponse {
                success: true,
                message: format!("Config {} deleted", req.config_id),
            })),
            Err(e) => Err(Status::internal(format!("Failed to delete config: {}", e))),
        }
    }

    // =========================================================================
    // B04/B05: Swarm Init / Join / Leave
    // =========================================================================

    async fn swarm_init(
        &self,
        request: Request<SwarmInitRequest>,
    ) -> Result<Response<SwarmInitResponse>, Status> {
        let req = request.into_inner();
        let listen_addr = if req.listen_addr.is_empty() { "0.0.0.0:2377" } else { &req.listen_addr };
        info!(listen_addr = %listen_addr, "Initializing swarm");

        match self.state.docker.swarm_init(listen_addr, &req.advertise_addr, req.force_new_cluster).await {
            Ok(node_id) => {
                info!(node_id = %node_id, "Swarm initialized");
                Ok(Response::new(SwarmInitResponse {
                    success: true,
                    message: "Swarm initialized successfully".to_string(),
                    node_id,
                }))
            }
            Err(e) => Err(Status::internal(format!("Failed to initialize swarm: {}", e))),
        }
    }

    async fn swarm_join(
        &self,
        request: Request<SwarmJoinRequest>,
    ) -> Result<Response<SwarmJoinResponse>, Status> {
        let req = request.into_inner();
        let listen_addr = if req.listen_addr.is_empty() { "0.0.0.0:2377" } else { &req.listen_addr };
        info!(remote_addrs = ?req.remote_addrs, "Joining swarm");

        match self.state.docker.swarm_join(req.remote_addrs, &req.join_token, listen_addr, &req.advertise_addr).await {
            Ok(()) => {
                info!("Successfully joined swarm");
                Ok(Response::new(SwarmJoinResponse {
                    success: true,
                    message: "Successfully joined swarm".to_string(),
                }))
            }
            Err(e) => Err(Status::internal(format!("Failed to join swarm: {}", e))),
        }
    }

    async fn swarm_leave(
        &self,
        request: Request<SwarmLeaveRequest>,
    ) -> Result<Response<SwarmLeaveResponse>, Status> {
        let req = request.into_inner();
        info!(force = req.force, "Leaving swarm");

        match self.state.docker.swarm_leave(req.force).await {
            Ok(()) => {
                info!("Successfully left swarm");
                Ok(Response::new(SwarmLeaveResponse {
                    success: true,
                    message: "Successfully left swarm".to_string(),
                }))
            }
            Err(e) => Err(Status::internal(format!("Failed to leave swarm: {}", e))),
        }
    }

    // =========================================================================
    // B07: Node Remove
    // =========================================================================

    async fn remove_node(
        &self,
        request: Request<RemoveNodeRequest>,
    ) -> Result<Response<RemoveNodeResponse>, Status> {
        let req = request.into_inner();
        info!(node_id = %req.node_id, force = req.force, "Removing node from swarm");

        match self.state.docker.remove_node(&req.node_id, req.force).await {
            Ok(()) => Ok(Response::new(RemoveNodeResponse {
                success: true,
                message: format!("Node {} removed from swarm", req.node_id),
            })),
            Err(e) => Err(Status::internal(format!("Failed to remove node: {}", e))),
        }
    }

    // =========================================================================
    // B14: Network Connect / Disconnect
    // =========================================================================

    async fn network_connect(
        &self,
        request: Request<NetworkConnectRequest>,
    ) -> Result<Response<NetworkConnectResponse>, Status> {
        let req = request.into_inner();
        info!(network_id = %req.network_id, container_id = %req.container_id, "Connecting container to network");

        match self.state.docker.network_connect(&req.network_id, &req.container_id).await {
            Ok(()) => Ok(Response::new(NetworkConnectResponse {
                success: true,
                message: format!("Container {} connected to network {}", req.container_id, req.network_id),
            })),
            Err(e) => Err(Status::internal(format!("Failed to connect to network: {}", e))),
        }
    }

    async fn network_disconnect(
        &self,
        request: Request<NetworkDisconnectRequest>,
    ) -> Result<Response<NetworkDisconnectResponse>, Status> {
        let req = request.into_inner();
        info!(network_id = %req.network_id, container_id = %req.container_id, "Disconnecting container from network");

        match self.state.docker.network_disconnect(&req.network_id, &req.container_id, req.force).await {
            Ok(()) => Ok(Response::new(NetworkDisconnectResponse {
                success: true,
                message: format!("Container {} disconnected from network {}", req.container_id, req.network_id),
            })),
            Err(e) => Err(Status::internal(format!("Failed to disconnect from network: {}", e))),
        }
    }

    // =========================================================================
    // B02: Task Log Streaming
    // =========================================================================

    async fn stream_task_logs(
        &self,
        request: Request<TaskLogStreamRequest>,
    ) -> Result<Response<Self::StreamTaskLogsStream>, Status> {
        let req = request.into_inner();
        let task_id = req.task_id.clone();
        info!(task_id = %task_id, "Streaming task logs");

        let since = req.since.unwrap_or(0).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        let until = req.until.unwrap_or(0).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        let tail = req.tail_lines.map(|n| n.to_string());

        let docker = self.state.docker.clone();
        let raw_stream = docker.stream_task_logs(
            &task_id,
            req.follow,
            tail,
            since,
            until,
            req.timestamps,
        );

        // Look up task metadata once: container_id, service_id, node_id, slot, service_name
        let (real_container_id, service_id, node_id, task_slot, service_name) =
            match docker.inspect_task(&task_id).await {
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

        let task_id_clone = task_id.clone();
        let timestamps_enabled = req.timestamps;
        let sequence_counter = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let output_stream = raw_stream.map(move |result| {
            match result {
                Ok(log_output) => {
                    let (stream_type, raw_bytes) = match log_output {
                        bollard::container::LogOutput::StdOut { message } => (LogLevel::Stdout as i32, message),
                        bollard::container::LogOutput::StdErr { message } => (LogLevel::Stderr as i32, message),
                        bollard::container::LogOutput::StdIn { message } => (LogLevel::Stdout as i32, message),
                        bollard::container::LogOutput::Console { message } => (LogLevel::Stdout as i32, message),
                    };

                    // Parse Docker timestamp from log line (RFC3339 prefix before first space)
                    // Only attempt when timestamps were requested — otherwise Docker
                    // does not prepend timestamps, and stripping the first token
                    // would corrupt application log content.
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

                    let seq = sequence_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

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
                            task_slot: task_slot,
                            node_id: node_id.clone(),
                        }),
                    })
                }
                Err(e) => Err(Status::internal(format!("Task log stream error: {}", e))),
            }
        });

        Ok(Response::new(Box::pin(output_stream)))
    }

    // =========================================================================
    // B03: Task Inspect
    // =========================================================================

    async fn inspect_task(
        &self,
        request: Request<TaskInspectRequest>,
    ) -> Result<Response<TaskInspectResponse>, Status> {
        let task_id = request.into_inner().task_id;
        debug!(task_id = %task_id, "Inspecting task");

        match self.state.docker.inspect_task(&task_id).await {
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
                    .map(|l| super::proto::ServiceResourceLimits {
                        nano_cpus: l.nano_cpus.unwrap_or(0),
                        memory_bytes: l.memory_bytes.unwrap_or(0),
                    });

                let resource_reservations = spec
                    .and_then(|s| s.resources.as_ref())
                    .and_then(|r| r.reservations.as_ref())
                    .map(|r| super::proto::ServiceResourceReservations {
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

                let finished_at = String::new();

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
                    finished_at,
                    port_status: Vec::new(),
                };

                Ok(Response::new(TaskInspectResponse { task: Some(info) }))
            }
            Ok(None) => Ok(Response::new(TaskInspectResponse { task: None })),
            Err(e) => Err(Status::internal(format!("Failed to inspect task: {}", e))),
        }
    }

    // =========================================================================
    // B05: Swarm Update / Unlock
    // =========================================================================

    async fn swarm_update(
        &self,
        request: Request<SwarmUpdateRequest>,
    ) -> Result<Response<SwarmUpdateResponse>, Status> {
        let req = request.into_inner();
        info!("Updating swarm settings");

        // Get current swarm to obtain version and spec
        use crate::docker::client::SwarmInspectResult;
        let swarm = match self.state.docker.swarm_inspect().await
            .map_err(|e| Status::internal(format!("Failed to inspect swarm: {}", e)))? {
            SwarmInspectResult::Manager(s) => s,
            SwarmInspectResult::Worker => return Err(Status::failed_precondition("This node is a worker, not a manager. Swarm updates require a manager node.")),
            SwarmInspectResult::NotInSwarm => return Err(Status::failed_precondition("Not in swarm mode")),
        };

        let version = swarm.version.as_ref()
            .and_then(|v| v.index)
            .ok_or_else(|| Status::internal("Swarm has no version"))? as i64;

        let mut spec = swarm.spec.unwrap_or_default();

        // Apply updates to spec
        if let Some(autolock) = req.autolock {
            let enc = spec.encryption_config.get_or_insert_with(Default::default);
            enc.auto_lock_managers = Some(autolock);
        }

        if let Some(task_history_limit) = req.task_history_limit {
            let orch = spec.orchestration.get_or_insert_with(Default::default);
            orch.task_history_retention_limit = Some(task_history_limit);
        }

        if let Some(snapshot_interval) = req.snapshot_interval {
            let raft = spec.raft.get_or_insert_with(Default::default);
            raft.snapshot_interval = Some(snapshot_interval);
        }

        if let Some(heartbeat_tick) = req.heartbeat_tick {
            let raft = spec.raft.get_or_insert_with(Default::default);
            raft.heartbeat_tick = Some(heartbeat_tick as i64);
        }

        if let Some(election_tick) = req.election_tick {
            let raft = spec.raft.get_or_insert_with(Default::default);
            raft.election_tick = Some(election_tick as i64);
        }

        if let Some(cert_expiry_ns) = req.cert_expiry_ns {
            let ca = spec.ca_config.get_or_insert_with(Default::default);
            ca.node_cert_expiry = Some(cert_expiry_ns);
        }

        match self.state.docker.swarm_update(
            spec,
            version,
            req.rotate_worker_token,
            req.rotate_manager_token,
            req.rotate_manager_unlock_key,
        ).await {
            Ok(()) => Ok(Response::new(SwarmUpdateResponse {
                success: true,
                message: "Swarm settings updated successfully".to_string(),
            })),
            Err(e) => Ok(Response::new(SwarmUpdateResponse {
                success: false,
                message: format!("Failed to update swarm: {}", e),
            })),
        }
    }

    async fn swarm_unlock_key(
        &self,
        _request: Request<SwarmUnlockKeyRequest>,
    ) -> Result<Response<SwarmUnlockKeyResponse>, Status> {
        info!("Retrieving swarm unlock key");

        match self.state.docker.swarm_unlock_key().await {
            Ok(key) => Ok(Response::new(SwarmUnlockKeyResponse {
                success: true,
                unlock_key: key,
                message: String::new(),
            })),
            Err(e) => Ok(Response::new(SwarmUnlockKeyResponse {
                success: false,
                unlock_key: String::new(),
                message: format!("{}", e),
            })),
        }
    }

    async fn swarm_unlock(
        &self,
        request: Request<SwarmUnlockRequest>,
    ) -> Result<Response<SwarmUnlockResponse>, Status> {
        let req = request.into_inner();
        info!("Unlocking swarm");

        match self.state.docker.swarm_unlock(&req.unlock_key).await {
            Ok(()) => Ok(Response::new(SwarmUnlockResponse {
                success: true,
                message: "Swarm unlocked successfully".to_string(),
            })),
            Err(e) => Ok(Response::new(SwarmUnlockResponse {
                success: false,
                message: format!("{}", e),
            })),
        }
    }

    // =========================================================================
    // B11: Compose Stack Deployment
    // =========================================================================

    async fn deploy_compose_stack(
        &self,
        request: Request<DeployComposeStackRequest>,
    ) -> Result<Response<DeployComposeStackResponse>, Status> {
        let req = request.into_inner();
        info!(stack_name = %req.stack_name, "Deploying compose stack");

        // Parse the compose YAML
        let compose: serde_yaml::Value = match serde_yaml::from_str(&req.compose_yaml) {
            Ok(v) => v,
            Err(e) => {
                return Ok(Response::new(DeployComposeStackResponse {
                    success: false,
                    message: format!("Failed to parse compose YAML: {}", e),
                    service_ids: Vec::new(),
                    network_names: Vec::new(),
                    volume_names: Vec::new(),
                    failed_services: Vec::new(),
                }));
            }
        };

        let mut created_service_ids = Vec::new();
        let mut created_network_names = Vec::new();
        let mut created_volume_names = Vec::new();
        let mut failed_services = Vec::new();

        // Always create the implicit _default overlay network.
        // Even when explicit top-level networks exist, services that omit
        // `networks:` are attached to `<stack>_default` (matches docker stack
        // deploy / docker-compose behavior).
        {
            let default_net = format!("{}_default", req.stack_name);
            let mut labels = std::collections::HashMap::new();
            labels.insert("com.docker.stack.namespace".to_string(), req.stack_name.clone());
            match self.state.docker.create_network(
                &default_net, Some("overlay"), labels,
                false, false, false,
                std::collections::HashMap::new(), None,
            ).await {
                Ok(_) => created_network_names.push(default_net),
                Err(e) => {
                    let err_str = format!("{}", e);
                    if err_str.contains("409") || err_str.contains("already exists") {
                        info!(network = %default_net, "Default network already exists, reusing");
                        created_network_names.push(default_net);
                    } else {
                        warn!(network = %default_net, "Failed to create default network: {}", e);
                        failed_services.push(format!("network/{}: {}", default_net, e));
                    }
                }
            }
        }

        // Track which network aliases are external → their actual Docker network name
        let mut external_networks: std::collections::HashMap<String, String> = std::collections::HashMap::new();

        // Create networks first
        if let Some(networks) = compose.get("networks").and_then(|n| n.as_mapping()) {
            for (name, config) in networks {
                let raw_name = name.as_str().unwrap_or("default");

                // Check for external: true
                let is_external = config.get("external")
                    .map(|v| {
                        // external: true  OR  external: { name: "foo" }
                        v.as_bool().unwrap_or(false) || v.is_mapping()
                    })
                    .unwrap_or(false);

                if is_external {
                    // External network: use as-is (or the explicit name if provided)
                    let ext_name = config.get("external")
                        .and_then(|v| v.as_mapping())
                        .and_then(|m| m.get(serde_yaml::Value::String("name".into())))
                        .and_then(|v| v.as_str())
                        // Compose v3.5+: top-level `name:` key
                        .or_else(|| config.get("name").and_then(|v| v.as_str()))
                        .unwrap_or(raw_name);
                    external_networks.insert(raw_name.to_string(), ext_name.to_string());
                    // Do NOT push into created_network_names — we did not create
                    // this network; it is an external pre-existing resource.
                    info!(network = %ext_name, alias = %raw_name, "External network — not creating, using as-is");
                    continue;
                }

                let net_name = format!("{}_{}", req.stack_name, raw_name);
                let driver = config.get("driver").and_then(|d| d.as_str()).unwrap_or("overlay");
                let mut labels = std::collections::HashMap::new();
                labels.insert("com.docker.stack.namespace".to_string(), req.stack_name.clone());

                match self.state.docker.create_network(
                    &net_name, Some(driver), labels,
                    false, false, false,
                    std::collections::HashMap::new(), None,
                ).await {
                    Ok(_) => created_network_names.push(net_name),
                    Err(e) => {
                        // If it's a 409 conflict the network already exists — that's fine.
                        // Otherwise treat it as a real failure.
                        let err_str = format!("{}", e);
                        if err_str.contains("409") || err_str.contains("already exists") {
                            info!(network = %net_name, "Network already exists, reusing");
                            created_network_names.push(net_name);
                        } else {
                            warn!(network = %net_name, "Failed to create network: {}", e);
                            failed_services.push(format!("network/{}: {}", net_name, e));
                        }
                    }
                }
            }
        }

        // Track which volume aliases are external → their actual Docker volume name
        let mut external_volumes: std::collections::HashMap<String, String> = std::collections::HashMap::new();

        // Create volumes
        if let Some(volumes) = compose.get("volumes").and_then(|v| v.as_mapping()) {
            for (name, config) in volumes {
                let raw_name = name.as_str().unwrap_or("default");

                // Check for external: true
                let is_external = config.get("external")
                    .map(|v| v.as_bool().unwrap_or(false) || v.is_mapping())
                    .unwrap_or(false);

                if is_external {
                    // External volume: use as-is (or the explicit name if provided)
                    let ext_name = config.get("external")
                        .and_then(|v| v.as_mapping())
                        .and_then(|m| m.get(serde_yaml::Value::String("name".into())))
                        .and_then(|v| v.as_str())
                        // Compose v3.5+: top-level `name:` key
                        .or_else(|| config.get("name").and_then(|v| v.as_str()))
                        .unwrap_or(raw_name);
                    external_volumes.insert(raw_name.to_string(), ext_name.to_string());
                    // Do NOT push into created_volume_names — we did not create
                    // this volume; it is an external pre-existing resource.
                    info!(volume = %ext_name, alias = %raw_name, "External volume — not creating, using as-is");
                    continue;
                }

                let vol_name = format!("{}_{}", req.stack_name, raw_name);
                let driver = config.get("driver").and_then(|d| d.as_str());
                let mut labels = std::collections::HashMap::new();
                labels.insert("com.docker.stack.namespace".to_string(), req.stack_name.clone());

                match self.state.docker.create_volume(&vol_name, driver, labels, std::collections::HashMap::new()).await {
                    Ok(_) => created_volume_names.push(vol_name),
                    Err(e) => {
                        let err_str = format!("{}", e);
                        if err_str.contains("409") || err_str.contains("already exists") {
                            info!(volume = %vol_name, "Volume already exists, reusing");
                            created_volume_names.push(vol_name);
                        } else {
                            warn!(volume = %vol_name, "Failed to create volume: {}", e);
                            failed_services.push(format!("volume/{}: {}", vol_name, e));
                        }
                    }
                }
            }
        }

        // Create services
        if let Some(services) = compose.get("services").and_then(|s| s.as_mapping()) {
            for (name, config) in services {
                let svc_name = format!("{}_{}", req.stack_name, name.as_str().unwrap_or("unnamed"));
                let image = config.get("image").and_then(|i| i.as_str()).unwrap_or("").to_string();
                if image.is_empty() {
                    failed_services.push(format!("{}: no image specified", svc_name));
                    continue;
                }

                // Parse replicas
                let replicas = config.get("deploy")
                    .and_then(|d| d.get("replicas"))
                    .and_then(|r| r.as_u64())
                    .unwrap_or(1);

                // Parse environment variables
                let mut env_vec = Vec::new();
                if let Some(env) = config.get("environment") {
                    if let Some(seq) = env.as_sequence() {
                        for item in seq {
                            if let Some(s) = item.as_str() {
                                env_vec.push(s.to_string());
                            }
                        }
                    } else if let Some(map) = env.as_mapping() {
                        for (k, v) in map {
                            if let Some(key) = k.as_str() {
                                // Support string, numeric, and boolean env values
                                let val = if let Some(s) = v.as_str() {
                                    s.to_string()
                                } else if let Some(b) = v.as_bool() {
                                    b.to_string()
                                } else if let Some(i) = v.as_i64() {
                                    i.to_string()
                                } else if let Some(f) = v.as_f64() {
                                    f.to_string()
                                } else if v.is_null() {
                                    // KEY with null value → KEY= (empty)
                                    String::new()
                                } else {
                                    continue;
                                };
                                env_vec.push(format!("{}={}", key, val));
                            }
                        }
                    }
                }

                // Parse ports (short string syntax and long/object syntax)
                let mut port_configs = Vec::new();
                if let Some(ports) = config.get("ports").and_then(|p| p.as_sequence()) {
                    for port in ports {
                        if let Some(port_str) = port.as_str() {
                            // Short syntax: "80", "8080:80", "127.0.0.1:8080:80",
                            // "8080:80/udp", "127.0.0.1:8080:80/udp"
                            let (main, protocol) = if let Some(idx) = port_str.rfind('/') {
                                (&port_str[..idx], &port_str[idx+1..])
                            } else {
                                (port_str, "tcp")
                            };
                            let parts: Vec<&str> = main.split(':').collect();
                            let (published, target) = match parts.len() {
                                1 => {
                                    // "80" — target only, no published port
                                    (0i64, parts[0].parse::<i64>().unwrap_or(0))
                                }
                                2 => {
                                    // "8080:80" — published:target
                                    (parts[0].parse::<i64>().unwrap_or(0), parts[1].parse::<i64>().unwrap_or(0))
                                }
                                3 => {
                                    // "127.0.0.1:8080:80" — host_ip:published:target
                                    // parts[0] is the IP (ignored for swarm ingress)
                                    (parts[1].parse::<i64>().unwrap_or(0), parts[2].parse::<i64>().unwrap_or(0))
                                }
                                _ => (0, 0),
                            };
                            if target > 0 {
                                port_configs.push(bollard::models::EndpointPortConfig {
                                    target_port: Some(target),
                                    published_port: if published > 0 { Some(published) } else { None },
                                    protocol: Some(match protocol {
                                        "udp" => bollard::models::EndpointPortConfigProtocolEnum::UDP,
                                        _ => bollard::models::EndpointPortConfigProtocolEnum::TCP,
                                    }),
                                    publish_mode: Some(bollard::models::EndpointPortConfigPublishModeEnum::INGRESS),
                                    ..Default::default()
                                });
                            }
                        } else if let Some(port_map) = port.as_mapping() {
                            // Long/object syntax: { target: 80, published: 8080, protocol: udp, mode: host }
                            let target = port_map.get(serde_yaml::Value::String("target".into()))
                                .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
                                .unwrap_or(0) as i64;
                            let published = port_map.get(serde_yaml::Value::String("published".into()))
                                .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
                                .unwrap_or(0) as i64;
                            let protocol = port_map.get(serde_yaml::Value::String("protocol".into()))
                                .and_then(|v| v.as_str())
                                .unwrap_or("tcp");
                            let mode = port_map.get(serde_yaml::Value::String("mode".into()))
                                .and_then(|v| v.as_str())
                                .unwrap_or("ingress");
                            if target > 0 {
                                port_configs.push(bollard::models::EndpointPortConfig {
                                    target_port: Some(target),
                                    published_port: if published > 0 { Some(published) } else { None },
                                    protocol: Some(match protocol {
                                        "udp" => bollard::models::EndpointPortConfigProtocolEnum::UDP,
                                        _ => bollard::models::EndpointPortConfigProtocolEnum::TCP,
                                    }),
                                    publish_mode: Some(match mode {
                                        "host" => bollard::models::EndpointPortConfigPublishModeEnum::HOST,
                                        _ => bollard::models::EndpointPortConfigPublishModeEnum::INGRESS,
                                    }),
                                    ..Default::default()
                                });
                            }
                        }
                    }
                }

                // Parse networks (sequence or mapping form)
                // External networks are not stack-prefixed.
                let networks: Option<Vec<bollard::models::NetworkAttachmentConfig>> = if let Some(net_val) = config.get("networks") {
                    if let Some(seq) = net_val.as_sequence() {
                        // Sequence form: networks: ["frontend", "backend"]
                        Some(seq.iter().filter_map(|n| n.as_str()).map(|n| {
                            let net_name = if let Some(ext) = external_networks.get(n) {
                                ext.clone()
                            } else {
                                format!("{}_{}", req.stack_name, n)
                            };
                            bollard::models::NetworkAttachmentConfig {
                                target: Some(net_name),
                                ..Default::default()
                            }
                        }).collect())
                    } else if let Some(map) = net_val.as_mapping() {
                        // Mapping form: networks: { frontend: { aliases: [...] } }
                        Some(map.keys().filter_map(|k| k.as_str()).map(|n| {
                            let net_name = if let Some(ext) = external_networks.get(n) {
                                ext.clone()
                            } else {
                                format!("{}_{}", req.stack_name, n)
                            };
                            bollard::models::NetworkAttachmentConfig {
                                target: Some(net_name),
                                ..Default::default()
                            }
                        }).collect())
                    } else {
                        None
                    }
                } else {
                    // No networks specified — attach to the implicit default network
                    Some(vec![bollard::models::NetworkAttachmentConfig {
                        target: Some(format!("{}_default", req.stack_name)),
                        ..Default::default()
                    }])
                };

                // Parse command
                let command = config.get("command").and_then(|c| {
                    if let Some(s) = c.as_str() {
                        Some(vec!["/bin/sh".to_string(), "-c".to_string(), s.to_string()])
                    } else if let Some(seq) = c.as_sequence() {
                        Some(seq.iter().filter_map(|i| i.as_str().map(|s| s.to_string())).collect())
                    } else {
                        None
                    }
                });

                // Stack labels
                let mut labels = std::collections::HashMap::new();
                labels.insert("com.docker.stack.namespace".to_string(), req.stack_name.clone());
                labels.insert("com.docker.stack.image".to_string(), image.clone());

                // Parse volumes / mounts (short and long/object syntax)
                let mut mounts: Vec<bollard::models::Mount> = Vec::new();
                if let Some(volumes) = config.get("volumes").and_then(|v| v.as_sequence()) {
                    for vol in volumes {
                        if let Some(vol_str) = vol.as_str() {
                            // Short syntax: "volume_name:/container/path[:ro]"
                            //               "/host/path:/container/path[:ro]"
                            //               "/container/path"  (anonymous volume)
                            let (main, read_only) = if vol_str.ends_with(":ro") {
                                (&vol_str[..vol_str.len()-3], true)
                            } else if vol_str.ends_with(":rw") {
                                (&vol_str[..vol_str.len()-3], false)
                            } else {
                                (vol_str, false)
                            };
                            let parts: Vec<&str> = main.splitn(2, ':').collect();
                            if parts.len() == 2 {
                                let source_raw = parts[0];
                                let target = parts[1].to_string();
                                // Determine mount type: absolute/relative path = bind, otherwise = volume
                                let (typ, source) = if source_raw.starts_with('/') || source_raw.starts_with('.') {
                                    (bollard::models::MountTypeEnum::BIND, source_raw.to_string())
                                } else if let Some(ext) = external_volumes.get(source_raw) {
                                    // External volume — use the real name without stack prefix
                                    (bollard::models::MountTypeEnum::VOLUME, ext.clone())
                                } else {
                                    // Named volume — stack-prefix it to match created volumes
                                    (bollard::models::MountTypeEnum::VOLUME, format!("{}_{}", req.stack_name, source_raw))
                                };
                                mounts.push(bollard::models::Mount {
                                    target: Some(target),
                                    source: Some(source),
                                    typ: Some(typ),
                                    read_only: Some(read_only),
                                    ..Default::default()
                                });
                            } else {
                                // Single path — anonymous volume
                                mounts.push(bollard::models::Mount {
                                    target: Some(parts[0].to_string()),
                                    typ: Some(bollard::models::MountTypeEnum::VOLUME),
                                    ..Default::default()
                                });
                            }
                        } else if let Some(vol_map) = vol.as_mapping() {
                            // Long/object syntax:
                            // - type: volume|bind|tmpfs
                            //   source: my_volume
                            //   target: /container/path
                            //   read_only: true
                            let mount_type_str = vol_map.get(serde_yaml::Value::String("type".into()))
                                .and_then(|v| v.as_str())
                                .unwrap_or("volume");
                            let source_raw = vol_map.get(serde_yaml::Value::String("source".into()))
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            let target = vol_map.get(serde_yaml::Value::String("target".into()))
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            let read_only = vol_map.get(serde_yaml::Value::String("read_only".into()))
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);

                            if !target.is_empty() {
                                let typ = match mount_type_str {
                                    "bind" => bollard::models::MountTypeEnum::BIND,
                                    "tmpfs" => bollard::models::MountTypeEnum::TMPFS,
                                    _ => bollard::models::MountTypeEnum::VOLUME,
                                };
                                // Stack-prefix named volume sources (not bind/tmpfs), unless external
                                let source = if mount_type_str == "volume" && !source_raw.is_empty() && !source_raw.starts_with('/') {
                                    if let Some(ext) = external_volumes.get(source_raw) {
                                        ext.clone()
                                    } else {
                                        format!("{}_{}", req.stack_name, source_raw)
                                    }
                                } else {
                                    source_raw.to_string()
                                };
                                mounts.push(bollard::models::Mount {
                                    target: Some(target.to_string()),
                                    source: if source.is_empty() { None } else { Some(source) },
                                    typ: Some(typ),
                                    read_only: Some(read_only),
                                    ..Default::default()
                                });
                            }
                        }
                    }
                }

                let spec = bollard::models::ServiceSpec {
                    name: Some(svc_name.clone()),
                    mode: Some(bollard::models::ServiceSpecMode {
                        replicated: Some(bollard::models::ServiceSpecModeReplicated {
                            replicas: Some(replicas as i64),
                        }),
                        ..Default::default()
                    }),
                    task_template: Some(bollard::models::TaskSpec {
                        container_spec: Some(bollard::models::TaskSpecContainerSpec {
                            image: Some(image),
                            env: if env_vec.is_empty() { None } else { Some(env_vec) },
                            command,
                            mounts: if mounts.is_empty() { None } else { Some(mounts) },
                            ..Default::default()
                        }),
                        networks,
                        ..Default::default()
                    }),
                    labels: Some(labels),
                    endpoint_spec: if port_configs.is_empty() {
                        None
                    } else {
                        Some(bollard::models::EndpointSpec {
                            ports: Some(port_configs),
                            ..Default::default()
                        })
                    },
                    ..Default::default()
                };

                match self.state.docker.create_service(spec, None).await {
                    Ok(id) => {
                        info!(service = %svc_name, id = %id, "Compose service created");
                        created_service_ids.push(id);
                    }
                    Err(e) => {
                        warn!(service = %svc_name, "Failed to create compose service: {}", e);
                        failed_services.push(format!("{}: {}", svc_name, e));
                    }
                }
            }
        }

        // Store the compose YAML for later retrieval (B12)
        {
            let mut store = self.state.stack_files.lock().await;
            store.insert(req.stack_name.clone(), req.compose_yaml.clone());
        }

        let all_ok = failed_services.is_empty();
        Ok(Response::new(DeployComposeStackResponse {
            success: all_ok,
            message: if all_ok {
                format!("Stack '{}' deployed: {} services, {} networks, {} volumes",
                    req.stack_name, created_service_ids.len(), created_network_names.len(), created_volume_names.len())
            } else {
                format!("Stack '{}' partially deployed: {} failed", req.stack_name, failed_services.len())
            },
            service_ids: created_service_ids,
            network_names: created_network_names,
            volume_names: created_volume_names,
            failed_services,
        }))
    }

    // =========================================================================
    // B12: Stack File Viewer
    // =========================================================================

    async fn get_stack_file(
        &self,
        request: Request<GetStackFileRequest>,
    ) -> Result<Response<GetStackFileResponse>, Status> {
        let stack_name = request.into_inner().stack_name;
        debug!(stack_name = %stack_name, "Getting stack file");

        let store = self.state.stack_files.lock().await;
        match store.get(&stack_name) {
            Some(yaml) => Ok(Response::new(GetStackFileResponse {
                found: true,
                compose_yaml: yaml.clone(),
                stack_name,
            })),
            None => Ok(Response::new(GetStackFileResponse {
                found: false,
                compose_yaml: String::new(),
                stack_name,
            })),
        }
    }
}

/// Convert a bollard Service to our proto ServiceInfo
fn convert_service_to_proto(
    s: &bollard::models::Service,
    tasks: &[bollard::models::Task],
) -> ServiceInfo {
    let spec = s.spec.as_ref();
    let task_template = spec.and_then(|sp| sp.task_template.as_ref());

    // Determine service mode
    let mode_spec = spec.and_then(|sp| sp.mode.as_ref());
    let (mode, replicas_desired) = if let Some(mode) = mode_spec {
        if let Some(replicated) = &mode.replicated {
            (ServiceMode::Replicated as i32, replicated.replicas.unwrap_or(1) as u64)
        } else if mode.global.is_some() {
            (ServiceMode::Global as i32, 0u64)
        } else if let Some(replicated_job) = &mode.replicated_job {
            (ServiceMode::ReplicatedJob as i32, replicated_job.max_concurrent.unwrap_or(1) as u64)
        } else if mode.global_job.is_some() {
            (ServiceMode::GlobalJob as i32, 0u64)
        } else {
            (ServiceMode::Unknown as i32, 0u64)
        }
    } else {
        (ServiceMode::Unknown as i32, 0u64)
    };

    // Count running replicas from tasks
    let service_id = s.id.as_deref().unwrap_or("");
    let replicas_running = tasks.iter()
        .filter(|t| {
            t.service_id.as_deref() == Some(service_id) &&
            t.status.as_ref()
                .and_then(|s| s.state.as_ref())
                .map(|s| matches!(s, bollard::models::TaskState::RUNNING))
                .unwrap_or(false)
        })
        .count() as u64;

    // Image
    let image = task_template
        .and_then(|tt| tt.container_spec.as_ref())
        .and_then(|cs| cs.image.clone())
        .unwrap_or_default();

    // Labels
    let labels = spec
        .and_then(|sp| sp.labels.as_ref())
        .cloned()
        .unwrap_or_default();

    // Stack namespace from label
    let stack_namespace = labels.get("com.docker.stack.namespace").cloned();

    // Ports
    let ports: Vec<ServicePort> = s.endpoint.as_ref()
        .and_then(|ep| ep.ports.as_ref())
        .map(|ports| {
            ports.iter().map(|p| ServicePort {
                protocol: p.protocol.as_ref()
                    .map(|pr| format!("{:?}", pr).to_lowercase())
                    .unwrap_or_else(|| "tcp".to_string()),
                target_port: p.target_port.unwrap_or(0) as u32,
                published_port: p.published_port.unwrap_or(0) as u32,
                publish_mode: p.publish_mode.as_ref()
                    .map(|pm| format!("{:?}", pm).to_lowercase())
                    .unwrap_or_else(|| "ingress".to_string()),
            }).collect()
        })
        .unwrap_or_default();

    // Update status
    let update_status = s.update_status.as_ref().map(|us| UpdateStatus {
        state: us.state.as_ref()
            .map(|s| format!("{:?}", s).to_lowercase())
            .unwrap_or_default(),
        started_at: us.started_at.as_ref()
            .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
            .map(|dt| dt.timestamp()),
        completed_at: us.completed_at.as_ref()
            .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
            .map(|dt| dt.timestamp()),
        message: us.message.clone().unwrap_or_default(),
    });

    // Placement constraints
    let placement_constraints = task_template
        .and_then(|tt| tt.placement.as_ref())
        .and_then(|p| p.constraints.as_ref())
        .cloned()
        .unwrap_or_default();

    // Networks
    let networks: Vec<String> = task_template
        .and_then(|tt| tt.networks.as_ref())
        .map(|nets| {
            nets.iter()
                .filter_map(|n| n.target.clone())
                .collect()
        })
        .unwrap_or_default();

    // Timestamps
    let created_at = s.created_at.as_ref()
        .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
        .map(|dt| dt.timestamp())
        .unwrap_or(0);

    let updated_at = s.updated_at.as_ref()
        .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
        .map(|dt| dt.timestamp())
        .unwrap_or(0);

    // S5: Virtual IPs from endpoint
    let virtual_ips: Vec<VirtualIp> = s.endpoint.as_ref()
        .and_then(|ep| ep.virtual_ips.as_ref())
        .map(|vips| {
            vips.iter().map(|vip| VirtualIp {
                network_id: vip.network_id.clone().unwrap_or_default(),
                addr: vip.addr.clone().unwrap_or_default(),
            }).collect()
        })
        .unwrap_or_default();

    // S6: Update config
    let update_config = spec.and_then(|sp| sp.update_config.as_ref()).map(|uc| UpdateConfig {
        parallelism: uc.parallelism.unwrap_or(1) as u64,
        delay_ns: uc.delay.unwrap_or(0),
        failure_action: uc.failure_action.as_ref()
            .map(|fa| format!("{}", fa))
            .unwrap_or_else(|| "pause".to_string()),
        monitor_ns: uc.monitor.unwrap_or(0),
        max_failure_ratio: uc.max_failure_ratio.unwrap_or(0.0),
        order: uc.order.as_ref()
            .map(|o| format!("{}", o))
            .unwrap_or_else(|| "stop-first".to_string()),
    });

    // S6: Rollback config (same shape as update config)
    let rollback_config = spec.and_then(|sp| sp.rollback_config.as_ref()).map(|rc| UpdateConfig {
        parallelism: rc.parallelism.unwrap_or(1) as u64,
        delay_ns: rc.delay.unwrap_or(0),
        failure_action: rc.failure_action.as_ref()
            .map(|fa| format!("{}", fa))
            .unwrap_or_else(|| "pause".to_string()),
        monitor_ns: rc.monitor.unwrap_or(0),
        max_failure_ratio: rc.max_failure_ratio.unwrap_or(0.0),
        order: rc.order.as_ref()
            .map(|o| format!("{}", o))
            .unwrap_or_else(|| "stop-first".to_string()),
    });

    // S6: Placement (full detail, not just constraints)
    let placement = task_template.and_then(|tt| tt.placement.as_ref()).map(|p| {
        let constraints = p.constraints.clone().unwrap_or_default();
        let preferences = p.preferences.as_ref()
            .map(|prefs| {
                prefs.iter().map(|pref| PlacementPreference {
                    spread_descriptor: pref.spread.as_ref()
                        .and_then(|sp| sp.spread_descriptor.clone())
                        .unwrap_or_default(),
                }).collect()
            })
            .unwrap_or_default();
        let max_replicas_per_node = p.max_replicas.map(|m| m as u64);
        let platforms = p.platforms.as_ref()
            .map(|ps| {
                ps.iter().map(|plat| Platform {
                    architecture: plat.architecture.clone().unwrap_or_default(),
                    os: plat.os.clone().unwrap_or_default(),
                }).collect()
            })
            .unwrap_or_default();

        ServicePlacement {
            constraints,
            preferences,
            max_replicas_per_node,
            platforms,
        }
    });

    // S8: Secret references from container spec
    let secret_references: Vec<SecretReferenceInfo> = task_template
        .and_then(|tt| tt.container_spec.as_ref())
        .and_then(|cs| cs.secrets.as_ref())
        .map(|secrets| {
            secrets.iter().map(|sr| {
                let file = sr.file.as_ref();
                SecretReferenceInfo {
                    secret_id: sr.secret_id.clone().unwrap_or_default(),
                    secret_name: sr.secret_name.clone().unwrap_or_default(),
                    file_name: file.and_then(|f| f.name.clone()).unwrap_or_default(),
                    file_uid: file.and_then(|f| f.uid.clone()).unwrap_or_default(),
                    file_gid: file.and_then(|f| f.gid.clone()).unwrap_or_default(),
                    file_mode: file.and_then(|f| f.mode).unwrap_or(0o444),
                }
            }).collect()
        })
        .unwrap_or_default();

    // S8: Config references from container spec
    let config_references: Vec<ConfigReferenceInfo> = task_template
        .and_then(|tt| tt.container_spec.as_ref())
        .and_then(|cs| cs.configs.as_ref())
        .map(|configs| {
            configs.iter().map(|cr| {
                let file = cr.file.as_ref();
                ConfigReferenceInfo {
                    config_id: cr.config_id.clone().unwrap_or_default(),
                    config_name: cr.config_name.clone().unwrap_or_default(),
                    file_name: file.and_then(|f| f.name.clone()).unwrap_or_default(),
                    file_uid: file.and_then(|f| f.uid.clone()).unwrap_or_default(),
                    file_gid: file.and_then(|f| f.gid.clone()).unwrap_or_default(),
                    file_mode: file.and_then(|f| f.mode).unwrap_or(0o444),
                }
            }).collect()
        })
        .unwrap_or_default();

    // S11: Restart policy from task template
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

    ServiceInfo {
        id: s.id.clone().unwrap_or_default(),
        name: spec.and_then(|sp| sp.name.clone()).unwrap_or_default(),
        image,
        mode,
        replicas_desired,
        replicas_running,
        labels,
        stack_namespace,
        created_at,
        updated_at,
        ports,
        update_status,
        placement_constraints,
        networks,
        virtual_ips,
        update_config,
        rollback_config,
        placement,
        secret_references,
        config_references,
        restart_policy,
    }
}

/// Convert a bollard Network to our proto SwarmNetworkInfo
fn convert_network_to_proto(
    net: &bollard::models::Network,
    services: &[bollard::models::Service],
) -> SwarmNetworkInfo {
    let net_id = net.id.as_deref().unwrap_or("");

    // IPAM configs
    let ipam_configs: Vec<IpamConfigEntry> = net.ipam.as_ref()
        .and_then(|ipam| ipam.config.as_ref())
        .map(|configs| {
            configs.iter().map(|c| IpamConfigEntry {
                subnet: c.subnet.clone(),
                gateway: c.gateway.clone(),
                ip_range: c.ip_range.clone(),
            }).collect()
        })
        .unwrap_or_default();

    // Peers
    let peers: Vec<NetworkPeerInfo> = net.peers.as_ref()
        .map(|ps| {
            ps.iter().map(|p| NetworkPeerInfo {
                name: p.name.clone().unwrap_or_default(),
                ip: p.ip.clone().unwrap_or_default(),
            }).collect()
        })
        .unwrap_or_default();

    // Labels
    let labels = net.labels.as_ref().cloned().unwrap_or_default();

    // Options
    let options = net.options.as_ref().cloned().unwrap_or_default();

    // Service attachments: find services whose endpoint VIPs reference this network
    let service_attachments: Vec<NetworkServiceAttachment> = services.iter()
        .filter_map(|svc| {
            let svc_id = svc.id.as_deref().unwrap_or("");
            let svc_name = svc.spec.as_ref()
                .and_then(|sp| sp.name.as_deref())
                .unwrap_or("");

            let vip = svc.endpoint.as_ref()
                .and_then(|ep| ep.virtual_ips.as_ref())
                .and_then(|vips| {
                    vips.iter().find(|v| {
                        v.network_id.as_deref() == Some(net_id)
                    })
                });

            vip.map(|v| NetworkServiceAttachment {
                service_id: svc_id.to_string(),
                service_name: svc_name.to_string(),
                virtual_ip: v.addr.clone().unwrap_or_default(),
            })
        })
        .collect();

    // Timestamps
    let created_at = net.created.as_ref()
        .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
        .map(|dt| dt.timestamp())
        .unwrap_or(0);

    SwarmNetworkInfo {
        id: net_id.to_string(),
        name: net.name.clone().unwrap_or_default(),
        driver: net.driver.clone().unwrap_or_default(),
        scope: net.scope.clone().unwrap_or_default(),
        is_internal: net.internal.unwrap_or(false),
        is_attachable: net.attachable.unwrap_or(false),
        is_ingress: net.ingress.unwrap_or(false),
        enable_ipv6: net.enable_ipv6.unwrap_or(false),
        created_at,
        labels,
        options,
        ipam_configs,
        peers,
        service_attachments,
    }
}

/// Convert a bollard Node to our proto NodeInfo (S9 helper, also used by list_nodes)
fn convert_node_to_proto(n: &bollard::models::Node) -> NodeInfo {
    let spec = n.spec.as_ref();
    let description = n.description.as_ref();
    let status = n.status.as_ref();
    let manager_status = n.manager_status.as_ref();

    let role = spec
        .and_then(|s| s.role.as_ref())
        .map(|r| match r {
            bollard::models::NodeSpecRoleEnum::MANAGER => NodeRole::Manager as i32,
            bollard::models::NodeSpecRoleEnum::WORKER => NodeRole::Worker as i32,
            _ => NodeRole::Unknown as i32,
        })
        .unwrap_or(NodeRole::Unknown as i32);

    let availability = spec
        .and_then(|s| s.availability.as_ref())
        .map(|a| match a {
            bollard::models::NodeSpecAvailabilityEnum::ACTIVE => NodeAvailability::Active as i32,
            bollard::models::NodeSpecAvailabilityEnum::PAUSE => NodeAvailability::Pause as i32,
            bollard::models::NodeSpecAvailabilityEnum::DRAIN => NodeAvailability::Drain as i32,
            _ => NodeAvailability::Unknown as i32,
        })
        .unwrap_or(NodeAvailability::Unknown as i32);

    let node_state = status
        .and_then(|s| s.state.as_ref())
        .map(|s| match s {
            bollard::models::NodeState::READY => NodeState::Ready as i32,
            bollard::models::NodeState::DOWN => NodeState::Down as i32,
            bollard::models::NodeState::DISCONNECTED => NodeState::Disconnected as i32,
            _ => NodeState::Unknown as i32,
        })
        .unwrap_or(NodeState::Unknown as i32);

    let platform = description.and_then(|d| d.platform.as_ref());
    let engine = description.and_then(|d| d.engine.as_ref());
    let resources = description.and_then(|d| d.resources.as_ref());

    let mgr_status = manager_status.map(|ms| ManagerStatus {
        leader: ms.leader.unwrap_or(false),
        reachability: ms.reachability.as_ref()
            .map(|r| format!("{:?}", r).to_lowercase())
            .unwrap_or_default(),
        addr: ms.addr.clone().unwrap_or_default(),
    });

    let labels = spec
        .and_then(|s| s.labels.as_ref())
        .cloned()
        .unwrap_or_default();

    NodeInfo {
        id: n.id.clone().unwrap_or_default(),
        hostname: description
            .and_then(|d| d.hostname.clone())
            .unwrap_or_default(),
        role,
        availability,
        status: node_state,
        addr: status
            .and_then(|s| s.addr.clone())
            .unwrap_or_default(),
        engine_version: engine
            .and_then(|e| e.engine_version.clone())
            .unwrap_or_default(),
        os: platform
            .and_then(|p| p.os.clone())
            .unwrap_or_default(),
        architecture: platform
            .and_then(|p| p.architecture.clone())
            .unwrap_or_default(),
        labels,
        manager_status: mgr_status,
        nano_cpus: resources
            .and_then(|r| r.nano_cpus)
            .unwrap_or(0),
        memory_bytes: resources
            .and_then(|r| r.memory_bytes)
            .unwrap_or(0),
    }
}

/// Convert a bollard Task to our proto TaskInfo (S9 helper for affected tasks in drain events)
fn convert_task_to_proto(t: &bollard::models::Task) -> TaskInfo {
    convert_task_to_proto_with_name(t, "")
}

/// Convert a bollard Task to our proto TaskInfo with an optional service name
fn convert_task_to_proto_with_name(t: &bollard::models::Task, service_name: &str) -> TaskInfo {
    let status = t.status.as_ref();
    let container_status = status.and_then(|s| s.container_status.as_ref());

    TaskInfo {
        id: t.id.clone().unwrap_or_default(),
        service_id: t.service_id.clone().unwrap_or_default(),
        node_id: t.node_id.clone().unwrap_or_default(),
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
        service_name: if service_name.is_empty() {
            // Fallback: try to find service name from labels (Docker adds it as com.docker.swarm.service.name)
            t.spec.as_ref()
                .and_then(|s| s.container_spec.as_ref())
                .and_then(|cs| cs.labels.as_ref())
                .and_then(|l| l.get("com.docker.swarm.service.name"))
                .cloned()
                .unwrap_or_default()
        } else {
            service_name.to_string()
        },
    }
}
