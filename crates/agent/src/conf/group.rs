//! Group — per-container multiline configuration resolution.

use std::collections::HashMap;
use super::model::MultilineConfig;

impl MultilineConfig {
    /// Get effective config for a container, considering overrides from config file and Docker labels
    /// Priority: Base Config -> File Override (by container name) -> Label Override (highest)
    pub fn for_container(&self, container_name: &str, labels: &HashMap<String, String>) -> Self {
        let mut config = self.clone();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conf::model::ContainerMultilineConfig;

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
        assert_eq!(result.max_lines, 50);
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
        assert!(result.enabled);
        assert_eq!(result.timeout_ms, 1000);
    }

    #[test]
    fn test_for_container_invalid_label_ignored() {
        let base = MultilineConfig::default();
        let mut labels = HashMap::new();
        labels.insert("docktail.multiline.timeout_ms".to_string(), "not_a_number".to_string());

        let result = base.for_container("any", &labels);
        assert_eq!(result.timeout_ms, 300);
    }
}
