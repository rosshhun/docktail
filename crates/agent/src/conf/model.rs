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
