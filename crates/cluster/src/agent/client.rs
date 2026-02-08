use super::Result;
use tonic::transport::Channel;

// Include the generated protobuf code
pub mod proto {
    tonic::include_proto!("docktail.agent");
}

pub use proto::{
    log_service_client::LogServiceClient,
    inventory_service_client::InventoryServiceClient,
    health_service_client::HealthServiceClient,
    stats_service_client::StatsServiceClient,
    control_service_client::ControlServiceClient,
    swarm_service_client::SwarmServiceClient,
    shell_service_client::ShellServiceClient,
    // Request/Response types
    LogStreamRequest, NormalizedLogEntry,
    ContainerListRequest, ContainerListResponse,
    ContainerInspectRequest, ContainerInspectResponse,
    HealthCheckRequest, HealthCheckResponse,
    ContainerStatsRequest, ContainerStatsResponse,
    ContainerControlRequest, ContainerControlResponse,
    ContainerRemoveRequest,
    // Image/Volume/Network management
    PullImageRequest, PullImageResponse,
    RemoveImageRequest, RemoveImageResponse,
    CreateVolumeRequest, CreateVolumeResponse,
    RemoveVolumeRequest, RemoveVolumeResponse,
    CreateNetworkRequest, CreateNetworkResponse,
    RemoveNetworkRequest, RemoveNetworkResponse,
    // Docker events stream (B01)
    DockerEventsRequest, DockerEvent,
    // Swarm types
    SwarmInfoRequest, SwarmInfoResponse,
    NodeListRequest, NodeListResponse,
    ServiceListRequest, ServiceListResponse,
    ServiceInspectRequest, ServiceInspectResponse,
    TaskListRequest, TaskListResponse,
    // S3: Service log streaming
    ServiceLogStreamRequest,
    // M6: Service management
    CreateServiceRequest, CreateServiceResponse,
    DeleteServiceRequest, DeleteServiceResponse,
    UpdateServiceRequest, UpdateServiceResponse,
    ServicePortConfig,
    // S5: Swarm networking
    SwarmNetworkListRequest, SwarmNetworkListResponse,
    SwarmNetworkInspectRequest, SwarmNetworkInspectResponse,
    // S6: Orchestration observability
    ServiceUpdateStreamRequest, ServiceUpdateEvent,
    // S8: Secrets & configs
    SecretListRequest, SecretListResponse,
    ConfigListRequest, ConfigListResponse,
    // B08/B09: Secret & config CRUD
    CreateSecretRequest, CreateSecretResponse,
    DeleteSecretRequest, DeleteSecretResponse,
    CreateConfigRequest, CreateConfigResponse,
    DeleteConfigRequest, DeleteConfigResponse,
    // S9: Node management & drain awareness
    NodeInspectRequest, NodeInspectResponse,
    NodeUpdateRequest, NodeUpdateResponse,
    NodeEventStreamRequest, NodeEvent,
    // S10: Service scaling insights & coverage
    ServiceEventStreamRequest, ServiceEvent,
    ServiceCoverageRequest, ServiceCoverageResponse,
    // S11: Stack health & restart policies
    StackHealthRequest, StackHealthResponse,
    ServiceRestartEventStreamRequest, ServiceRestartEvent,
    // B06: Service rollback
    RollbackServiceRequest, RollbackServiceResponse,
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
    TaskInspectRequest, TaskInspectResponse,
    // B05: Swarm update/unlock
    SwarmUpdateRequest, SwarmUpdateResponse,
    SwarmUnlockKeyRequest, SwarmUnlockKeyResponse,
    SwarmUnlockRequest, SwarmUnlockResponse,
    // B11: Compose stack deploy
    DeployComposeStackRequest, DeployComposeStackResponse,
    // B12: Stack file viewer
    GetStackFileRequest, GetStackFileResponse,
    // Shell/Exec types
    ExecCommandRequest, ExecCommandResponse,
    ShellRequest, ShellResponse,
    OpenShellInit, ShellInput, ShellResize, TerminalSize,
    shell_request, shell_response,
    // Enums
    LogLevel, FilterMode, LogFormat,
};

