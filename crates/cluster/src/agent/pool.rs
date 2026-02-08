use super::{AgentError, AgentGrpcClient, Result};
use crate::config::{AgentConfig, AgentRegistryConfig};
use dashmap::DashMap;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Identity};
use tracing::{debug, error, info, warn};

/// Agent health status
/// Matches the gRPC HealthStatus enum from agent.proto
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    Unknown = 0,
    Healthy = 1,
    Unhealthy = 2,
    Degraded = 3,  // Partial functionality - success rate degraded but not critical
}

/// How this agent was added to the pool
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentSource {
    /// Manually configured in cluster.toml
    Static,
    /// Discovered via Swarm node labels
    Discovered,
    /// Registered via HTTP API
    Registered,
}

impl From<u8> for HealthStatus {
    fn from(value: u8) -> Self {
        match value {
            1 => HealthStatus::Healthy,
            2 => HealthStatus::Unhealthy,
            3 => HealthStatus::Degraded,
            _ => HealthStatus::Unknown,
        }
    }
}

/// Agent information (metadata)
#[derive(Debug, Clone)]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub address: String,
    pub labels: HashMap<String, String>,
    pub version: Option<String>,
}

impl AgentInfo {
    pub fn from_config(config: &AgentConfig) -> Self {
        Self {
            id: config.id.clone(),
            name: config.name.clone(),
            address: config.address.clone(),
            labels: config.labels.clone(),
            version: None, // Will be populated during health check
        }
    }
}

/// A single agent connection
pub struct AgentConnection {
    pub info: AgentInfo,
    pub client: Arc<Mutex<AgentGrpcClient>>,
    pub source: AgentSource,
    health_status: Arc<AtomicU8>,
    last_seen: Arc<RwLock<Instant>>,
}

impl AgentConnection {
    /// Check if the agent is healthy
    pub fn is_healthy(&self) -> bool {
        let status: HealthStatus = self.health_status.load(Ordering::Acquire).into();
        status == HealthStatus::Healthy
    }

    /// Get current health status
    pub fn health_status(&self) -> HealthStatus {
        self.health_status.load(Ordering::Acquire).into()
    }

    /// Mark agent as healthy
    #[allow(dead_code)]
    pub fn mark_healthy(&self) {
        self.health_status.store(HealthStatus::Healthy as u8, Ordering::Release);
    }

    /// Mark agent as unhealthy
    pub fn mark_unhealthy(&self) {
        self.health_status.store(HealthStatus::Unhealthy as u8, Ordering::Release);
    }

    /// Mark agent as degraded
    #[allow(dead_code)]
    pub fn mark_degraded(&self) {
        self.health_status.store(HealthStatus::Degraded as u8, Ordering::Release);
    }

    /// Update health status from proto value
    fn update_health_status(&self, proto_status: i32) {
        // Map proto enum values to our HealthStatus
        // HEALTH_STATUS_UNSPECIFIED = 0
        // HEALTH_STATUS_HEALTHY = 1
        // HEALTH_STATUS_UNHEALTHY = 2
        // HEALTH_STATUS_DEGRADED = 3
        // HEALTH_STATUS_UNKNOWN = 4
        let status = match proto_status {
            1 => HealthStatus::Healthy,
            2 => HealthStatus::Unhealthy,
            3 => HealthStatus::Degraded,
            _ => HealthStatus::Unknown,
        };
        self.health_status.store(status as u8, Ordering::Release);
    }

    /// Get last seen timestamp
    pub async fn last_seen(&self) -> Instant {
        *self.last_seen.read().await
    }

    /// Update last seen timestamp
    pub async fn update_last_seen(&self) {
        *self.last_seen.write().await = Instant::now();
    }

