//! Route — StatsService gRPC handler.

use tonic::{Request, Response, Status};
use tracing::{debug, error};
use tokio_stream::StreamExt;

use crate::state::SharedState;
use crate::stats::map;

use crate::proto::{
    stats_service_server::StatsService,
    ContainerStatsRequest, ContainerStatsResponse,
};

/// Provides real-time container resource statistics
pub struct StatsServiceImpl {
    state: SharedState,
}

impl StatsServiceImpl {
    pub fn new(state: SharedState) -> Self {
        Self { state }
    }

    /// Get single stats snapshot
    async fn get_stats_once(&self, container_id: &str) -> Result<ContainerStatsResponse, Status> {
        let mut stats_stream = self.state.docker
            .stats(container_id, false)
            .await
            .map_err(|e| {
                error!("Failed to get stats for container {}: {}", container_id, e);
                map::classify_docker_error(container_id, e)
            })?;

        if let Some(stats_result) = stats_stream.next().await {
            let stats = stats_result.map_err(|e| {
                error!("Error reading stats: {}", e);
                Status::internal(format!("Failed to read stats: {}", e))
            })?;

            debug!("Retrieved stats for container {}", container_id);
            Ok(map::convert_stats(container_id, stats))
        } else {
            Err(Status::internal("No stats available"))
        }
    }
}

#[tonic::async_trait]
impl StatsService for StatsServiceImpl {
    async fn get_container_stats(
        &self,
        request: Request<ContainerStatsRequest>,
    ) -> Result<Response<ContainerStatsResponse>, Status> {
        let req = request.into_inner();
        let container_id = req.container_id.trim().to_string();

        if container_id.is_empty() {
            return Err(Status::invalid_argument("container_id must not be empty"));
        }

        debug!("Getting stats for container: {}", container_id);
        let stats = self.get_stats_once(&container_id).await?;
        Ok(Response::new(stats))
    }

    async fn stream_container_stats(
        &self,
        request: Request<ContainerStatsRequest>,
    ) -> Result<Response<Self::StreamContainerStatsStream>, Status> {
        let req = request.into_inner();
        let container_id = req.container_id.trim().to_string();

        if container_id.is_empty() {
            return Err(Status::invalid_argument("container_id must not be empty"));
        }

        debug!("Starting stats stream for container: {}", container_id);

        let stats_stream = self.state.docker
            .stats(&container_id, true)
            .await
            .map_err(|e| {
                error!("Failed to start stats stream for {}: {}", container_id, e);
                map::classify_docker_error(&container_id, e)
            })?;

        let container_id_clone = container_id.clone();

        let output_stream = stats_stream.map(move |result| {
            match result {
                Ok(stats) => Ok(map::convert_stats(&container_id_clone, stats)),
                Err(e) => {
                    error!("Error in stats stream: {}", e);
                    Err(Status::internal(format!("Stats stream error: {}", e)))
                }
            }
        });

        Ok(Response::new(Box::pin(output_stream)))
    }

    type StreamContainerStatsStream = std::pin::Pin<
        Box<dyn tokio_stream::Stream<Item = Result<ContainerStatsResponse, Status>> + Send>
    >;
}
