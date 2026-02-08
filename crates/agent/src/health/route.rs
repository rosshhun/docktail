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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_snapshot(
        total_parsed: u64,
        success_rate: f64,
        parse_panics: u64,
        docker_failures: u64,
    ) -> MetricsSnapshot {
        MetricsSnapshot {
            detection_attempts: 0,
            detection_success: 0,
            detection_fallback: 0,
            json_parsed: 0,
            logfmt_parsed: 0,
            syslog_parsed: 0,
            http_parsed: 0,
            plain_parsed: 0,
            total_parsed,
            avg_parse_time_us: 0.0,
            parse_errors: 0,
            parse_timeouts: 0,
            parse_panics,
            lines_too_large: 0,
            non_utf8_content: 0,
            success_rate,
            active_containers: 0,
            disabled_containers: 0,
            docker_consecutive_failures: docker_failures,
        }
    }

    #[test]
    fn test_evaluate_health_healthy() {
        let snap = make_snapshot(1000, 0.99, 0, 0);
        let (status, msg) = HealthServiceImpl::evaluate_health(&snap);
        assert!(matches!(status, HealthStatus::Healthy));
        assert!(msg.contains("normally"));
    }

    #[test]
    fn test_evaluate_health_unhealthy_panics() {
        let snap = make_snapshot(100, 0.95, 3, 0);
        let (status, msg) = HealthServiceImpl::evaluate_health(&snap);
        assert!(matches!(status, HealthStatus::Unhealthy));
        assert!(msg.contains("panic"));
    }

    #[test]
    fn test_evaluate_health_unhealthy_docker_failures() {
        let snap = make_snapshot(100, 0.95, 0, 5);
        let (status, msg) = HealthServiceImpl::evaluate_health(&snap);
        assert!(matches!(status, HealthStatus::Unhealthy));
        assert!(msg.contains("Docker"));
    }

    #[test]
    fn test_evaluate_health_degraded_low_success_rate() {
        let snap = make_snapshot(200, 0.60, 0, 0);
        let (status, msg) = HealthServiceImpl::evaluate_health(&snap);
        assert!(matches!(status, HealthStatus::Degraded));
        assert!(msg.contains("60.0%"));
    }

    #[test]
    fn test_evaluate_health_low_volume_not_degraded() {
        // Success rate is low but volume is too small to trigger
        let snap = make_snapshot(50, 0.60, 0, 0);
        let (status, _msg) = HealthServiceImpl::evaluate_health(&snap);
        assert!(matches!(status, HealthStatus::Healthy),
            "Low volume (50 < 100) should not trigger degraded status");
    }

    #[test]
    fn test_evaluate_health_fresh_agent_healthy() {
        // No data yet — should be healthy
        let snap = make_snapshot(0, 1.0, 0, 0);
        let (status, _msg) = HealthServiceImpl::evaluate_health(&snap);
        assert!(matches!(status, HealthStatus::Healthy));
    }

    #[test]
    fn test_evaluate_health_panic_takes_priority_over_docker() {
        // Both panics and docker failures — panic should win
        let snap = make_snapshot(100, 0.90, 1, 5);
        let (status, msg) = HealthServiceImpl::evaluate_health(&snap);
        assert!(matches!(status, HealthStatus::Unhealthy));
        assert!(msg.contains("panic"), "Panics should take priority: {}", msg);
    }

    #[test]
    fn test_evaluate_health_docker_takes_priority_over_degraded() {
        // Docker failures + low success rate — docker should win
        let snap = make_snapshot(200, 0.60, 0, 3);
        let (status, msg) = HealthServiceImpl::evaluate_health(&snap);
        assert!(matches!(status, HealthStatus::Unhealthy));
        assert!(msg.contains("Docker"), "Docker failures should take priority: {}", msg);
    }

    #[test]
    fn test_evaluate_health_boundary_docker_failures() {
        // 2 failures (under threshold) — should still be healthy
        let snap = make_snapshot(200, 0.95, 0, 2);
        let (status, _) = HealthServiceImpl::evaluate_health(&snap);
        assert!(matches!(status, HealthStatus::Healthy));

        // 3 failures (at threshold) — unhealthy
        let snap = make_snapshot(200, 0.95, 0, 3);
        let (status, _) = HealthServiceImpl::evaluate_health(&snap);
        assert!(matches!(status, HealthStatus::Unhealthy));
    }

    #[test]
    fn test_evaluate_health_boundary_success_rate() {
        // 80% (at threshold) — should still be healthy
        let snap = make_snapshot(200, 0.80, 0, 0);
        let (status, _) = HealthServiceImpl::evaluate_health(&snap);
        assert!(matches!(status, HealthStatus::Healthy));

        // 79% (below threshold) — degraded
        let snap = make_snapshot(200, 0.79, 0, 0);
        let (status, _) = HealthServiceImpl::evaluate_health(&snap);
        assert!(matches!(status, HealthStatus::Degraded));
    }
}
