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

#[cfg(test)]
mod tests {
    use super::*;

    // ── from_file ────────────────────────────────────────────────

    #[test]
    fn test_from_file_valid_toml() {
        let toml_content = r#"
            bind_address = "127.0.0.1:8080"
            tls_cert_path = "/tmp/cert.pem"
            tls_key_path = "/tmp/key.pem"
            tls_ca_path = "/tmp/ca.pem"
            docker_socket = "/var/run/docker.sock"
            max_concurrent_streams = 50
            inventory_sync_interval_secs = 5
        "#;
        let dir = std::env::temp_dir().join("docktail_test_valid_toml.toml");
        std::fs::write(&dir, toml_content).unwrap();

        let config = AgentConfig::from_file(dir.to_str().unwrap()).unwrap();
        assert_eq!(config.bind_address, "127.0.0.1:8080");
        assert_eq!(config.max_concurrent_streams, 50);
        assert_eq!(config.inventory_sync_interval_secs, 5);
        assert_eq!(config.docker_socket, "/var/run/docker.sock");

        std::fs::remove_file(&dir).ok();
    }

    #[test]
    fn test_from_file_missing_fields_uses_defaults() {
        let toml_content = r#"bind_address = "0.0.0.0:9090""#;
        let dir = std::env::temp_dir().join("docktail_test_partial_toml.toml");
        std::fs::write(&dir, toml_content).unwrap();

        let config = AgentConfig::from_file(dir.to_str().unwrap()).unwrap();
        assert_eq!(config.bind_address, "0.0.0.0:9090");
        assert_eq!(config.max_concurrent_streams, 100); // default
        assert!(config.multiline.enabled); // default

        std::fs::remove_file(&dir).ok();
    }

    #[test]
    fn test_from_file_nonexistent_path() {
        let result = AgentConfig::from_file("/nonexistent/path/config.toml");
        assert!(result.is_err());
    }

    #[test]
    fn test_from_file_invalid_toml() {
        let dir = std::env::temp_dir().join("docktail_test_invalid_toml.toml");
        std::fs::write(&dir, "this is {{ not valid }} toml!!!").unwrap();

        let result = AgentConfig::from_file(dir.to_str().unwrap());
        assert!(result.is_err());

        std::fs::remove_file(&dir).ok();
    }

    // ── from_env ─────────────────────────────────────────────────

    #[test]
    fn test_from_env_uses_defaults_when_unset() {
        // Clear relevant env vars to test defaults
        std::env::remove_var("AGENT_BIND_ADDRESS");
        std::env::remove_var("DOCKER_SOCKET");
        std::env::remove_var("AGENT_MAX_STREAMS");
        std::env::remove_var("AGENT_AUDIT_LOG");

        let config = AgentConfig::from_env();
        assert_eq!(config.bind_address, "0.0.0.0:50051");
        assert!(config.docker_socket.is_empty());
        assert_eq!(config.max_concurrent_streams, 100);
        assert!(config.audit_log_path.is_none());
    }

    #[test]
    fn test_from_env_invalid_max_streams_uses_default() {
        std::env::set_var("AGENT_MAX_STREAMS", "not_a_number");
        let config = AgentConfig::from_env();
        assert_eq!(config.max_concurrent_streams, 100);
        std::env::remove_var("AGENT_MAX_STREAMS");
    }

    // ── validate ─────────────────────────────────────────────────

    #[test]
    fn test_validate_empty_bind_address() {
        let mut cfg = AgentConfig::default();
        cfg.bind_address = String::new();
        let err = cfg.validate().unwrap_err();
        assert!(err.contains("bind_address"));
    }

    #[test]
    fn test_validate_zero_max_streams() {
        let mut cfg = AgentConfig::default();
        cfg.max_concurrent_streams = 0;
        let err = cfg.validate().unwrap_err();
        assert!(err.contains("max_concurrent_streams"));
    }

    #[test]
    fn test_validate_zero_sync_interval() {
        let mut cfg = AgentConfig::default();
        cfg.inventory_sync_interval_secs = 0;
        let err = cfg.validate().unwrap_err();
        assert!(err.contains("inventory_sync_interval_secs"));
    }

    #[test]
    fn test_validate_delegates_to_multiline_validate() {
        let mut cfg = AgentConfig::default();
        cfg.multiline.enabled = true;
        cfg.multiline.timeout_ms = 0;
        let err = cfg.validate().unwrap_err();
        assert!(err.contains("timeout_ms"));
    }

    // ── validate_file ────────────────────────────────────────────

    #[test]
    fn test_validate_file_empty_path_returns_error() {
        let cfg = AgentConfig::default();
        let err = cfg.validate_file("", "test cert").unwrap_err();
        assert!(err.contains("not configured"));
    }

    #[test]
    fn test_validate_file_nonexistent_path_returns_error() {
        let cfg = AgentConfig::default();
        let err = cfg.validate_file("/nonexistent/file.pem", "test cert").unwrap_err();
        assert!(err.contains("not found"));
        assert!(err.contains("/nonexistent/file.pem"));
    }

    #[test]
    fn test_validate_file_existing_path_succeeds() {
        let cfg = AgentConfig::default();
        let tmp = std::env::temp_dir().join("docktail_test_validate_file.pem");
        std::fs::write(&tmp, "dummy cert data").unwrap();

        let result = cfg.validate_file(tmp.to_str().unwrap(), "test cert");
        assert!(result.is_ok());

        std::fs::remove_file(&tmp).ok();
    }

    // ── MultilineConfig::from_env ────────────────────────────────

    #[test]
    fn test_multiline_from_env_defaults() {
        std::env::remove_var("AGENT_MULTILINE_ENABLED");
        std::env::remove_var("AGENT_MULTILINE_TIMEOUT_MS");
        std::env::remove_var("AGENT_MULTILINE_MAX_LINES");
        std::env::remove_var("AGENT_MULTILINE_REQUIRE_ERROR_ANCHOR");

        let ml = MultilineConfig::from_env();
        assert!(ml.enabled);
        assert_eq!(ml.timeout_ms, 300);
        assert_eq!(ml.max_lines, 50);
        assert!(ml.require_error_anchor);
        assert!(ml.container_overrides.is_empty());
    }

    #[test]
    fn test_multiline_from_env_invalid_bool_uses_default() {
        std::env::set_var("AGENT_MULTILINE_ENABLED", "not_bool");
        let ml = MultilineConfig::from_env();
        assert!(ml.enabled); // default true
        std::env::remove_var("AGENT_MULTILINE_ENABLED");
    }
}
