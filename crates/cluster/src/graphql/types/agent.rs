use async_graphql::{SimpleObject, Enum};
use crate::agent::{HealthStatus as AgentHealthStatus, AgentSource};
use std::sync::Arc;

/// Agent status in GraphQL
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum AgentStatus {
    Healthy,
    Degraded,
    Unhealthy,
    Unknown,
}

impl From<AgentHealthStatus> for AgentStatus {
    fn from(status: AgentHealthStatus) -> Self {
        match status {
            AgentHealthStatus::Healthy => AgentStatus::Healthy,
            AgentHealthStatus::Degraded => AgentStatus::Degraded,
            AgentHealthStatus::Unhealthy => AgentStatus::Unhealthy,
            AgentHealthStatus::Unknown => AgentStatus::Unknown,
        }
    }
}

/// How the agent was added to the cluster
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum AgentSourceGql {
    /// Manually configured in cluster.toml
    Static,
    /// Auto-discovered via Swarm node labels
    Discovered,
    /// Registered via HTTP API
    Registered,
}

impl From<AgentSource> for AgentSourceGql {
    fn from(source: AgentSource) -> Self {
        match source {
            AgentSource::Static => AgentSourceGql::Static,
            AgentSource::Discovered => AgentSourceGql::Discovered,
            AgentSource::Registered => AgentSourceGql::Registered,
        }
    }
}

/// Label (key-value pair)
#[derive(Debug, Clone, SimpleObject)]
pub struct Label {
    pub key: String,
    pub value: String,
}

/// Helper to build an AgentView from agent info (used by schema.rs)
pub fn agent_view_from_connection(conn: &Arc<crate::agent::AgentConnection>, last_seen: chrono::DateTime<chrono::Utc>) -> AgentView {
    AgentView {
        id: conn.info.id.clone(),
        name: conn.info.name.clone(),
        address: conn.info.address.clone(),
        status: conn.health_status().into(),
        source: conn.source.into(),
        last_seen,
        labels: conn.info.labels.iter().map(|(k, v)| Label {
            key: k.clone(),
            value: v.clone(),
        }).collect(),
        version: conn.info.version.clone(),
    }
}

/// Simple agent view without connection (for listing)
#[derive(Debug, Clone, SimpleObject)]
pub struct AgentView {
    pub id: String,
    pub name: String,
    pub address: String,
    pub status: AgentStatus,
    /// How this agent was added (static config, discovered, or registered)
    pub source: AgentSourceGql,
    pub last_seen: chrono::DateTime<chrono::Utc>,
    pub labels: Vec<Label>,
    pub version: Option<String>,
}

/// Agent health summary
#[derive(Debug, Clone, SimpleObject)]
pub struct AgentHealthSummary {
    pub total: i32,
    pub healthy: i32,
    pub degraded: i32,
    pub unhealthy: i32,
    pub unknown: i32,
}

/// Agent discovery status
#[derive(Debug, Clone, SimpleObject)]
pub struct DiscoveryStatusView {
    /// Whether Swarm-aware auto-discovery is enabled
    pub swarm_discovery_enabled: bool,
    /// Whether the HTTP registration API is enabled
    pub registration_enabled: bool,
    /// Node label used for Swarm discovery
    pub discovery_label: String,
    /// Discovery polling interval (seconds)
    pub discovery_interval_secs: i32,
    /// Default agent gRPC port for discovered agents
    pub agent_port: i32,
    /// Total agents in pool
    pub total_agents: i32,
    /// Agents from static config
    pub static_agents: i32,
    /// Agents from Swarm discovery
    pub discovered_agents: i32,
    /// Agents from HTTP registration
    pub registered_agents: i32,
}

/// Real-time agent health event (for subscriptions)
#[derive(Debug, Clone, SimpleObject)]
pub struct AgentHealthEvent {
    /// Agent ID
    pub agent_id: String,
    
    /// Health status
    pub status: AgentStatus,
    
    /// Human-readable status message
    pub message: String,
    
    /// Timestamp of the health check
    pub timestamp: i64,
    
    /// Additional metadata (parsing metrics, etc.)
    pub metadata: Vec<MetadataEntry>,
}

/// Metadata key-value pair
#[derive(Debug, Clone, SimpleObject)]
pub struct MetadataEntry {
    pub key: String,
    pub value: String,
}
