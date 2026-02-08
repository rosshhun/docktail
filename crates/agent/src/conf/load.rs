//! Load — config loading from file and environment variables.

use std::path::Path;
use std::fs::File;
use std::io::Read;

use super::model::{AgentConfig, MultilineConfig};

impl AgentConfig {
    /// Load configuration from file or environment variables
    /// Priority: Environment Variables > Config File > Defaults
    pub fn load() -> Result<Self, Box<dyn std::error::Error>> {
        let config_path = std::env::var("AGENT_CONFIG_FILE")
            .unwrap_or_else(|_| "/etc/docktail/agent.toml".to_string());
        
        let mut config = if Path::new(&config_path).exists() {
            tracing::info!("Loading configuration from: {}", config_path);
            Self::from_file(&config_path)?
        } else {
            tracing::info!("Config file not found at {}, using environment variables", config_path);
            Self::from_env()
        };
        
        // Environment variables override file config for critical settings
        if let Ok(bind) = std::env::var("AGENT_BIND_ADDRESS") {
            config.bind_address = bind;
        }
        if let Ok(socket) = std::env::var("DOCKER_SOCKET") {
            config.docker_socket = socket;
        }
        if let Ok(cert) = std::env::var("AGENT_TLS_CERT") {
            config.tls_cert_path = cert;
        }
        if let Ok(key) = std::env::var("AGENT_TLS_KEY") {
            config.tls_key_path = key;
        }
        if let Ok(ca) = std::env::var("AGENT_TLS_CA") {
            config.tls_ca_path = ca;
        }
        
        Ok(config)
    }

    /// Load configuration from TOML file
    pub fn from_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let mut file = File::open(path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        
        let config: AgentConfig = toml::from_str(&contents)?;
        Ok(config)
    }

    /// Load configuration from environment variables with sensible defaults
    pub fn from_env() -> Self {
        Self {
            bind_address: std::env::var("AGENT_BIND_ADDRESS")
                .unwrap_or_else(|_| "0.0.0.0:50051".to_string()),
            tls_cert_path: std::env::var("AGENT_TLS_CERT")
                .unwrap_or_else(|_| "certs/agent.crt".to_string()),
            tls_key_path: std::env::var("AGENT_TLS_KEY")
                .unwrap_or_else(|_| "certs/agent.key".to_string()),
            tls_ca_path: std::env::var("AGENT_TLS_CA")
                .unwrap_or_else(|_| "certs/ca.crt".to_string()),
            docker_socket: std::env::var("DOCKER_SOCKET")
                .unwrap_or_else(|_| "".to_string()),
            max_concurrent_streams: std::env::var("AGENT_MAX_STREAMS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(100),
            audit_log_path: std::env::var("AGENT_AUDIT_LOG").ok(),
            multiline: MultilineConfig::from_env(),
            inventory_sync_interval_secs: std::env::var("AGENT_INVENTORY_SYNC_INTERVAL")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(2),
        }
    }

    /// Validate that all required files exist and configuration values are sane
    pub fn validate(&self) -> Result<(), String> {
        if self.bind_address.is_empty() {
            return Err("bind_address must not be empty".to_string());
        }
        if self.max_concurrent_streams == 0 {
            return Err("max_concurrent_streams must be > 0".to_string());
        }
        if self.inventory_sync_interval_secs == 0 {
            return Err("inventory_sync_interval_secs must be > 0".to_string());
        }
        self.multiline.validate()?;

        self.validate_file(&self.tls_cert_path, "TLS certificate")?;
        self.validate_file(&self.tls_key_path, "TLS key")?;
        self.validate_file(&self.tls_ca_path, "CA certificate")?;
        Ok(())
    }

    fn validate_file(&self, path: &str, name: &str) -> Result<(), String> {
        if path.is_empty() {
            return Err(format!("{} path is not configured (empty string)", name));
        }
        if !Path::new(path).exists() {
            return Err(format!("{} not found at: {}", name, path));
        }
        Ok(())
    }
}

impl MultilineConfig {
    /// Load multiline configuration from environment variables
    pub fn from_env() -> Self {
        Self {
            enabled: std::env::var("AGENT_MULTILINE_ENABLED")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(true),
            timeout_ms: std::env::var("AGENT_MULTILINE_TIMEOUT_MS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(300),
            max_lines: std::env::var("AGENT_MULTILINE_MAX_LINES")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(50),
            require_error_anchor: std::env::var("AGENT_MULTILINE_REQUIRE_ERROR_ANCHOR")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(true),
            container_overrides: std::collections::HashMap::new(),
        }
    }
}
