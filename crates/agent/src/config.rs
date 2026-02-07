use std::path::Path;
use std::sync::Arc;
use std::fs::File;
use std::io::{BufReader, Read};
use std::collections::HashMap;
use rustls::ServerConfig;
use rustls::pki_types::CertificateDer;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentConfig {
    pub bind_address: String,
    pub tls_cert_path: String,
    pub tls_key_path: String,
    pub tls_ca_path: String,
    pub docker_socket: String,
    pub max_concurrent_streams: usize,
    pub audit_log_path: Option<String>,
    pub multiline: MultilineConfig,
    pub inventory_sync_interval_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MultilineConfig {
    pub enabled: bool,
    pub timeout_ms: u64,
    pub max_lines: usize,
    pub require_error_anchor: bool,
    pub container_overrides: HashMap<String, ContainerMultilineConfig>,
}

/// Per-container multiline override
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerMultilineConfig {
    pub enabled: bool,
    pub timeout_ms: Option<u64>,
    pub max_lines: Option<usize>,
}

impl AgentConfig {
    /// Load configuration from file or environment variables
    /// Priority: Environment Variables > Config File > Defaults
    /// 
    /// Environment variables always override config file settings for critical values
    pub fn load() -> Result<Self, Box<dyn std::error::Error>> {
        // Try to load from file first
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
        // Validate configuration values first (fast, no I/O)
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

        // Validate file existence (I/O)
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

    /// Build a rustls ServerConfig with mTLS from the configuration
    pub fn build_rustls_config(&self) -> Result<Arc<ServerConfig>, Box<dyn std::error::Error>> {
        // Load certificates
        let cert_file = File::open(&self.tls_cert_path)?;
        let mut cert_reader = BufReader::new(cert_file);
        let certs: Vec<CertificateDer> = rustls_pemfile::certs(&mut cert_reader)
            .collect::<Result<Vec<_>, _>>()?;

        // Load private key
        let key_file = File::open(&self.tls_key_path)?;
        let mut key_reader = BufReader::new(key_file);
        let key = rustls_pemfile::private_key(&mut key_reader)?
            .ok_or("No private key found in file")?;

        // Load CA certificate for client verification (mTLS)
        let ca_file = File::open(&self.tls_ca_path)?;
        let mut ca_reader = BufReader::new(ca_file);
        let ca_certs: Vec<CertificateDer> = rustls_pemfile::certs(&mut ca_reader)
            .collect::<Result<Vec<_>, _>>()?;

        // Create client certificate verifier
        let mut root_store = rustls::RootCertStore::empty();
        for cert in ca_certs {
            root_store.add(cert)?;
        }
        
        let client_verifier = rustls::server::WebPkiClientVerifier::builder(
            Arc::new(root_store)
        ).build()?;

        // Build server config with mTLS
        let mut config = ServerConfig::builder()
            .with_client_cert_verifier(client_verifier)
            .with_single_cert(certs, key)?;

        // Configure ALPN to support HTTP/2 (required for gRPC)
        config.alpn_protocols = vec![b"h2".to_vec()];

        Ok(Arc::new(config))
    }
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            bind_address: "0.0.0.0:50051".to_string(),
            tls_cert_path: "certs/agent.crt".to_string(),
            tls_key_path: "certs/agent.key".to_string(),
            tls_ca_path: "certs/ca.crt".to_string(),
            docker_socket: "".to_string(),
            max_concurrent_streams: 100,
            audit_log_path: None,
            multiline: MultilineConfig::default(),
            inventory_sync_interval_secs: 2,
        }
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
            container_overrides: HashMap::new(),
        }
    }

    /// Get effective config for a container, considering overrides from config file and Docker labels
    /// Priority: Base Config → File Override (by container name) → Label Override (highest)
    /// 
    /// # Arguments
    /// * `container_name` - The container name or ID to look up in container_overrides
    /// * `labels` - The container's Docker labels for runtime overrides
    pub fn for_container(&self, container_name: &str, labels: &HashMap<String, String>) -> Self {
        let mut config = self.clone();
        // The returned config is for a single container — it never needs the
        // full overrides map.  Clearing it avoids cloning O(N) entries per stream.
        config.container_overrides.clear();

        // 1. Check for container-specific override in config file (medium priority)
        if let Some(file_override) = self.container_overrides.get(container_name) {
            config.enabled = file_override.enabled;
            if let Some(timeout) = file_override.timeout_ms {
                config.timeout_ms = timeout;
            }
            if let Some(max) = file_override.max_lines {
                config.max_lines = max;
            }
        }

        // 2. Check for container-specific override via Docker labels (highest priority)
        if let Some(enabled_str) = labels.get("docktail.multiline.enabled") {
            if let Ok(enabled) = enabled_str.parse::<bool>() {
                config.enabled = enabled;
            }
        }

        if let Some(timeout_str) = labels.get("docktail.multiline.timeout_ms") {
            if let Ok(timeout) = timeout_str.parse::<u64>() {
                config.timeout_ms = timeout;
            }
        }

        if let Some(max_str) = labels.get("docktail.multiline.max_lines") {
            if let Ok(max) = max_str.parse::<usize>() {
                config.max_lines = max;
            }
        }

        if let Some(anchor_str) = labels.get("docktail.multiline.require_error_anchor") {
            if let Ok(anchor) = anchor_str.parse::<bool>() {
                config.require_error_anchor = anchor;
            }
        }

        config
    }
}

