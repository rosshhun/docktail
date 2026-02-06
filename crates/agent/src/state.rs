use dashmap::DashMap;
use std::sync::Arc;
use crate::docker::client::DockerClient;
use crate::docker::inventory::ContainerInfo;
use crate::config::AgentConfig;
use crate::parser::metrics::ParsingMetrics;
use crate::parser::cache::ParserCache;

pub struct AgentState {
    pub inventory: DashMap<String, ContainerInfo>,
    pub docker: DockerClient,
    pub config: AgentConfig,
    pub metrics: Arc<ParsingMetrics>,
    pub parser_cache: Arc<ParserCache>,
}

impl AgentState {
    pub fn new(docker: DockerClient, config: AgentConfig) -> Self {
        Self {
            inventory: DashMap::new(),
            docker,
            config,
            metrics: Arc::new(ParsingMetrics::new()),
            parser_cache: Arc::new(ParserCache::new()),
        }
    }
}

pub type SharedState = Arc<AgentState>;