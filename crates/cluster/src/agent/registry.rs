use super::{AgentPool, Result};
use std::sync::Arc;
use std::time::Duration;
use tokio::time;
use tracing::{debug, info};

/// Agent registry - manages agent lifecycle and health monitoring
pub struct AgentRegistry {
    pool: Arc<AgentPool>,
    health_check_interval: Duration,
    shutdown_rx: tokio::sync::watch::Receiver<bool>,
}

impl AgentRegistry {
    /// Create a new agent registry
    pub fn new(
        pool: Arc<AgentPool>,
        health_check_interval: Duration,
        shutdown_rx: tokio::sync::watch::Receiver<bool>,
    ) -> Self {
        Self {
            pool,
            health_check_interval,
            shutdown_rx,
        }
    }

    /// Start the health monitoring background task
    pub async fn start_health_monitoring(mut self) {
        info!(
            "Starting agent health monitoring (interval: {}s)",
            self.health_check_interval.as_secs()
        );

        let mut interval = time::interval(self.health_check_interval);
        interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    debug!("Running scheduled health check");
                    self.pool.health_check_all().await;
                }
                _ = self.shutdown_rx.changed() => {
                    if *self.shutdown_rx.borrow() {
                        info!("Received shutdown signal, stopping health monitoring");
                        break;
                    }
                }
            }
        }

        info!("Agent health monitoring stopped");
    }

    /// Perform an immediate health check on all agents
    #[allow(dead_code)]
    pub async fn health_check_now(&self) -> Result<()> {
        self.pool.health_check_all().await;
        Ok(())
    }
}