    /// Perform health check with a dedicated 5-second timeout
    pub async fn check_health(&self) -> Result<()> {
        use super::client::HealthCheckRequest;

        // Clone the client to avoid holding the lock during network I/O
        // Tonic clients are cheap to clone (Arc internally)
        let mut client = {
            let guard = self.client.lock().await;
            guard.clone()
        };
        // Lock is dropped here - no blocking during network request

        let request = HealthCheckRequest {
            service: String::new(), // Empty means check overall health
        };

        // Use a dedicated short timeout for health checks to avoid
        // one slow agent blocking the entire health-check cycle
        let health_check_timeout = Duration::from_secs(5);
        let result = tokio::time::timeout(
            health_check_timeout,
            client.check_health(request),
        )
        .await;

        let rpc_result = match result {
            Ok(r) => r,
            Err(_) => {
                self.mark_unhealthy();
                warn!("Agent {} health check timed out after {}s", self.info.id, health_check_timeout.as_secs());
                return Err(AgentError::ConnectionFailed(
                    format!("Health check timed out for agent {}", self.info.id),
                ));
            }
        };

        match rpc_result {
            Ok(response) => {
                // Update status based on what the agent reported
                self.update_health_status(response.status);
                self.update_last_seen().await;
                
                let status = self.health_status();
                match status {
                    HealthStatus::Healthy => {
                        debug!("Agent {} health check passed: {}", self.info.id, response.message);
                    }
                    HealthStatus::Degraded => {
                        warn!("Agent {} is degraded: {}", self.info.id, response.message);
                    }
                    HealthStatus::Unhealthy => {
                        error!("Agent {} is unhealthy: {}", self.info.id, response.message);
                    }
                    HealthStatus::Unknown => {
                        warn!("Agent {} health status unknown: {}", self.info.id, response.message);
                    }
                }
                
                Ok(())
            }
            Err(e) => {
                self.mark_unhealthy();
                warn!("Agent {} health check failed: {}", self.info.id, e);
                Err(e)
            }
        }
    }
}

/// Agent connection pool
pub struct AgentPool {
    /// Map: agent_id -> AgentConnection
    connections: DashMap<String, Arc<AgentConnection>>,
    config: AgentRegistryConfig,
}

impl AgentPool {
    /// Create a new agent pool
    pub fn new(config: AgentRegistryConfig) -> Self {
        Self {
            connections: DashMap::new(),
            config,
        }
    }

    /// Initialize the pool with static agents from configuration
    pub async fn initialize(&self) -> Result<()> {
        info!("Initializing agent pool with {} static agents", self.config.static_agents.len());

        for agent_config in &self.config.static_agents {
            match self.add_agent(agent_config.clone()).await {
                Ok(_) => {
                    info!("✓ Agent '{}' ({}) added successfully", agent_config.name, agent_config.id);
                }
                Err(e) => {
                    error!("✗ Failed to add agent '{}' ({}): {}", agent_config.name, agent_config.id, e);
                    // Continue with other agents - don't fail the entire initialization
                }
            }
        }

        info!("Agent pool initialized with {} agents", self.connections.len());
        Ok(())
    }

    /// Add a new agent to the pool (static agents from config)
    pub async fn add_agent(&self, config: AgentConfig) -> Result<()> {
        self.add_agent_with_source(config, AgentSource::Static).await
    }

    /// Add a dynamically discovered or registered agent
    pub async fn add_dynamic_agent(&self, config: AgentConfig, source: AgentSource) -> Result<()> {
        if self.connections.contains_key(&config.id) {
            debug!("Agent {} already in pool, skipping", config.id);
            return Ok(());
        }
        self.add_agent_with_source(config, source).await
    }

    /// Internal: add agent with a specific source
    async fn add_agent_with_source(&self, config: AgentConfig, source: AgentSource) -> Result<()> {
        debug!("Adding agent: {} ({}) source={:?}", config.name, config.id, source);

        // Create mTLS channel
        let channel = self.create_channel(&config).await?;
        let client = AgentGrpcClient::new(channel);

        let connection = Arc::new(AgentConnection {
            info: AgentInfo::from_config(&config),
            client: Arc::new(Mutex::new(client)),
            source,
            health_status: Arc::new(AtomicU8::new(HealthStatus::Unknown as u8)),
            last_seen: Arc::new(RwLock::new(Instant::now())),
        });

        // Perform initial health check
        if let Err(e) = connection.check_health().await {
            warn!("Initial health check failed for agent {}: {}", config.id, e);
            // Still add the agent, but mark it as unhealthy
            connection.mark_unhealthy();
        }

        self.connections.insert(config.id.clone(), connection);
        Ok(())
    }

