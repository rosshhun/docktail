use super::Result;
use tonic::transport::Channel;

// Include the generated protobuf code
mod proto {
    tonic::include_proto!("docktail.agent");
}

pub use proto::{
    log_service_client::LogServiceClient,
    inventory_service_client::InventoryServiceClient,
    health_service_client::HealthServiceClient,
    stats_service_client::StatsServiceClient,
    // Request/Response types
    LogStreamRequest, NormalizedLogEntry,
    ContainerListRequest, ContainerListResponse,
    ContainerInspectRequest, ContainerInspectResponse,
    HealthCheckRequest, HealthCheckResponse,
    ContainerStatsRequest, ContainerStatsResponse,
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
}

impl AgentGrpcClient {
    /// Create a new client from a gRPC channel
    pub fn new(channel: Channel) -> Self {
        Self {
            log_client: LogServiceClient::new(channel.clone()),
            inventory_client: InventoryServiceClient::new(channel.clone()),
            health_client: HealthServiceClient::new(channel.clone()),
            stats_client: StatsServiceClient::new(channel),
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
}
