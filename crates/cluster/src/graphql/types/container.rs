// Container GraphQL types - Phase 3

use async_graphql::{Context, Enum, InputObject, Object, SimpleObject};
use crate::agent::client::ContainerInspectRequest;
use crate::state::AppState;
use crate::error::ApiError;
use super::agent::Label;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Container state enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum ContainerState {
    Running,
    Paused,
    Exited,
    Created,
    Restarting,
    Removing,
    Dead,
    Unknown,
}

impl From<&str> for ContainerState {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "running" => ContainerState::Running,
            "paused" => ContainerState::Paused,
            "exited" => ContainerState::Exited,
            "created" => ContainerState::Created,
            "restarting" => ContainerState::Restarting,
            "removing" => ContainerState::Removing,
            "dead" => ContainerState::Dead,
            _ => ContainerState::Unknown,
        }
    }
}

/// Port mapping information
#[derive(Debug, Clone, SimpleObject)]
pub struct PortMapping {
    /// Container port
    pub container_port: i32,
    
    /// Protocol (tcp, udp, sctp)
    pub protocol: String,
    
    /// Host IP (if mapped to host)
    pub host_ip: Option<String>,
    
    /// Host port (if mapped to host)
    pub host_port: Option<i32>,
}

/// Container GraphQL type (lightweight info)
#[derive(Debug, Clone)]
pub struct Container {
    /// Container ID (64-char hash)
    pub id: String,
    
    /// Agent ID this container belongs to
    pub agent_id: String,
    
    /// Container name (without leading /)
    pub name: String,
    
    /// Image name with tag
    pub image: String,
    
    /// Current state
    pub state: ContainerState,
    
    /// Human-readable status
    pub status: String,
    
    /// Container labels
    pub labels_map: std::collections::HashMap<String, String>,
    
    /// Creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
    
    /// Log driver (if available)
    pub log_driver: Option<String>,
    
    /// Port mappings
    pub ports: Vec<PortMapping>,

    /// Detailed state info (from inspect, may be None for list-only queries)
    pub state_info: Option<ContainerStateInfoGql>,
}

#[Object]
impl Container {
    /// Container ID (64-char hash)
    async fn id(&self) -> &str {
        &self.id
    }

    /// Agent ID this container belongs to
    async fn agent_id(&self) -> &str {
        &self.agent_id
    }

    /// Container name (without leading /)
    async fn name(&self) -> &str {
        &self.name
    }

    /// Image name with tag
    async fn image(&self) -> &str {
        &self.image
    }

    /// Current state
    async fn state(&self) -> ContainerState {
        self.state
    }

    /// Human-readable status
    async fn status(&self) -> &str {
        &self.status
    }

    /// Creation timestamp
    async fn created_at(&self) -> chrono::DateTime<chrono::Utc> {
        self.created_at
    }

    /// Log driver (if available)
    async fn log_driver(&self) -> Option<&str> {
        self.log_driver.as_deref()
    }

    /// Container labels as key-value pairs
    async fn labels(&self) -> Vec<Label> {
        self.labels_map
            .iter()
            .map(|(k, v)| Label {
                key: k.clone(),
                value: v.clone(),
            })
            .collect()
    }

    /// Port mappings
    async fn ports(&self) -> Vec<PortMapping> {
        self.ports.clone()
    }

    /// Detailed state information (OOM killed, PID, exit code, timestamps, restart count)
    async fn state_info(&self) -> Option<&ContainerStateInfoGql> {
        self.state_info.as_ref()
    }

    /// Get detailed information about this container.
    /// Results are cached per-request to avoid N+1 gRPC calls when multiple
    /// containers in the same query request details.
    async fn details(&self, ctx: &Context<'_>) -> async_graphql::Result<Option<ContainerDetails>> {
        let state = ctx.data::<AppState>()?;
        
        // Check per-request cache first
        let cache = ctx.data::<ContainerDetailsCache>()?;
        {
            let guard = cache.0.lock().await;
            if let Some(cached) = guard.get(&self.id) {
                return Ok(cached.clone());
            }
        }
        
        // Get the agent connection
        let agent = state.agent_pool.get_agent(&self.agent_id)
            .ok_or_else(|| ApiError::AgentNotFound(self.agent_id.clone()).extend())?;
        
        // Lock, clone, drop pattern to avoid head-of-line blocking
        let mut client = {
            let guard = agent.client.lock().await;
            guard.clone()
        };
        
        // Network call happens on the clone (no lock held)
        let response = client
            .inspect_container(ContainerInspectRequest {
                container_id: self.id.clone(),
            })
            .await?;
        
        // Convert to ContainerDetails
        let result = if let Some(details) = response.details {
            Some(ContainerDetails {
                command: details.command,
                working_dir: details.working_dir,
                env: details.env,
                exposed_ports: details.exposed_ports,
                mounts: details.mounts.into_iter().map(|m| VolumeMount {
                    source: m.source,
                    destination: m.destination,
                    mode: m.mode,
                    mount_type: if m.mount_type.is_empty() { None } else { Some(m.mount_type) },
                    propagation: if m.propagation.is_empty() { None } else { Some(m.propagation) },
                }).collect(),
                networks: details.networks.into_iter().map(|n| NetworkInfo {
                    network_name: n.network_name,
                    ip_address: n.ip_address,
                    gateway: n.gateway,
                    mac_address: if n.mac_address.is_empty() { None } else { Some(n.mac_address) },
                }).collect(),
                limits: details.limits.map(|l| ResourceLimits {
                    memory_limit_bytes: l.memory_limit_bytes,
                    cpu_limit: l.cpu_limit,
                    pids_limit: l.pids_limit,
                }),
                entrypoint: details.entrypoint,
                hostname: if details.hostname.is_empty() { None } else { Some(details.hostname) },
                user: if details.user.is_empty() { None } else { Some(details.user) },
                restart_policy: details.restart_policy.map(|rp| RestartPolicyGql {
                    name: rp.name,
                    max_retry_count: rp.max_retry_count,
                }),
                network_mode: if details.network_mode.is_empty() { None } else { Some(details.network_mode) },
                healthcheck: details.healthcheck.map(|hc| HealthcheckConfigGql {
                    test: hc.test,
                    interval_ns: hc.interval_ns,
                    timeout_ns: hc.timeout_ns,
                    retries: hc.retries,
                    start_period_ns: hc.start_period_ns,
                }),
                platform: if details.platform.is_empty() { None } else { Some(details.platform) },
                runtime: if details.runtime.is_empty() { None } else { Some(details.runtime) },
            })
        } else {
            None
        };
        
        // Store in cache for this request
        {
            let mut guard = cache.0.lock().await;
            guard.insert(self.id.clone(), result.clone());
        }
        
        Ok(result)
    }
}