    /// Remove an agent from the pool
    pub fn remove_agent(&self, agent_id: &str) -> Option<Arc<AgentConnection>> {
        let removed = self.connections.remove(agent_id).map(|(_, conn)| conn);
        if let Some(ref conn) = removed {
            info!("Removed agent '{}' ({}) from pool", conn.info.name, conn.info.id);
        } else {
            warn!("Attempted to remove non-existent agent: {}", agent_id);
        }
        removed
    }

    /// Attempt to reconnect an unhealthy agent with backoff
    async fn reconnect_agent(&self, agent_id: &str) -> Result<()> {
        // Find the matching static config for this agent
        let agent_config = self.config.static_agents
            .iter()
            .find(|c| c.id == agent_id)
            .cloned();

        let config = match agent_config {
            Some(c) => c,
            None => {
                // For dynamic agents, skip reconnect (they will be re-discovered or re-registered)
                if let Some(conn) = self.connections.get(agent_id) {
                    if conn.source != AgentSource::Static {
                        debug!("Agent {} is dynamic ({:?}), skipping reconnect", agent_id, conn.source);
                        return Ok(());
                    }
                }
                debug!("Agent {} config not found, skipping reconnect", agent_id);
                return Ok(());
            }
        };

        let backoff_base = Duration::from_secs(self.config.reconnect_backoff);
        let max_attempts = self.config.max_reconnect_attempts;

        for attempt in 1..=max_attempts {
            info!(
                "Reconnecting agent {} (attempt {}/{})",
                agent_id, attempt, max_attempts
            );

            match self.create_channel(&config).await {
                Ok(channel) => {
                    let client = AgentGrpcClient::new(channel);

                    // Update the existing connection's client
                    if let Some(conn) = self.connections.get(agent_id) {
                        let mut guard = conn.client.lock().await;
                        *guard = client;
                    }

                    // Verify with a health check
                    if let Some(conn) = self.connections.get(agent_id) {
                        if conn.check_health().await.is_ok() {
                            info!("✓ Agent {} reconnected successfully", agent_id);
                            return Ok(());
                        }
                    }

                    warn!("Agent {} reconnected but health check failed", agent_id);
                }
                Err(e) => {
                    warn!(
                        "Reconnect attempt {}/{} failed for agent {}: {}",
                        attempt, max_attempts, agent_id, e
                    );
                }
            }

            // Exponential backoff: base * 2^(attempt-1), capped at 60s
            let delay = backoff_base
                .saturating_mul(1u32 << (attempt - 1).min(5))
                .min(Duration::from_secs(60));
            tokio::time::sleep(delay).await;
        }

        error!(
            "Failed to reconnect agent {} after {} attempts",
            agent_id, max_attempts
        );
        Err(AgentError::ConnectionFailed(format!(
            "Failed to reconnect agent {} after {} attempts",
            agent_id, max_attempts
        )))
    }

    /// Get an agent connection by ID
    pub fn get_agent(&self, agent_id: &str) -> Option<Arc<AgentConnection>> {
        self.connections.get(agent_id).map(|entry| entry.value().clone())
    }

    /// List all agent IDs
    #[allow(dead_code)]
    pub fn list_agent_ids(&self) -> Vec<String> {
        self.connections.iter().map(|entry| entry.key().clone()).collect()
    }

    /// List all agent connections
    pub fn list_agents(&self) -> Vec<Arc<AgentConnection>> {
        self.connections.iter().map(|entry| entry.value().clone()).collect()
    }

    /// Get agent info for all agents
    #[allow(dead_code)]
    pub fn list_agent_info(&self) -> Vec<AgentInfo> {
        self.connections
            .iter()
            .map(|entry| entry.value().info.clone())
            .collect()
    }

    /// Count total agents
    pub fn count(&self) -> usize {
        self.connections.len()
    }

    /// Count healthy agents
    pub fn count_healthy(&self) -> usize {
        self.connections
            .iter()
            .filter(|entry| entry.value().health_status() == HealthStatus::Healthy)
            .count()
    }

