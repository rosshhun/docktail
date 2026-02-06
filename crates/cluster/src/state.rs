use crate::config::ClusterConfig;
use crate::agent::{AgentPool, AgentRegistry};
use crate::metrics::SubscriptionMetrics;
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

/// Shared application state (thread-safe)
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<ClusterConfig>,
    pub agent_pool: Arc<AgentPool>,
    pub metrics: Arc<SubscriptionMetrics>,
    /// Watch channel for shutdown signaling.
    /// Unlike broadcast, watch never loses messages — receivers always
    /// see the latest value, even if they subscribe after the send.
    pub shutdown_tx: tokio::sync::watch::Sender<bool>,
}

impl AppState {
    pub fn new(config: ClusterConfig) -> Self {
        let (shutdown_tx, _) = tokio::sync::watch::channel(false);

        // Create agent pool
        let agent_pool = Arc::new(AgentPool::new(config.agents.clone()));
        
        // Create metrics tracker
        let metrics = Arc::new(SubscriptionMetrics::new());

        Self {
            config: Arc::new(config),
            agent_pool,
            metrics,
            shutdown_tx,
        }
    }

    /// Initialize the application state (async initialization)
    pub async fn initialize(&self) -> anyhow::Result<()> {
        info!("Initializing application state...");

        // Initialize agent pool
        self.agent_pool.initialize().await?;

        // Start health monitoring
        let registry = AgentRegistry::new(
            self.agent_pool.clone(),
            Duration::from_secs(self.config.agents.health_check_interval),
            self.shutdown_tx.subscribe(),
        );

        tokio::spawn(async move {
            registry.start_health_monitoring().await;
        });

        info!("✓ Application state initialized successfully");
        Ok(())
    }

    /// Signal shutdown to all components
    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
    }
}
