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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_swarm_role_as_str_manager() {
        assert_eq!(SwarmRole::Manager.as_str(), "manager");
    }

    #[test]
    fn test_swarm_role_as_str_worker() {
        assert_eq!(SwarmRole::Worker.as_str(), "worker");
    }

    #[test]
    fn test_swarm_role_as_str_none() {
        assert_eq!(SwarmRole::None.as_str(), "none");
    }

    #[test]
    fn test_swarm_role_equality() {
        assert_eq!(SwarmRole::Manager, SwarmRole::Manager);
        assert_ne!(SwarmRole::Manager, SwarmRole::Worker);
        assert_ne!(SwarmRole::Worker, SwarmRole::None);
    }

    #[test]
    fn test_swarm_role_clone() {
        let role = SwarmRole::Manager;
        let cloned = role;
        assert_eq!(role, cloned);
    }

    #[test]
    fn test_swarm_role_debug() {
        let debug_str = format!("{:?}", SwarmRole::Manager);
        assert!(debug_str.contains("Manager"));
    }
}
