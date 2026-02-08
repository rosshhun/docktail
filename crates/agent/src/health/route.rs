use std::pin::Pin;
use std::sync::Arc;
use tokio_stream::Stream;
use tonic::{Request, Response, Status};

use crate::proto::{
    health_service_server::HealthService,
    HealthCheckRequest, HealthCheckResponse,
    HealthStatus,
};
use crate::parser::metrics::{ParsingMetrics, MetricsSnapshot};
use crate::state::SharedState;

/// Implementation of the HealthService gRPC service
/// Provides health check and monitoring capabilities based on real-time metrics
pub struct HealthServiceImpl {
    /// Reference to the global parsing metrics for health determination
    metrics: Arc<ParsingMetrics>,
    /// Reference to agent state for swarm role detection
    state: SharedState,
}

impl HealthServiceImpl {
    pub fn new(metrics: Arc<ParsingMetrics>, state: SharedState) -> Self {
        Self { metrics, state }
    }

    /// Static health evaluation logic to ensure consistency between check() and watch()
    fn evaluate_health(snapshot: &MetricsSnapshot) -> (HealthStatus, String) {
        // Critical Failure: Parser panics indicate serious bugs (catch_unwind triggered)
        if snapshot.parse_panics > 0 {
            return (
                HealthStatus::Unhealthy,
                format!("Critical: {} parser panics detected", snapshot.parse_panics)
            );
        }

        // Critical Failure: Docker connectivity lost
        if snapshot.docker_consecutive_failures >= 3 {
             return (
                HealthStatus::Unhealthy,
                format!("Critical: Docker daemon unreachable ({} consecutive failures)", snapshot.docker_consecutive_failures)
            );
        }

        // Degraded State: Success rate drops below 80%
        // Only trigger if we have meaningful volume (>100 lines) to avoid false positives
        if snapshot.total_parsed > 100 && snapshot.success_rate < 0.80 {
            return (
                HealthStatus::Degraded,
                format!("Degraded: Success rate is {:.1}%", snapshot.success_rate * 100.0)
            );
        }

        // Healthy: All metrics are within acceptable ranges
        (
            HealthStatus::Healthy,
            format!("Agent is operating normally (parsed: {}, success: {:.1}%)", 
                snapshot.total_parsed, 
                snapshot.success_rate * 100.0)
        )
    }
}

#[tonic::async_trait]
impl HealthService for HealthServiceImpl {
    async fn check(
        &self,
        _request: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status> {
        let snapshot = self.metrics.snapshot();
        let (status, message) = Self::evaluate_health(&snapshot);

        // Refresh and include swarm role in metadata
        self.state.refresh_swarm_role().await;
        let swarm_role = self.state.get_swarm_role().await;

        let mut metadata = snapshot.to_metadata_map();
        metadata.insert("swarm_role".to_string(), swarm_role.as_str().to_string());

        let response = HealthCheckResponse {
            status: status as i32,
            message,
            timestamp: chrono::Utc::now().timestamp(),
            metadata,
        };

        Ok(Response::new(response))
    }

    type WatchStream = Pin<Box<dyn Stream<Item = Result<HealthCheckResponse, Status>> + Send>>;

    async fn watch(
        &self,
        _request: Request<HealthCheckRequest>,
    ) -> Result<Response<Self::WatchStream>, Status> {
        // Clone the Arcs to move into the async stream
        let metrics = self.metrics.clone();
        let state = self.state.clone();

        let stream = async_stream::stream! {
            loop {
                // Re-evaluate health on every tick
                let snapshot = metrics.snapshot();
                
                let (status, message) = HealthServiceImpl::evaluate_health(&snapshot);

                // Refresh and include swarm role
                state.refresh_swarm_role().await;
                let swarm_role = state.get_swarm_role().await;

                let mut metadata = snapshot.to_metadata_map();
                metadata.insert("swarm_role".to_string(), swarm_role.as_str().to_string());

                let response = HealthCheckResponse {
                    status: status as i32,
                    message,
                    timestamp: chrono::Utc::now().timestamp(),
                    metadata,
                };
                
                yield Ok(response);
                
                // Standard health check interval (configurable in production)
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        };

        Ok(Response::new(Box::pin(stream)))
    }
}