/// Wrapper around generated gRPC clients for a single agent
///
/// Tonic clients are cheap to clone (Arc internally), allowing
/// the client to be shared across multiple async tasks.
#[derive(Clone)]
pub struct AgentGrpcClient {
    log_client: LogServiceClient<Channel>,
    inventory_client: InventoryServiceClient<Channel>,
    health_client: HealthServiceClient<Channel>,
    stats_client: StatsServiceClient<Channel>,
    control_client: ControlServiceClient<Channel>,
    swarm_client: SwarmServiceClient<Channel>,
    shell_client: ShellServiceClient<Channel>,
}

impl AgentGrpcClient {
    /// Create a new client from a gRPC channel
    pub fn new(channel: Channel) -> Self {
        Self {
            log_client: LogServiceClient::new(channel.clone()),
            inventory_client: InventoryServiceClient::new(channel.clone()),
            health_client: HealthServiceClient::new(channel.clone()),
            stats_client: StatsServiceClient::new(channel.clone()),
            control_client: ControlServiceClient::new(channel.clone()),
            swarm_client: SwarmServiceClient::new(channel.clone()),
            shell_client: ShellServiceClient::new(channel),
        }
    }

    /// Stream logs from a container
    pub async fn stream_logs(
        &mut self,
        request: LogStreamRequest,
    ) -> Result<tonic::Streaming<NormalizedLogEntry>> {
        let response = self
            .log_client
            .stream_logs(tonic::Request::new(request))
            .await?;

        Ok(response.into_inner())
    }

    /// List containers on the agent
    pub async fn list_containers(
        &mut self,
        request: ContainerListRequest,
    ) -> Result<ContainerListResponse> {
        let response = self
            .inventory_client
            .list_containers(tonic::Request::new(request))
            .await?;

        Ok(response.into_inner())
    }

    /// Inspect a specific container
    pub async fn inspect_container(
        &mut self,
        request: ContainerInspectRequest,
    ) -> Result<ContainerInspectResponse> {
        let response = self
            .inventory_client
            .inspect_container(tonic::Request::new(request))
            .await?;

        Ok(response.into_inner())
    }

    /// Health check
    pub async fn check_health(
        &mut self,
        request: HealthCheckRequest,
    ) -> Result<HealthCheckResponse> {
        let response = self
            .health_client
            .check(tonic::Request::new(request))
            .await?;

        Ok(response.into_inner())
    }

    /// Watch health status (streaming)
    pub async fn watch_health(
        &mut self,
        request: HealthCheckRequest,
    ) -> Result<tonic::Streaming<HealthCheckResponse>> {
        let response = self
            .health_client
            .watch(tonic::Request::new(request))
            .await?;

        Ok(response.into_inner())
    }

    /// Get container stats
    pub async fn get_container_stats(
        &mut self,
        request: ContainerStatsRequest,
    ) -> Result<ContainerStatsResponse> {
        let response = self
            .stats_client
            .get_container_stats(tonic::Request::new(request))
            .await?;

        Ok(response.into_inner())
    }

    /// Stream container stats
    pub async fn stream_container_stats(
        &mut self,
        request: ContainerStatsRequest,
    ) -> Result<tonic::Streaming<ContainerStatsResponse>> {
        let response = self
            .stats_client
            .stream_container_stats(tonic::Request::new(request))
            .await?;

        Ok(response.into_inner())
    }

    // =========================================================================
    // Container Lifecycle Methods
    // =========================================================================

    /// Start a container
    pub async fn start_container(
        &mut self,
        request: ContainerControlRequest,
    ) -> Result<ContainerControlResponse> {
        let response = self
            .control_client
            .start_container(tonic::Request::new(request))
            .await?;
        Ok(response.into_inner())
    }

    /// Stop a container
    pub async fn stop_container(
        &mut self,
        request: ContainerControlRequest,
    ) -> Result<ContainerControlResponse> {
        let response = self
            .control_client
            .stop_container(tonic::Request::new(request))
            .await?;
        Ok(response.into_inner())
    }

    /// Restart a container
    pub async fn restart_container(
        &mut self,
        request: ContainerControlRequest,
    ) -> Result<ContainerControlResponse> {
        let response = self
            .control_client
            .restart_container(tonic::Request::new(request))
            .await?;
        Ok(response.into_inner())
    }