/// Detailed container information
#[derive(Debug, Clone, SimpleObject)]
pub struct ContainerDetails {
    /// Command that was run
    pub command: Vec<String>,
    
    /// Working directory
    pub working_dir: String,
    
    /// Environment variables
    pub env: Vec<String>,
    
    /// Exposed ports
    pub exposed_ports: Vec<String>,
    
    /// Volume mounts
    pub mounts: Vec<VolumeMount>,
    
    /// Network information
    pub networks: Vec<NetworkInfo>,
    
    /// Resource limits
    pub limits: Option<ResourceLimits>,

    /// Entrypoint command
    pub entrypoint: Vec<String>,

    /// Container hostname
    pub hostname: Option<String>,

    /// User the container process runs as
    pub user: Option<String>,

    /// Restart policy
    pub restart_policy: Option<RestartPolicyGql>,

    /// Network mode (bridge, host, none, container:<id>)
    pub network_mode: Option<String>,

    /// Healthcheck configuration
    pub healthcheck: Option<HealthcheckConfigGql>,

    /// Platform (e.g., "linux")
    pub platform: Option<String>,

    /// Container runtime (e.g., "runc")
    pub runtime: Option<String>,
}

/// Volume mount information
#[derive(Debug, Clone, SimpleObject)]
pub struct VolumeMount {
    pub source: String,
    pub destination: String,
    pub mode: String,
    /// Mount type: "bind", "volume", or "tmpfs"
    pub mount_type: Option<String>,
    /// Mount propagation mode
    pub propagation: Option<String>,
}

/// Network information
#[derive(Debug, Clone, SimpleObject)]
pub struct NetworkInfo {
    pub network_name: String,
    pub ip_address: String,
    pub gateway: String,
    /// MAC address on this network
    pub mac_address: Option<String>,
}

/// Resource limits
#[derive(Debug, Clone, SimpleObject)]
pub struct ResourceLimits {
    pub memory_limit_bytes: Option<i64>,
    pub cpu_limit: Option<f64>,
    pub pids_limit: Option<i64>,
}

/// Detailed container state information
#[derive(Debug, Clone, SimpleObject)]
pub struct ContainerStateInfoGql {
    /// Whether the container was killed due to OOM
    pub oom_killed: bool,
    /// Host PID of the container's main process
    pub pid: i64,
    /// Exit code of the last run
    pub exit_code: i32,
    /// When the container last started (RFC3339)
    pub started_at: String,
    /// When the container last finished (RFC3339)
    pub finished_at: String,
    /// Number of times the container has restarted
    pub restart_count: i32,
}

/// Container restart policy
#[derive(Debug, Clone, SimpleObject)]
pub struct RestartPolicyGql {
    /// Policy name: "no", "always", "unless-stopped", "on-failure"
    pub name: String,
    /// Maximum retry count (for "on-failure" policy)
    pub max_retry_count: i32,
}

/// Container healthcheck configuration
#[derive(Debug, Clone, SimpleObject)]
pub struct HealthcheckConfigGql {
    /// Test command
    pub test: Vec<String>,
    /// Interval between checks (nanoseconds)
    pub interval_ns: i64,
    /// Timeout for each check (nanoseconds)
    pub timeout_ns: i64,
    /// Retries before marking unhealthy
    pub retries: i32,
    /// Grace period before checks begin (nanoseconds)
    pub start_period_ns: i64,
}

/// Per-request cache for container details to prevent N+1 gRPC calls.
/// Insert this into the GraphQL context data for each request.
pub struct ContainerDetailsCache(pub Arc<Mutex<HashMap<String, Option<ContainerDetails>>>>);

impl ContainerDetailsCache {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(HashMap::new())))
    }
}

impl Default for ContainerDetailsCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Filter for container queries
#[derive(Debug, Clone, InputObject)]
pub struct ContainerFilter {
    /// Filter by container state
    pub state: Option<ContainerState>,
    
    /// Include stopped containers (default: false)
    pub include_stopped: Option<bool>,
    
    /// Filter by label key-value pairs
    pub labels: Option<Vec<LabelFilter>>,
    
    /// Filter by name pattern (substring match)
    pub name_pattern: Option<String>,
    
    /// Filter by image pattern (substring match)
    pub image_pattern: Option<String>,
    
    /// Limit number of results (must be > 0 if provided)
    pub limit: Option<i32>,
}

/// Label filter for matching key-value pairs
#[derive(Debug, Clone, InputObject)]
pub struct LabelFilter {
    pub key: String,
    pub value: Option<String>,
}
