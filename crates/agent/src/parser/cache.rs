use dashmap::DashMap;
use super::LogFormat;

/// State for a specific container's parser
#[derive(Debug, Clone, Copy)]
pub struct ContainerState {
    pub format: LogFormat,
    pub is_enabled: bool,
}

/// Per-container parser cache
/// 
/// Caches the detected format and parser instance for each container.
/// This avoids re-detecting the format on every log line.
#[derive(Debug)]
pub struct ParserCache {
    /// Single Map: container_id → State
    /// Merging them ensures atomic updates and single-lookup efficiency
    state: DashMap<String, ContainerState>,
}

impl ParserCache {
    pub fn new() -> Self {
        Self {
            state: DashMap::new(),
        }
    }
 
    /// Get the cached format for a container (if enabled)
    pub fn get_format(&self, container_id: &str) -> Option<LogFormat> {
        self.state.get(container_id).and_then(|r| {
            if r.is_enabled {
                Some(r.format)
            } else {
                // If parsing is explicitly disabled, we might want to return 
                // PlainText or None depending on caller logic. 
                // Usually returning None forces the caller to fallback.
                None 
            }
        })
    }

    /// Check if parsing is disabled for a container
    /// Returns true if the container is in the cache but parsing is disabled
    /// Returns false if the container is not in cache or parsing is enabled
    pub fn is_disabled(&self, container_id: &str) -> bool {
        self.state.get(container_id).map(|r| !r.is_enabled).unwrap_or(false)
    }

    /// Set the detected format for a container
    pub fn set_format(&self, container_id: String, format: LogFormat) {
        // Upsert: If exists, update format. If new, insert with enabled=true
        self.state.entry(container_id).and_modify(|s| {
            // Only re-enable if the format has CHANGED.
            // If we previously disabled "Json" parsing because of errors, and detection
            // says "It's Json" again, we should stay disabled to avoid an infinite
            // Enable -> Fail -> Disable -> Detect -> Enable loop.
            if s.format != format {
                s.format = format;
                s.is_enabled = true; 
            }
        }).or_insert(ContainerState {
            format,
            is_enabled: true,
        });
    }

    /// Disable parsing for a container (fallback to plain text)
    pub fn disable_parsing(&self, container_id: &str) {
        if let Some(mut entry) = self.state.get_mut(container_id) {
            entry.is_enabled = false;
        }
    }

    /// Enable parsing for a container
    pub fn enable_parsing(&self, container_id: &str) {
        if let Some(mut entry) = self.state.get_mut(container_id) {
            entry.is_enabled = true;
        }
    }

    /// Remove a container from the cache
    pub fn remove(&self, container_id: &str) {
        self.state.remove(container_id);
    }

    pub fn clear(&self) {
        self.state.clear();
    }

    pub fn len(&self) -> usize {
        self.state.len()
    }

    pub fn is_empty(&self) -> bool {
        self.state.is_empty()
    }

    /// Get statistics about the cache
    /// This is now atomic-ish (iterating one map is safer than correlating two)
    pub fn stats(&self) -> CacheStats {
        let mut stats = CacheStats {
            total_containers: 0,
            enabled_containers: 0,
            disabled_containers: 0,
            json_containers: 0,
            logfmt_containers: 0,
            syslog_containers: 0,
            httplog_containers: 0,
            plain_containers: 0,
            unknown_containers: 0,
        };

        // Single pass iteration O(N)
        for entry in self.state.iter() {
            let state = entry.value();
            stats.total_containers += 1;
            
            if state.is_enabled {
                stats.enabled_containers += 1;
            } else {
                stats.disabled_containers += 1;
            }

            match state.format {
                LogFormat::Json => stats.json_containers += 1,
                LogFormat::Logfmt => stats.logfmt_containers += 1,
                LogFormat::Syslog => stats.syslog_containers += 1,
                LogFormat::HttpLog => stats.httplog_containers += 1,
                LogFormat::PlainText => stats.plain_containers += 1,
                LogFormat::Unknown => stats.unknown_containers += 1,
            }
        }

        stats
    }
}

impl Default for ParserCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Cache statistics
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    pub total_containers: usize,
    pub enabled_containers: usize,
    pub disabled_containers: usize,
    pub json_containers: usize,
    pub logfmt_containers: usize,
    pub syslog_containers: usize,
    pub httplog_containers: usize,
    pub plain_containers: usize,
    pub unknown_containers: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_lifecycle() {
        let cache = ParserCache::new();
        
        // 1. Set Format
        cache.set_format("c1".to_string(), LogFormat::Json);
        assert_eq!(cache.get_format("c1"), Some(LogFormat::Json));

        // 2. Disable
        cache.disable_parsing("c1");
        assert_eq!(cache.get_format("c1"), None, "Should return None when disabled");

        // 3. Re-enable
        cache.enable_parsing("c1");
        assert_eq!(cache.get_format("c1"), Some(LogFormat::Json));
    }

    #[test]
    fn test_cache_stats_accuracy() {
        let cache = ParserCache::new();
        
        cache.set_format("c1".to_string(), LogFormat::Json);
        cache.set_format("c2".to_string(), LogFormat::Logfmt);
        cache.set_format("c3".to_string(), LogFormat::Json);
        
        cache.disable_parsing("c2");

        let stats = cache.stats();
        
        assert_eq!(stats.total_containers, 3);
        assert_eq!(stats.enabled_containers, 2); // c1, c3
        assert_eq!(stats.disabled_containers, 1); // c2
        assert_eq!(stats.json_containers, 2);
        assert_eq!(stats.logfmt_containers, 1);
    }
    
    #[test]
    fn test_set_format_re_enables() {
        // If a container was disabled, detecting a new format should probably re-enable it
        let cache = ParserCache::new();
        cache.set_format("c1".to_string(), LogFormat::Json);
        cache.disable_parsing("c1");
        
        // New detection happens (e.g. logs changed format)
        cache.set_format("c1".to_string(), LogFormat::Logfmt);
        
        let stats = cache.stats();
        assert_eq!(stats.disabled_containers, 0);
        assert_eq!(stats.logfmt_containers, 1);
        assert_eq!(cache.get_format("c1"), Some(LogFormat::Logfmt));
    }

    #[test]
    fn test_set_format_no_reenable_same_format() {
        // PREVENT LOOP: If disabled, setting SAME format should NOT re-enable
        let cache = ParserCache::new();
        cache.set_format("c1".to_string(), LogFormat::Json);
        cache.disable_parsing("c1");
        
        // Detection runs again, finds JSON again
        cache.set_format("c1".to_string(), LogFormat::Json);
        
        let stats = cache.stats();
        // Should STILL be disabled
        assert_eq!(stats.disabled_containers, 1);
        assert_eq!(stats.enabled_containers, 0);
        // get_format returns None because disabled
        assert_eq!(cache.get_format("c1"), None);
    }
}