    /// Pause a container
    pub async fn pause_container(
        &mut self,
        request: ContainerControlRequest,
    ) -> Result<ContainerControlResponse> {
        let response = self
            .control_client
            .pause_container(tonic::Request::new(request))
            .await?;
        Ok(response.into_inner())
    }

    /// Unpause a container
    pub async fn unpause_container(
        &mut self,
        request: ContainerControlRequest,
    ) -> Result<ContainerControlResponse> {
        let response = self
            .control_client
            .unpause_container(tonic::Request::new(request))
            .await?;
        Ok(response.into_inner())
    }

    /// Remove a container
    pub async fn remove_container(
        &mut self,
        request: ContainerRemoveRequest,
    ) -> Result<ContainerControlResponse> {
        let response = self
            .control_client
            .remove_container(tonic::Request::new(request))
            .await?;
        Ok(response.into_inner())
    }

    // =========================================================================
    // Swarm Methods
    // =========================================================================

    /// Get swarm information
    pub async fn get_swarm_info(
        &mut self,
        request: SwarmInfoRequest,
    ) -> Result<SwarmInfoResponse> {
        let response = self
            .swarm_client
            .get_swarm_info(tonic::Request::new(request))
            .await?;
        Ok(response.into_inner())
    }

    /// List swarm nodes
    pub async fn list_nodes(
        &mut self,
        request: NodeListRequest,
    ) -> Result<NodeListResponse> {
        let response = self
            .swarm_client
            .list_nodes(tonic::Request::new(request))
            .await?;
        Ok(response.into_inner())
    }

    /// List swarm services
    pub async fn list_services(
        &mut self,
        request: ServiceListRequest,
    ) -> Result<ServiceListResponse> {
        let response = self
            .swarm_client
            .list_services(tonic::Request::new(request))
            .await?;
        Ok(response.into_inner())
    }

    /// Inspect a swarm service
    pub async fn inspect_service(
        &mut self,
        request: ServiceInspectRequest,
    ) -> Result<ServiceInspectResponse> {
        let response = self
            .swarm_client
            .inspect_service(tonic::Request::new(request))
            .await?;
        Ok(response.into_inner())
    }

    /// List swarm tasks
    pub async fn list_tasks(
        &mut self,
        request: TaskListRequest,
    ) -> Result<TaskListResponse> {
        let response = self
            .swarm_client
            .list_tasks(tonic::Request::new(request))
            .await?;
        Ok(response.into_inner())
    }

    // =========================================================================
    // S3: Service Log Streaming
    // =========================================================================

    /// Stream aggregated logs from all tasks of a swarm service
    pub async fn stream_service_logs(
        &mut self,
        request: ServiceLogStreamRequest,
    ) -> Result<tonic::Streaming<NormalizedLogEntry>> {
        let response = self
            .swarm_client
            .stream_service_logs(tonic::Request::new(request))
            .await?;
        Ok(response.into_inner())
    }

    // =========================================================================
    // M6: Service Management (Compose/Stack)
    // =========================================================================

    /// Create a swarm service
    pub async fn create_service(
        &mut self,
        request: CreateServiceRequest,
    ) -> Result<CreateServiceResponse> {
        let response = self
            .swarm_client
            .create_service(tonic::Request::new(request))
            .await?;
        Ok(response.into_inner())
    }

    /// Delete a swarm service
    pub async fn delete_service(
        &mut self,
        request: DeleteServiceRequest,
    ) -> Result<DeleteServiceResponse> {
        let response = self
            .swarm_client
            .delete_service(tonic::Request::new(request))
            .await?;
        Ok(response.into_inner())
    }

    /// Update a swarm service
    pub async fn update_service(
        &mut self,
        request: UpdateServiceRequest,
    ) -> Result<UpdateServiceResponse> {
        let response = self
            .swarm_client
            .update_service(tonic::Request::new(request))
            .await?;
        Ok(response.into_inner())
    }

    /// List swarm networks (optionally filter to swarm-scoped only)
    pub async fn list_swarm_networks(
        &mut self,
        swarm_only: bool,
    ) -> Result<SwarmNetworkListResponse> {
        let response = self
            .swarm_client
            .list_swarm_networks(tonic::Request::new(SwarmNetworkListRequest {
                swarm_only,
            }))
            .await?;
        Ok(response.into_inner())
    }

