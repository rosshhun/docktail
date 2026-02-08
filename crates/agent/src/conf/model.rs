//! Model — AgentConfig and related structs.

use std::collections::HashMap;
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── AgentConfig Defaults ─────────────────────────────────────

    #[test]
    fn test_agent_config_default_bind_address() {
        let cfg = AgentConfig::default();
        assert_eq!(cfg.bind_address, "0.0.0.0:50051");
    }

    #[test]
    fn test_agent_config_default_tls_paths() {
        let cfg = AgentConfig::default();
        assert_eq!(cfg.tls_cert_path, "certs/agent.crt");
        assert_eq!(cfg.tls_key_path, "certs/agent.key");
        assert_eq!(cfg.tls_ca_path, "certs/ca.crt");
    }

    #[test]
    fn test_agent_config_default_docker_socket_empty() {
        let cfg = AgentConfig::default();
        assert!(cfg.docker_socket.is_empty(), "Default docker_socket should be empty (use system default)");
    }

    #[test]
    fn test_agent_config_default_max_streams() {
        let cfg = AgentConfig::default();
        assert_eq!(cfg.max_concurrent_streams, 100);
    }

    #[test]
    fn test_agent_config_default_no_audit_log() {
        let cfg = AgentConfig::default();
        assert!(cfg.audit_log_path.is_none());
    }

    #[test]
    fn test_agent_config_default_sync_interval() {
        let cfg = AgentConfig::default();
        assert_eq!(cfg.inventory_sync_interval_secs, 2);
    }

    // ── MultilineConfig Defaults ─────────────────────────────────

    #[test]
    fn test_multiline_config_defaults() {
        let ml = MultilineConfig::default();
        assert!(ml.enabled);
        assert_eq!(ml.timeout_ms, 300);
        assert_eq!(ml.max_lines, 50);
        assert!(ml.require_error_anchor);
        assert!(ml.container_overrides.is_empty());
    }

    // ── MultilineConfig Validation ───────────────────────────────

    #[test]
    fn test_multiline_validate_default_passes() {
        let ml = MultilineConfig::default();
        assert!(ml.validate().is_ok());
    }

    #[test]
    fn test_multiline_validate_disabled_allows_zero_timeout() {
        let ml = MultilineConfig {
            enabled: false,
            timeout_ms: 0,
            max_lines: 0,
            ..Default::default()
        };
        assert!(ml.validate().is_ok(), "Disabled multiline should accept zero values");
    }

    #[test]
    fn test_multiline_validate_enabled_rejects_zero_timeout() {
        let ml = MultilineConfig {
            enabled: true,
            timeout_ms: 0,
            max_lines: 50,
            ..Default::default()
        };
        let err = ml.validate().unwrap_err();
        assert!(err.contains("timeout_ms"), "Error should mention timeout_ms: {}", err);
    }

    #[test]
    fn test_multiline_validate_enabled_rejects_zero_max_lines() {
        let ml = MultilineConfig {
            enabled: true,
            timeout_ms: 300,
            max_lines: 0,
            ..Default::default()
        };
        let err = ml.validate().unwrap_err();
        assert!(err.contains("max_lines"), "Error should mention max_lines: {}", err);
    }

    // ── Serialization Round-trip ─────────────────────────────────

    #[test]
    fn test_agent_config_toml_round_trip() {
        let cfg = AgentConfig::default();
        let toml_str = toml::to_string(&cfg).expect("Should serialize to TOML");
        let deserialized: AgentConfig = toml::from_str(&toml_str).expect("Should deserialize from TOML");
        assert_eq!(deserialized.bind_address, cfg.bind_address);
        assert_eq!(deserialized.max_concurrent_streams, cfg.max_concurrent_streams);
        assert_eq!(deserialized.multiline.timeout_ms, cfg.multiline.timeout_ms);
    }

    #[test]
    fn test_agent_config_deserialize_partial_toml() {
        // Only set bind_address; rest should use defaults via #[serde(default)]
        let toml_str = r#"bind_address = "127.0.0.1:9999""#;
        let cfg: AgentConfig = toml::from_str(toml_str).expect("Should accept partial TOML");
        assert_eq!(cfg.bind_address, "127.0.0.1:9999");
        assert_eq!(cfg.max_concurrent_streams, 100); // default
        assert!(cfg.multiline.enabled); // default
    }

    #[test]
    fn test_container_multiline_config_deserialize() {
        let toml_str = r#"
            [container_overrides.myapp]
            enabled = false
            timeout_ms = 500
        "#;
        let ml: MultilineConfig = toml::from_str(toml_str).expect("Should parse container overrides");
        let override_cfg = ml.container_overrides.get("myapp").expect("myapp override should exist");
        assert!(!override_cfg.enabled);
        assert_eq!(override_cfg.timeout_ms, Some(500));
        assert_eq!(override_cfg.max_lines, None);
    }
}
