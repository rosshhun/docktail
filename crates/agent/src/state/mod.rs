//! State module — agent state and swarm role.

pub mod agent;
pub mod role;

pub use agent::{AgentState, SharedState};
pub use role::SwarmRole;