    /// Inspect a specific swarm network by ID
    pub async fn inspect_swarm_network(
        &mut self,
        network_id: &str,
    ) -> Result<SwarmNetworkInspectResponse> {
        let response = self
            .swarm_client
            .inspect_swarm_network(tonic::Request::new(SwarmNetworkInspectRequest {
                network_id: network_id.to_string(),
            }))
            .await?;
        Ok(response.into_inner())
    }

    /// Stream rolling update progress for a service (S6)
    pub async fn service_update_stream(
        &mut self,
        service_id: &str,
        poll_interval_ms: Option<u64>,
    ) -> Result<tonic::Streaming<ServiceUpdateEvent>> {
        let response = self
            .swarm_client
            .service_update_stream(tonic::Request::new(ServiceUpdateStreamRequest {
                service_id: service_id.to_string(),
                poll_interval_ms,
            }))
            .await?;
        Ok(response.into_inner())
    }

    /// List swarm secrets (metadata only, never exposes secret data) (S8)
    pub async fn list_secrets(&mut self) -> Result<SecretListResponse> {
        let response = self
            .swarm_client
            .list_secrets(tonic::Request::new(SecretListRequest {}))
            .await?;
        Ok(response.into_inner())
    }

    /// List swarm configs (metadata only) (S8)
    pub async fn list_configs(&mut self) -> Result<ConfigListResponse> {
        let response = self
            .swarm_client
            .list_configs(tonic::Request::new(ConfigListRequest {}))
            .await?;
        Ok(response.into_inner())
    }

    // =========================================================================
    // S9: Node Management & Drain Awareness
    // =========================================================================

    pub async fn inspect_node(&mut self, node_id: &str) -> Result<NodeInspectResponse> {
        let response = self
            .swarm_client
            .inspect_node(tonic::Request::new(NodeInspectRequest {
                node_id: node_id.to_string(),
            }))
            .await?;
        Ok(response.into_inner())
    }

    pub async fn update_node(&mut self, request: NodeUpdateRequest) -> Result<NodeUpdateResponse> {
        let response = self
            .swarm_client
            .update_node(tonic::Request::new(request))
            .await?;
        Ok(response.into_inner())
    }

    pub async fn node_event_stream(
        &mut self,
        request: NodeEventStreamRequest,
    ) -> Result<tonic::Streaming<NodeEvent>> {
        let response = self
            .swarm_client
            .node_event_stream(tonic::Request::new(request))
            .await?;
        Ok(response.into_inner())
    }

    // =========================================================================
    // S10: Service Scaling Insights & Coverage
    // =========================================================================

    pub async fn service_event_stream(
        &mut self,
        service_id: &str,
        poll_interval_ms: Option<u64>,
    ) -> Result<tonic::Streaming<ServiceEvent>> {
        let response = self
            .swarm_client
            .service_event_stream(tonic::Request::new(ServiceEventStreamRequest {
                service_id: service_id.to_string(),
                poll_interval_ms: poll_interval_ms.unwrap_or(2000),
            }))
            .await?;
        Ok(response.into_inner())
    }

    pub async fn get_service_coverage(
        &mut self,
        service_id: &str,
    ) -> Result<ServiceCoverageResponse> {
        let response = self
            .swarm_client
            .get_service_coverage(tonic::Request::new(ServiceCoverageRequest {
                service_id: service_id.to_string(),
            }))
            .await?;
        Ok(response.into_inner())
    }

    // =========================================================================
    // S11: Stack Health & Restart Policies
    // =========================================================================

    pub async fn get_stack_health(
        &mut self,
        namespace: &str,
    ) -> Result<StackHealthResponse> {
        let response = self
            .swarm_client
            .get_stack_health(tonic::Request::new(StackHealthRequest {
                namespace: namespace.to_string(),
            }))
            .await?;
        Ok(response.into_inner())
    }

    pub async fn service_restart_event_stream(
        &mut self,
        service_id: Option<&str>,
        poll_interval_ms: Option<u64>,
    ) -> Result<tonic::Streaming<ServiceRestartEvent>> {
        let response = self
            .swarm_client
            .service_restart_event_stream(tonic::Request::new(ServiceRestartEventStreamRequest {
                service_id: service_id.unwrap_or_default().to_string(),
                poll_interval_ms: poll_interval_ms.unwrap_or(2000),
            }))
            .await?;
        Ok(response.into_inner())
    }