    /// Count unhealthy agents
    pub fn count_unhealthy(&self) -> usize {
        self.connections
            .iter()
            .filter(|entry| entry.value().health_status() == HealthStatus::Unhealthy)
            .count()
    }

    /// Count degraded agents
    pub fn count_degraded(&self) -> usize {
        self.connections
            .iter()
            .filter(|entry| entry.value().health_status() == HealthStatus::Degraded)
            .count()
    }

    /// Count agents with unknown health
    pub fn count_unknown(&self) -> usize {
        self.connections
            .iter()
            .filter(|entry| entry.value().health_status() == HealthStatus::Unknown)
            .count()
    }

    /// Perform health check on all agents, attempting reconnection for unhealthy ones
    pub async fn health_check_all(&self) {
        debug!("Running health check on all {} agents", self.connections.len());
        
        // Collect agents upfront to release DashMap shard locks before async work
        let agents: Vec<(String, Arc<AgentConnection>)> = self.connections
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().clone()))
            .collect();

        let mut tasks = Vec::new();
        
        for (agent_id, agent) in &agents {
            let agent = agent.clone();
            let agent_id = agent_id.clone();
            tasks.push(tokio::spawn(async move {
                if let Err(e) = agent.check_health().await {
                    debug!("Health check failed for agent {}: {}", agent_id, e);
                }
                (agent_id, agent.health_status())
            }));
        }

        // Wait for all health checks to complete, collect unhealthy agent IDs
        let mut unhealthy_ids = Vec::new();
        for task in tasks {
            match task.await {
                Ok((id, status)) => {
                    if status == HealthStatus::Unhealthy {
                        unhealthy_ids.push(id);
                    }
                }
                Err(e) => {
                    error!("Health check task panicked: {}", e);
                }
            }
        }

        // Attempt reconnection for unhealthy agents (sequentially to avoid stampede)
        for agent_id in &unhealthy_ids {
            if let Err(e) = self.reconnect_agent(agent_id).await {
                debug!("Reconnection failed for agent {}: {}", agent_id, e);
            }
        }

        info!(
            "Health check complete: {} healthy, {} degraded, {} unhealthy, {} unknown",
            self.count_healthy(),
            self.count_degraded(),
            self.count_unhealthy(),
            self.count_unknown()
        );
    }

    /// Create a mTLS gRPC channel to an agent
    async fn create_channel(&self, config: &AgentConfig) -> Result<Channel> {
        debug!("Creating mTLS channel to agent {} at {}", config.id, config.address);

        // Load certificates
        let cert = tokio::fs::read(&config.tls_cert)
            .await
            .map_err(|e| AgentError::Tls(format!("Failed to read client cert: {}", e)))?;

        let key = tokio::fs::read(&config.tls_key)
            .await
            .map_err(|e| AgentError::Tls(format!("Failed to read client key: {}", e)))?;

        let ca = tokio::fs::read(&config.tls_ca)
            .await
            .map_err(|e| AgentError::Tls(format!("Failed to read CA cert: {}", e)))?;

        // Build mTLS config
        let identity = Identity::from_pem(cert, key);
        let ca_cert = Certificate::from_pem(ca);

        let tls_config = ClientTlsConfig::new()
            .identity(identity)
            .ca_certificate(ca_cert)
            .domain_name(&config.tls_domain); // Must match the SAN in agent's certificate

        // Create endpoint
        let endpoint = Channel::from_shared(format!("https://{}", config.address))
            .map_err(|e| AgentError::InvalidConfig(format!("Invalid address: {}", e)))?
            .tls_config(tls_config)
            .map_err(|e| AgentError::Tls(format!("TLS config error: {}", e)))?
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .tcp_keepalive(Some(Duration::from_secs(60)));

        // Connect
        let channel = endpoint
            .connect()
            .await
            .map_err(|e| {
                error!("Failed to connect to agent {} at {}: {:?}", config.id, config.address, e);
                AgentError::ConnectionFailed(format!("Failed to connect to {}: {}", config.address, e))
            })?;

        debug!("✓ mTLS channel established to agent {}", config.id);
        Ok(channel)
    }
}
