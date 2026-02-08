//! Route — SwarmService gRPC handler (thin delegation layer).
//!
//! Each method delegates to its domain sub-module.
//! Business logic lives in `info`, `node`, `service`, `task`, `event`,
//! `secret`, `config`, `health`, `update`, `compose`.

use tonic::{Request, Response, Status};
use tracing::{debug, info};
use std::pin::Pin;
use futures_util::stream::Stream;

use crate::state::SharedState;

use crate::proto::{
    swarm_service_server::SwarmService,
    SwarmInfoRequest, SwarmInfoResponse,
    NodeListRequest, NodeListResponse,
    ServiceListRequest, ServiceListResponse,
    ServiceInspectRequest, ServiceInspectResponse,
    TaskListRequest, TaskListResponse,
    ServiceLogStreamRequest, NormalizedLogEntry,
    CreateServiceRequest, CreateServiceResponse,
    DeleteServiceRequest, DeleteServiceResponse,
    UpdateServiceRequest, UpdateServiceResponse,
    SwarmNetworkListRequest, SwarmNetworkListResponse,
    SwarmNetworkInspectRequest, SwarmNetworkInspectResponse,
    ServiceUpdateStreamRequest, ServiceUpdateEvent,
    SecretListRequest, SecretListResponse,
    ConfigListRequest, ConfigListResponse,
    NodeInspectRequest, NodeInspectResponse,
    NodeUpdateRequest, NodeUpdateResponse,
    NodeEventStreamRequest, NodeEvent,
    ServiceEventStreamRequest, ServiceEvent,
    ServiceCoverageRequest, ServiceCoverageResponse,
    StackHealthRequest, StackHealthResponse,
    ServiceRestartEventStreamRequest, ServiceRestartEvent,
    RollbackServiceRequest, RollbackServiceResponse,
    CreateSecretRequest, CreateSecretResponse,
    DeleteSecretRequest, DeleteSecretResponse,
    CreateConfigRequest, CreateConfigResponse,
    DeleteConfigRequest, DeleteConfigResponse,
    SwarmInitRequest, SwarmInitResponse,
    SwarmJoinRequest, SwarmJoinResponse,
    SwarmLeaveRequest, SwarmLeaveResponse,
    RemoveNodeRequest, RemoveNodeResponse,
    NetworkConnectRequest, NetworkConnectResponse,
    NetworkDisconnectRequest, NetworkDisconnectResponse,
    TaskLogStreamRequest,
    TaskInspectRequest, TaskInspectResponse,
    SwarmUpdateRequest, SwarmUpdateResponse,
    SwarmUnlockKeyRequest, SwarmUnlockKeyResponse,
    SwarmUnlockRequest, SwarmUnlockResponse,
    DeployComposeStackRequest, DeployComposeStackResponse,
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
    type StreamServiceLogsStream = Pin<Box<dyn Stream<Item = Result<NormalizedLogEntry, Status>> + Send>>;
    type ServiceUpdateStreamStream = Pin<Box<dyn Stream<Item = Result<ServiceUpdateEvent, Status>> + Send>>;
    type NodeEventStreamStream = Pin<Box<dyn Stream<Item = Result<NodeEvent, Status>> + Send>>;
    type ServiceEventStreamStream = Pin<Box<dyn Stream<Item = Result<ServiceEvent, Status>> + Send>>;
    type ServiceRestartEventStreamStream = Pin<Box<dyn Stream<Item = Result<ServiceRestartEvent, Status>> + Send>>;
    type StreamTaskLogsStream = Pin<Box<dyn Stream<Item = Result<NormalizedLogEntry, Status>> + Send>>;

    // ── Swarm Info ────────────────────────────────────────────────

    async fn get_swarm_info(
        &self, _request: Request<SwarmInfoRequest>,
    ) -> Result<Response<SwarmInfoResponse>, Status> {
        debug!("Getting swarm info");
        crate::swarm::info::get_info(&self.state.docker).await
            .map(Response::new)
            .map_err(|msg| Status::internal(msg))
    }

    // ── Node CRUD ─────────────────────────────────────────────────

    async fn list_nodes(
        &self, _request: Request<NodeListRequest>,
    ) -> Result<Response<NodeListResponse>, Status> {
        debug!("Listing swarm nodes");
        crate::swarm::node::list(&self.state.docker).await.map(Response::new)
    }

    async fn inspect_node(
        &self, request: Request<NodeInspectRequest>,
    ) -> Result<Response<NodeInspectResponse>, Status> {
        let node_id = request.into_inner().node_id;
        debug!("Inspecting node {}", node_id);
        crate::swarm::node::inspect(&self.state.docker, &node_id).await.map(Response::new)
    }

    async fn update_node(
        &self, request: Request<NodeUpdateRequest>,
    ) -> Result<Response<NodeUpdateResponse>, Status> {
        let req = request.into_inner();
        info!("Updating node {} — availability={:?}, role={:?}", req.node_id, req.availability, req.role);
        crate::swarm::node::update(
            &self.state.docker, &req.node_id,
            req.availability, req.role, req.labels,
        ).await.map(Response::new)
    }

    async fn remove_node(
        &self, request: Request<RemoveNodeRequest>,
    ) -> Result<Response<RemoveNodeResponse>, Status> {
        let req = request.into_inner();
        info!(node_id = %req.node_id, force = req.force, "Removing node from swarm");
        crate::swarm::node::remove(&self.state.docker, &req.node_id, req.force).await.map(Response::new)
    }

    async fn node_event_stream(
        &self, request: Request<NodeEventStreamRequest>,
    ) -> Result<Response<Self::NodeEventStreamStream>, Status> {
        let req = request.into_inner();
        let filter = if req.node_id.is_empty() { None } else { Some(req.node_id) };
        let poll_ms = if req.poll_interval_ms == 0 { 2000 } else { req.poll_interval_ms };
        info!("Starting node event stream (filter={:?}, poll={}ms)", filter, poll_ms);
        Ok(Response::new(crate::swarm::node::event_stream(
            self.state.docker.clone(), filter, poll_ms,
        )))
    }

    // ── Service CRUD ──────────────────────────────────────────────

    async fn list_services(
        &self, _request: Request<ServiceListRequest>,
    ) -> Result<Response<ServiceListResponse>, Status> {
        debug!("Listing swarm services");
        crate::swarm::service::list_all(&self.state.docker).await.map(Response::new)
    }

    async fn inspect_service(
        &self, request: Request<ServiceInspectRequest>,
    ) -> Result<Response<ServiceInspectResponse>, Status> {
        let service_id = request.into_inner().service_id;
        debug!(service_id = %service_id, "Inspecting swarm service");
        crate::swarm::service::inspect_full(&self.state.docker, &service_id).await.map(Response::new)
    }

    async fn create_service(
        &self, request: Request<CreateServiceRequest>,
    ) -> Result<Response<CreateServiceResponse>, Status> {
        let req = request.into_inner();
        info!(name = %req.name, image = %req.image, "Creating swarm service");
        crate::swarm::service::create(&self.state.docker, req).await.map(Response::new)
    }

    async fn delete_service(
        &self, request: Request<DeleteServiceRequest>,
    ) -> Result<Response<DeleteServiceResponse>, Status> {
        let service_id = request.into_inner().service_id;
        info!(service_id = %service_id, "Deleting swarm service");
        crate::swarm::service::delete(&self.state.docker, &service_id).await.map(Response::new)
    }

    async fn update_service(
        &self, request: Request<UpdateServiceRequest>,
    ) -> Result<Response<UpdateServiceResponse>, Status> {
        let req = request.into_inner();
        info!(service_id = %req.service_id, "Updating swarm service");
        crate::swarm::service::update_existing(&self.state.docker, req).await.map(Response::new)
    }

    async fn rollback_service(
        &self, request: Request<RollbackServiceRequest>,
    ) -> Result<Response<RollbackServiceResponse>, Status> {
        let req = request.into_inner();
        info!(service_id = %req.service_id, "Rolling back service");
        crate::swarm::service::rollback(&self.state.docker, &req.service_id).await.map(Response::new)
    }

    // ── Tasks ─────────────────────────────────────────────────────

    async fn list_tasks(
        &self, request: Request<TaskListRequest>,
    ) -> Result<Response<TaskListResponse>, Status> {
        let req = request.into_inner();
        debug!(service_filter = ?req.service_id, "Listing tasks");
        let task_infos = crate::swarm::task::list(&self.state.docker, req.service_id).await?;
        Ok(Response::new(TaskListResponse { tasks: task_infos }))
    }

    async fn inspect_task(
        &self, request: Request<TaskInspectRequest>,
    ) -> Result<Response<TaskInspectResponse>, Status> {
        let task_id = request.into_inner().task_id;
        debug!(task_id = %task_id, "Inspecting task");
        let info = crate::swarm::task::inspect(&self.state.docker, &task_id).await?;
        Ok(Response::new(TaskInspectResponse { task: info }))
    }

    // ── Log Streaming ─────────────────────────────────────────────

    async fn stream_service_logs(
        &self, request: Request<ServiceLogStreamRequest>,
    ) -> Result<Response<Self::StreamServiceLogsStream>, Status> {
        let req = request.into_inner();
        debug!(service_id = %req.service_id, "Streaming service logs");
        let stream = crate::swarm::task::stream_service_logs(
            &self.state.docker, &req.service_id, req.follow,
            req.tail_lines.map(|t| t as u64),
            req.since.unwrap_or(0), req.until.unwrap_or(0), req.timestamps,
        ).await?;
        Ok(Response::new(stream))
    }

    async fn stream_task_logs(
        &self, request: Request<TaskLogStreamRequest>,
    ) -> Result<Response<Self::StreamTaskLogsStream>, Status> {
        let req = request.into_inner();
        info!(task_id = %req.task_id, "Streaming task logs");
        let stream = crate::swarm::task::stream_task_logs(
            self.state.docker.clone(), &req.task_id, req.follow,
            req.tail_lines.map(|t| t as u64),
            req.since.unwrap_or(0), req.until.unwrap_or(0), req.timestamps,
        ).await;
        Ok(Response::new(stream))
    }

    // ── Networking ────────────────────────────────────────────────

    async fn list_swarm_networks(
        &self, request: Request<SwarmNetworkListRequest>,
    ) -> Result<Response<SwarmNetworkListResponse>, Status> {
        let req = request.into_inner();
        debug!(swarm_only = req.swarm_only, "Listing swarm networks");
        crate::swarm::event::list_networks(&self.state.docker, req.swarm_only).await.map(Response::new)
    }

    async fn inspect_swarm_network(
        &self, request: Request<SwarmNetworkInspectRequest>,
    ) -> Result<Response<SwarmNetworkInspectResponse>, Status> {
        let network_id = request.into_inner().network_id;
        debug!(network_id = %network_id, "Inspecting swarm network");
        let network = crate::swarm::event::inspect_network(&self.state.docker, &network_id).await?;
        Ok(Response::new(SwarmNetworkInspectResponse { network: Some(network) }))
    }

    async fn network_connect(
        &self, request: Request<NetworkConnectRequest>,
    ) -> Result<Response<NetworkConnectResponse>, Status> {
        let req = request.into_inner();
        crate::swarm::event::connect_network(
            &self.state.docker, &req.network_id, &req.container_id,
        ).await.map(Response::new)
    }

    async fn network_disconnect(
        &self, request: Request<NetworkDisconnectRequest>,
    ) -> Result<Response<NetworkDisconnectResponse>, Status> {
        let req = request.into_inner();
        crate::swarm::event::disconnect_network(
            &self.state.docker, &req.network_id, &req.container_id, req.force,
        ).await.map(Response::new)
    }

    // ── Event / Update Streams ────────────────────────────────────

    async fn service_update_stream(
        &self, request: Request<ServiceUpdateStreamRequest>,
    ) -> Result<Response<Self::ServiceUpdateStreamStream>, Status> {
        let req = request.into_inner();
        let poll_ms = req.poll_interval_ms.unwrap_or(1000).max(500);
        debug!(service_id = %req.service_id, poll_ms, "Starting service update stream");
        Ok(Response::new(crate::swarm::event::update_stream(
            self.state.docker.clone(), req.service_id, poll_ms,
        )))
    }

    async fn service_event_stream(
        &self, request: Request<ServiceEventStreamRequest>,
    ) -> Result<Response<Self::ServiceEventStreamStream>, Status> {
        let req = request.into_inner();
        let service_id = req.service_id.trim().to_string();
        if service_id.is_empty() {
            return Err(Status::invalid_argument("service_id must not be empty"));
        }
        let poll_ms = if req.poll_interval_ms == 0 { 2000 } else { req.poll_interval_ms };
        info!("Starting service event stream for service {} (poll={}ms)", service_id, poll_ms);
        Ok(Response::new(crate::swarm::event::service_event_stream(
            self.state.docker.clone(), service_id, poll_ms,
        )))
    }

    // ── Secrets & Configs ─────────────────────────────────────────

    async fn list_secrets(
        &self, _request: Request<SecretListRequest>,
    ) -> Result<Response<SecretListResponse>, Status> {
        debug!("Listing swarm secrets (metadata only)");
        let secret_infos = crate::swarm::secret::list(&self.state.docker).await?;
        Ok(Response::new(SecretListResponse { secrets: secret_infos }))
    }

    async fn list_configs(
        &self, _request: Request<ConfigListRequest>,
    ) -> Result<Response<ConfigListResponse>, Status> {
        debug!("Listing swarm configs (metadata only)");
        let config_infos = crate::swarm::config::list(&self.state.docker).await?;
        Ok(Response::new(ConfigListResponse { configs: config_infos }))
    }

    async fn create_secret(
        &self, request: Request<CreateSecretRequest>,
    ) -> Result<Response<CreateSecretResponse>, Status> {
        let req = request.into_inner();
        info!(name = %req.name, "Creating swarm secret");
        crate::swarm::secret::create(&self.state.docker, &req.name, &req.data, req.labels).await.map(Response::new)
    }

    async fn delete_secret(
        &self, request: Request<DeleteSecretRequest>,
    ) -> Result<Response<DeleteSecretResponse>, Status> {
        let req = request.into_inner();
        info!(secret_id = %req.secret_id, "Deleting swarm secret");
        crate::swarm::secret::delete(&self.state.docker, &req.secret_id).await.map(Response::new)
    }

    async fn create_config(
        &self, request: Request<CreateConfigRequest>,
    ) -> Result<Response<CreateConfigResponse>, Status> {
        let req = request.into_inner();
        info!(name = %req.name, "Creating swarm config");
        crate::swarm::config::create(&self.state.docker, &req.name, &req.data, req.labels).await.map(Response::new)
    }

    async fn delete_config(
        &self, request: Request<DeleteConfigRequest>,
    ) -> Result<Response<DeleteConfigResponse>, Status> {
        let req = request.into_inner();
        info!(config_id = %req.config_id, "Deleting swarm config");
        crate::swarm::config::delete(&self.state.docker, &req.config_id).await.map(Response::new)
    }

    // ── Health & Coverage ─────────────────────────────────────────

    async fn get_service_coverage(
        &self, request: Request<ServiceCoverageRequest>,
    ) -> Result<Response<ServiceCoverageResponse>, Status> {
        let service_id = request.into_inner().service_id;
        crate::swarm::health::get_coverage(&self.state.docker, &service_id).await.map(Response::new)
    }

    async fn get_stack_health(
        &self, request: Request<StackHealthRequest>,
    ) -> Result<Response<StackHealthResponse>, Status> {
        let namespace = request.into_inner().namespace;
        crate::swarm::health::get_stack_health(&self.state.docker, &namespace).await.map(Response::new)
    }

    async fn service_restart_event_stream(
        &self, request: Request<ServiceRestartEventStreamRequest>,
    ) -> Result<Response<Self::ServiceRestartEventStreamStream>, Status> {
        let req = request.into_inner();
        let filter = if req.service_id.is_empty() { None } else { Some(req.service_id) };
        let poll_ms = if req.poll_interval_ms == 0 { 2000 } else { req.poll_interval_ms };
        info!("Starting service restart event stream (filter={:?}, poll={}ms)", filter, poll_ms);
        Ok(Response::new(crate::swarm::health::restart_event_stream(
            self.state.docker.clone(), filter, poll_ms,
        )))
    }

    // ── Swarm Init / Join / Leave ─────────────────────────────────

    async fn swarm_init(
        &self, request: Request<SwarmInitRequest>,
    ) -> Result<Response<SwarmInitResponse>, Status> {
        let req = request.into_inner();
        crate::swarm::info::init(
            &self.state.docker, &req.listen_addr, &req.advertise_addr, req.force_new_cluster,
        ).await.map(Response::new)
    }

    async fn swarm_join(
        &self, request: Request<SwarmJoinRequest>,
    ) -> Result<Response<SwarmJoinResponse>, Status> {
        let req = request.into_inner();
        crate::swarm::info::join(
            &self.state.docker, req.remote_addrs, &req.join_token, &req.listen_addr, &req.advertise_addr,
        ).await.map(Response::new)
    }

    async fn swarm_leave(
        &self, request: Request<SwarmLeaveRequest>,
    ) -> Result<Response<SwarmLeaveResponse>, Status> {
        let req = request.into_inner();
        crate::swarm::info::leave(&self.state.docker, req.force).await.map(Response::new)
    }

    // ── Swarm Update / Unlock ─────────────────────────────────────

    async fn swarm_update(
        &self, request: Request<SwarmUpdateRequest>,
    ) -> Result<Response<SwarmUpdateResponse>, Status> {
        let req = request.into_inner();
        crate::swarm::update::update_swarm(
            &self.state.docker,
            req.autolock, req.task_history_limit, req.snapshot_interval,
            req.heartbeat_tick, req.election_tick, req.cert_expiry_ns,
            req.rotate_worker_token, req.rotate_manager_token, req.rotate_manager_unlock_key,
        ).await.map(Response::new)
    }

    async fn swarm_unlock_key(
        &self, _request: Request<SwarmUnlockKeyRequest>,
    ) -> Result<Response<SwarmUnlockKeyResponse>, Status> {
        crate::swarm::info::unlock_key(&self.state.docker).await.map(Response::new)
    }

    async fn swarm_unlock(
        &self, request: Request<SwarmUnlockRequest>,
    ) -> Result<Response<SwarmUnlockResponse>, Status> {
        let req = request.into_inner();
        crate::swarm::info::unlock(&self.state.docker, &req.unlock_key).await.map(Response::new)
    }

    // ── Compose Stack Deploy ──────────────────────────────────────

    async fn deploy_compose_stack(
        &self, request: Request<DeployComposeStackRequest>,
    ) -> Result<Response<DeployComposeStackResponse>, Status> {
        let req = request.into_inner();
        info!(stack_name = %req.stack_name, "Deploying compose stack");
        let result = crate::swarm::compose::deploy(&self.state.docker, &req.stack_name, &req.compose_yaml).await;
        let all_ok = result.failed.is_empty();
        let response = crate::swarm::compose::into_response(&req.stack_name, result);
        // Only persist the stack file on successful deployment
        if all_ok {
            let mut store = self.state.stack_files.lock().await;
            store.insert(req.stack_name.clone(), req.compose_yaml.clone());
        }
        Ok(Response::new(response))
    }

    async fn get_stack_file(
        &self, request: Request<GetStackFileRequest>,
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
