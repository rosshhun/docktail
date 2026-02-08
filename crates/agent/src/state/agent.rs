//! Agent state — AgentState struct, shared state type alias.

use dashmap::DashMap;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::RwLock;

use crate::docker::client::DockerClient;
use crate::docker::inventory::ContainerInfo;
use crate::conf::AgentConfig;
use crate::parser::metrics::ParsingMetrics;
use crate::parser::cache::ParserCache;
use super::role::SwarmRole;

pub struct AgentState {
    pub inventory: DashMap<String, ContainerInfo>,
    pub docker: DockerClient,
    pub config: AgentConfig,
    pub metrics: Arc<ParsingMetrics>,
    pub parser_cache: Arc<ParserCache>,
    /// In-memory storage for deployed compose stack files (stack_name -> YAML)
    pub stack_files: Mutex<HashMap<String, String>>,
    /// Cached swarm role — updated on every health check.
    pub swarm_role: RwLock<SwarmRole>,
}

impl AgentState {
    pub fn new(docker: DockerClient, config: AgentConfig) -> Self {
        Self {
            inventory: DashMap::new(),
            docker,
            config,
            metrics: Arc::new(ParsingMetrics::new()),
            parser_cache: Arc::new(ParserCache::new()),
            stack_files: Mutex::new(HashMap::new()),
            swarm_role: RwLock::new(SwarmRole::None),
        }
    }

    /// Refresh the cached swarm role from Docker.
    pub async fn refresh_swarm_role(&self) {
        use crate::docker::client::SwarmInspectResult;
        let role = match self.docker.swarm_inspect().await {
            Ok(SwarmInspectResult::Manager(_)) => SwarmRole::Manager,
            Ok(SwarmInspectResult::Worker) => SwarmRole::Worker,
            Ok(SwarmInspectResult::NotInSwarm) => SwarmRole::None,
            Err(_) => SwarmRole::None,
        };
        *self.swarm_role.write().await = role;
    }

    /// Get the current cached swarm role.
    pub async fn get_swarm_role(&self) -> SwarmRole {
        *self.swarm_role.read().await
    }
}

pub type SharedState = Arc<AgentState>;
