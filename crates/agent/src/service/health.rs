use std::pin::Pin;
use std::sync::Arc;
use tokio_stream::Stream;
use tonic::{Request, Response, Status};

use super::proto::{
    health_service_server::HealthService,
    HealthCheckRequest, HealthCheckResponse,
    HealthStatus,
};
use crate::parser::metrics::{ParsingMetrics, MetricsSnapshot};

/// Implementation of the HealthService gRPC service
/// Provides health check and monitoring capabilities based on real-time metrics
pub struct HealthServiceImpl {
    /// Reference to the global parsing metrics for health determination
    metrics: Arc<ParsingMetrics>,
}

impl HealthServiceImpl {
    pub fn new(metrics: Arc<ParsingMetrics>) -> Self {
        Self { metrics }
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

        let response = HealthCheckResponse {
            status: status as i32,
            message,
            timestamp: chrono::Utc::now().timestamp(),
            metadata: snapshot.to_metadata_map(),
        };

        Ok(Response::new(response))
    }

    type WatchStream = Pin<Box<dyn Stream<Item = Result<HealthCheckResponse, Status>> + Send>>;

    async fn watch(
        &self,
        _request: Request<HealthCheckRequest>,
    ) -> Result<Response<Self::WatchStream>, Status> {
        // Clone the Arc to move into the async stream
        let metrics = self.metrics.clone();

        let stream = async_stream::stream! {
            loop {
                // Re-evaluate health on every tick
                let snapshot = metrics.snapshot();
                
                let (status, message) = HealthServiceImpl::evaluate_health(&snapshot);

                let response = HealthCheckResponse {
                    status: status as i32,
                    message,
                    timestamp: chrono::Utc::now().timestamp(),
                    metadata: snapshot.to_metadata_map(),
                };
                
                yield Ok(response);
                
                // Standard health check interval (configurable in production)
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        };

        Ok(Response::new(Box::pin(stream)))
    }
}