    // =========================================================================
    // Shell / Exec Methods
    // =========================================================================

    /// Execute a one-shot command inside a container (non-interactive)
    pub async fn exec_command(
        &mut self,
        request: ExecCommandRequest,
    ) -> Result<ExecCommandResponse> {
        let response = self
            .shell_client
            .exec_command(tonic::Request::new(request))
            .await?;
        Ok(response.into_inner())
    }

    /// Open an interactive shell (bidirectional streaming)
    ///
    /// Returns a pair of (sender, response_stream) so the caller can
    /// forward stdin/resize from WebSocket and pipe stdout back.
    pub async fn open_shell(
        &mut self,
        request_stream: impl tonic::IntoStreamingRequest<Message = ShellRequest>,
    ) -> Result<tonic::Streaming<ShellResponse>> {
        let response = self
            .shell_client
            .open_shell(request_stream)
            .await?;
        Ok(response.into_inner())
    }

    // =========================================================================
    // Image / Volume / Network Management
    // =========================================================================

    pub async fn pull_image(&mut self, request: PullImageRequest) -> Result<PullImageResponse> {
        let response = self.control_client.pull_image(tonic::Request::new(request)).await?;
        Ok(response.into_inner())
    }

    pub async fn remove_image(&mut self, request: RemoveImageRequest) -> Result<RemoveImageResponse> {
        let response = self.control_client.remove_image(tonic::Request::new(request)).await?;
        Ok(response.into_inner())
    }

    pub async fn create_volume(&mut self, request: CreateVolumeRequest) -> Result<CreateVolumeResponse> {
        let response = self.control_client.create_volume(tonic::Request::new(request)).await?;
        Ok(response.into_inner())
    }

    pub async fn remove_volume(&mut self, request: RemoveVolumeRequest) -> Result<RemoveVolumeResponse> {
        let response = self.control_client.remove_volume(tonic::Request::new(request)).await?;
        Ok(response.into_inner())
    }

    pub async fn create_network_rpc(&mut self, request: CreateNetworkRequest) -> Result<CreateNetworkResponse> {
        let response = self.control_client.create_network(tonic::Request::new(request)).await?;
        Ok(response.into_inner())
    }

    pub async fn remove_network_rpc(&mut self, request: RemoveNetworkRequest) -> Result<RemoveNetworkResponse> {
        let response = self.control_client.remove_network(tonic::Request::new(request)).await?;
        Ok(response.into_inner())
    }

    /// Stream Docker engine events (B01)
    pub async fn stream_docker_events(
        &mut self,
        request: DockerEventsRequest,
    ) -> Result<tonic::Streaming<DockerEvent>> {
        let response = self.control_client.stream_docker_events(tonic::Request::new(request)).await?;
        Ok(response.into_inner())
    }

    // =========================================================================
    // B06: Service Rollback
    // =========================================================================

    pub async fn rollback_service(&mut self, request: RollbackServiceRequest) -> Result<RollbackServiceResponse> {
        let response = self.swarm_client.rollback_service(tonic::Request::new(request)).await?;
        Ok(response.into_inner())
    }

    // =========================================================================
    // B08/B09: Secret & Config CRUD
    // =========================================================================

    pub async fn create_secret(&mut self, request: CreateSecretRequest) -> Result<CreateSecretResponse> {
        let response = self.swarm_client.create_secret(tonic::Request::new(request)).await?;
        Ok(response.into_inner())
    }

    pub async fn delete_secret(&mut self, request: DeleteSecretRequest) -> Result<DeleteSecretResponse> {
        let response = self.swarm_client.delete_secret(tonic::Request::new(request)).await?;
        Ok(response.into_inner())
    }

    pub async fn create_config(&mut self, request: CreateConfigRequest) -> Result<CreateConfigResponse> {
        let response = self.swarm_client.create_config(tonic::Request::new(request)).await?;
        Ok(response.into_inner())
    }

