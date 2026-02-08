use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClusterConfig {
    pub server: ServerConfig,
    pub agents: AgentRegistryConfig,
    pub security: SecurityConfig,
    pub logging: LoggingConfig,
    pub graphql: GraphQLConfig,
    #[serde(default)]
    pub discovery: DiscoveryConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    pub bind_address: String,
    pub read_timeout_secs: u64,
    pub write_timeout_secs: u64,
    pub max_concurrent_streams: usize,
    pub enable_cors: bool,
    pub cors_origins: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentRegistryConfig {
    #[serde(default)]
    pub static_agents: Vec<AgentConfig>,
    pub health_check_interval: u64,
    pub reconnect_backoff: u64,
    pub max_reconnect_attempts: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentConfig {
    pub id: String,
    pub name: String,
    pub address: String,
    pub tls_cert: String,
    pub tls_key: String,
    pub tls_ca: String,
    /// TLS domain name for certificate verification (defaults to "localhost")
    #[serde(default = "default_tls_domain")]
    pub tls_domain: String,
    #[serde(default)]
    pub labels: HashMap<String, String>,
}

fn default_tls_domain() -> String {
    "localhost".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SecurityConfig {
    // TODO: JWT auth
    pub jwt_secret: Option<String>,
    #[serde(default)]
    pub enable_rbac: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LoggingConfig {
    pub level: String,
    pub format: LogFormat,
    pub output: LogOutput,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    Json,
    Pretty,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogOutput {
    Stdout,
    File { path: String },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GraphQLConfig {
    pub enable_graphiql: bool,
    pub max_depth: usize,
    pub max_complexity: usize,
}

/// Agent auto-discovery configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct DiscoveryConfig {
    /// Enable Swarm-aware auto-discovery via node labels
    pub swarm_discovery: bool,
    /// Node label to identify agent nodes (e.g. "docktail.agent=true")
    pub discovery_label: String,
    /// How often to poll for new agents (seconds)
    pub discovery_interval_secs: u64,
    /// Default gRPC port for discovered agents
    pub agent_port: u16,
    /// Enable HTTP registration endpoint (POST /api/agents/register)
    pub registration_enabled: bool,
    /// TLS cert/key/ca for discovered agents (shared credentials)
    pub tls_cert: Option<String>,
    pub tls_key: Option<String>,
    pub tls_ca: Option<String>,
    /// TLS domain for discovered agents
    #[serde(default = "default_tls_domain")]
    pub tls_domain: String,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            swarm_discovery: false,
            discovery_label: "docktail.agent=true".to_string(),
            discovery_interval_secs: 30,
            agent_port: 50051,
            registration_enabled: false,
            tls_cert: None,
            tls_key: None,
            tls_ca: None,
            tls_domain: "localhost".to_string(),
        }
    }
}

impl ClusterConfig {
    /// Load configuration from cluster.toml and environment variables
    pub fn load() -> Result<Self> {
        // Load .env file if it exists
        dotenvy::dotenv().ok();

        // Start with compile-time defaults as the foundation
        // This ensures that if a key is missing in files/env, we use the default
        let defaults = config::Config::try_from(&ClusterConfig::default())
            .context("Failed to serialize default configuration")?;

        let mut builder = config::Config::builder()
            .add_source(defaults);

        // Layer config files (overrides defaults)
        // Try these locations in order:
        // 1. /etc/docktail/cluster.toml (Docker/production)
        // 2. config/cluster.toml (local development)
        // 3. crates/cluster/config/cluster.toml (workspace root)
        let config_paths = vec![
            "/etc/docktail/cluster",
            "config/cluster",
            "crates/cluster/config/cluster",
        ];

        for path in config_paths {
            builder = builder.add_source(config::File::with_name(path).required(false));
        }

        // Layer environment variables (overrides everything)
        // Use double underscore for nested keys: CLUSTER_SERVER__BIND_ADDRESS
        builder = builder.add_source(
            config::Environment::with_prefix("CLUSTER")
                .separator("__")
                .try_parsing(true),
        );

        builder
            .build()
            .context("Failed to build configuration")?
            .try_deserialize()
            .context("Failed to deserialize configuration")
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<()> {
        // Validate bind address
        self.server.bind_address.parse::<std::net::SocketAddr>()
            .context("Invalid bind_address")?;

        // Validate agent configurations
        for agent in &self.agents.static_agents {
            // Check that all TLS cert/key/ca files exist
            let tls_files = [
                ("cert", &agent.tls_cert),
                ("key", &agent.tls_key),
                ("ca", &agent.tls_ca),
            ];
            for (label, path) in &tls_files {
                let p = std::path::Path::new(path);
                if !p.exists() {
                    anyhow::bail!(
                        "Agent '{}' TLS {} file not found: {} (resolved: {})",
                        agent.id,
                        label,
                        path,
                        p.canonicalize()
                            .map(|c| c.display().to_string())
                            .unwrap_or_else(|_| "unresolvable".to_string())
                    );
                }
            }
        }

        Ok(())
    }
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                bind_address: "0.0.0.0:8080".to_string(),
                read_timeout_secs: 30,
                write_timeout_secs: 30,
                max_concurrent_streams: 1000,
                enable_cors: true,
                cors_origins: vec![
                    "http://localhost:3000".to_string(),
                    "http://localhost:5173".to_string(),
                ],
            },
            agents: AgentRegistryConfig {
                static_agents: vec![],
                health_check_interval: 30,
                reconnect_backoff: 5,
                max_reconnect_attempts: 3,
            },
            security: SecurityConfig {
                jwt_secret: None,
                enable_rbac: false,
            },
            logging: LoggingConfig {
                level: "info,cluster=debug".to_string(),
                format: LogFormat::Pretty,
                output: LogOutput::Stdout,
            },
            graphql: GraphQLConfig {
                enable_graphiql: false,
                max_depth: 15,
                max_complexity: 1000,
            },
            discovery: DiscoveryConfig::default(),
        }
    }
}
