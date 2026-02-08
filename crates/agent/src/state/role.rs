//! Role — SwarmRole enum and helpers.

/// The swarm role of this agent's Docker node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwarmRole {
    /// This node is a swarm manager.
    Manager,
    /// This node is in a swarm but is a worker.
    Worker,
    /// This node is not part of any swarm.
    None,
}

impl SwarmRole {
    /// String representation for gRPC health-check metadata.
    pub fn as_str(&self) -> &'static str {
        match self {
            SwarmRole::Manager => "manager",
            SwarmRole::Worker => "worker",
            SwarmRole::None => "none",
        }
    }
}