    pub async fn delete_config(&mut self, request: DeleteConfigRequest) -> Result<DeleteConfigResponse> {
        let response = self.swarm_client.delete_config(tonic::Request::new(request)).await?;
        Ok(response.into_inner())
    }

    // =========================================================================
    // B04/B05: Swarm Init / Join / Leave
    // =========================================================================

    pub async fn swarm_init(&mut self, request: SwarmInitRequest) -> Result<SwarmInitResponse> {
        let response = self.swarm_client.swarm_init(tonic::Request::new(request)).await?;
        Ok(response.into_inner())
    }

    pub async fn swarm_join(&mut self, request: SwarmJoinRequest) -> Result<SwarmJoinResponse> {
        let response = self.swarm_client.swarm_join(tonic::Request::new(request)).await?;
        Ok(response.into_inner())
    }

    pub async fn swarm_leave(&mut self, request: SwarmLeaveRequest) -> Result<SwarmLeaveResponse> {
        let response = self.swarm_client.swarm_leave(tonic::Request::new(request)).await?;
        Ok(response.into_inner())
    }

    // =========================================================================
    // B07: Node Remove
    // =========================================================================

    pub async fn remove_node(&mut self, request: RemoveNodeRequest) -> Result<RemoveNodeResponse> {
        let response = self.swarm_client.remove_node(tonic::Request::new(request)).await?;
        Ok(response.into_inner())
    }

    // =========================================================================
    // B14: Network Connect / Disconnect
    // =========================================================================

    pub async fn network_connect(&mut self, request: NetworkConnectRequest) -> Result<NetworkConnectResponse> {
        let response = self.swarm_client.network_connect(tonic::Request::new(request)).await?;
        Ok(response.into_inner())
    }

    pub async fn network_disconnect(&mut self, request: NetworkDisconnectRequest) -> Result<NetworkDisconnectResponse> {
        let response = self.swarm_client.network_disconnect(tonic::Request::new(request)).await?;
        Ok(response.into_inner())
    }

    // =========================================================================
    // B02: Task Log Streaming
    // =========================================================================

    pub async fn stream_task_logs(
        &mut self,
        request: TaskLogStreamRequest,
    ) -> Result<tonic::Streaming<NormalizedLogEntry>> {
        let response = self.swarm_client
            .stream_task_logs(tonic::Request::new(request))
            .await?;
        Ok(response.into_inner())
    }

    // =========================================================================
    // B03: Task Inspect
    // =========================================================================

    pub async fn inspect_task(&mut self, request: TaskInspectRequest) -> Result<TaskInspectResponse> {
        let response = self.swarm_client.inspect_task(tonic::Request::new(request)).await?;
        Ok(response.into_inner())
    }

    // =========================================================================
    // B05: Swarm Update / Unlock
    // =========================================================================

    pub async fn swarm_update(&mut self, request: SwarmUpdateRequest) -> Result<SwarmUpdateResponse> {
        let response = self.swarm_client.swarm_update(tonic::Request::new(request)).await?;
        Ok(response.into_inner())
    }

    pub async fn swarm_unlock_key(&mut self, request: SwarmUnlockKeyRequest) -> Result<SwarmUnlockKeyResponse> {
        let response = self.swarm_client.swarm_unlock_key(tonic::Request::new(request)).await?;
        Ok(response.into_inner())
    }

    pub async fn swarm_unlock(&mut self, request: SwarmUnlockRequest) -> Result<SwarmUnlockResponse> {
        let response = self.swarm_client.swarm_unlock(tonic::Request::new(request)).await?;
        Ok(response.into_inner())
    }

    // =========================================================================
    // B11: Compose Stack Deployment
    // =========================================================================

    pub async fn deploy_compose_stack(&mut self, request: DeployComposeStackRequest) -> Result<DeployComposeStackResponse> {
        let response = self.swarm_client.deploy_compose_stack(tonic::Request::new(request)).await?;
        Ok(response.into_inner())
    }

    // =========================================================================
    // B12: Stack File Viewer
    // =========================================================================

    pub async fn get_stack_file(&mut self, request: GetStackFileRequest) -> Result<GetStackFileResponse> {
        let response = self.swarm_client.get_stack_file(tonic::Request::new(request)).await?;
        Ok(response.into_inner())
    }
}