impl MultilineConfig {
    /// Validate multiline configuration values
    pub fn validate(&self) -> Result<(), String> {
        if self.enabled {
            if self.timeout_ms == 0 {
                return Err("multiline.timeout_ms must be > 0 when multiline is enabled".to_string());
            }
            if self.max_lines == 0 {
                return Err("multiline.max_lines must be > 0 when multiline is enabled".to_string());
            }
        }
        Ok(())
    }
}

impl Default for MultilineConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            timeout_ms: 300,
            max_lines: 50,
            require_error_anchor: true,
            container_overrides: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_config() -> AgentConfig {
        AgentConfig {
            // Use paths that don't need to exist for non-file validation tests
            tls_cert_path: String::new(),
            tls_key_path: String::new(),
            tls_ca_path: String::new(),
            ..AgentConfig::default()
        }
    }

    // ── AgentConfig validation ──────────────────────────────────

    #[test]
    fn test_validate_empty_bind_address() {
        let mut config = valid_config();
        config.bind_address = "".to_string();
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("bind_address"));
    }

    #[test]
    fn test_validate_zero_max_concurrent_streams() {
        let mut config = valid_config();
        config.max_concurrent_streams = 0;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("max_concurrent_streams"));
    }

    #[test]
    fn test_validate_zero_inventory_sync_interval() {
        let mut config = valid_config();
        config.inventory_sync_interval_secs = 0;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("inventory_sync_interval"));
    }

    // ── MultilineConfig validation ──────────────────────────────

    #[test]
    fn test_validate_multiline_defaults_ok() {
        let config = MultilineConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_multiline_zero_timeout_when_enabled() {
        let config = MultilineConfig {
            enabled: true,
            timeout_ms: 0,
            ..MultilineConfig::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_multiline_zero_max_lines_when_enabled() {
        let config = MultilineConfig {
            enabled: true,
            max_lines: 0,
            ..MultilineConfig::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_multiline_zero_values_ok_when_disabled() {
        let config = MultilineConfig {
            enabled: false,
            timeout_ms: 0,
            max_lines: 0,
            ..MultilineConfig::default()
        };
        assert!(config.validate().is_ok());
    }

    // ── for_container override priority ─────────────────────────

    #[test]
    fn test_for_container_no_overrides() {
        let base = MultilineConfig::default();
        let result = base.for_container("unknown", &HashMap::new());
        assert_eq!(result.timeout_ms, 300);
        assert_eq!(result.max_lines, 50);
    }

    #[test]
    fn test_for_container_file_override() {
        let mut base = MultilineConfig::default();
        base.container_overrides.insert("myapp".to_string(), ContainerMultilineConfig {
            enabled: false,
            timeout_ms: Some(500),
            max_lines: None,
        });

        let result = base.for_container("myapp", &HashMap::new());
        assert!(!result.enabled);
        assert_eq!(result.timeout_ms, 500);
        assert_eq!(result.max_lines, 50); // Unchanged
    }

    #[test]
    fn test_for_container_label_overrides_file() {
        let mut base = MultilineConfig::default();
        base.container_overrides.insert("myapp".to_string(), ContainerMultilineConfig {
            enabled: false,
            timeout_ms: Some(500),
            max_lines: None,
        });

        let mut labels = HashMap::new();
        labels.insert("docktail.multiline.enabled".to_string(), "true".to_string());
        labels.insert("docktail.multiline.timeout_ms".to_string(), "1000".to_string());

        let result = base.for_container("myapp", &labels);
        assert!(result.enabled); // Label overrides file
        assert_eq!(result.timeout_ms, 1000); // Label overrides file
    }

    #[test]
    fn test_for_container_invalid_label_ignored() {
        let base = MultilineConfig::default();
        let mut labels = HashMap::new();
        labels.insert("docktail.multiline.timeout_ms".to_string(), "not_a_number".to_string());

        let result = base.for_container("any", &labels);
        assert_eq!(result.timeout_ms, 300); // Unchanged, invalid label ignored
    }

    // ── Default values ──────────────────────────────────────────

    #[test]
    fn test_agent_config_defaults() {
        let config = AgentConfig::default();
        assert_eq!(config.bind_address, "0.0.0.0:50051");
        assert_eq!(config.max_concurrent_streams, 100);
        assert_eq!(config.inventory_sync_interval_secs, 2);
        assert!(config.multiline.enabled);
    }
}