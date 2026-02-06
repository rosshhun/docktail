use std::sync::atomic::{AtomicU64, Ordering};
use grep_matcher::Matcher;
use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum FilterError {
    #[error("Invalid regex pattern: {0}")]
    InvalidRegex(String),
}

#[derive(Debug, Clone)]
pub enum FilterMode {
    Include, 
    Exclude,
}

#[derive(Debug, Default)]
pub struct FilterStats {
    pub lines_scanned: AtomicU64,
    pub lines_matched: AtomicU64,
    pub bytes_processed: AtomicU64,
}

pub struct FilterEngine {
    matcher: RegexMatcher,
    mode: FilterMode,
    stats: FilterStats,
}

impl FilterEngine {
    pub fn new(pattern: &str, case_sensitive: bool, mode: FilterMode) -> Result<Self, FilterError> {
        let matcher = RegexMatcherBuilder::new()
            .case_insensitive(!case_sensitive)
            .multi_line(false)
            .build(pattern)
            .map_err(|e| FilterError::InvalidRegex(e.to_string()))?;

        Ok(Self {
            matcher,
            mode,
            stats: FilterStats::default(),
        })
    }

    #[inline]
    pub fn should_include(&self, line: &[u8]) -> bool {
        self.stats.lines_scanned.fetch_add(1, Ordering::Relaxed);
        self.stats.bytes_processed.fetch_add(line.len() as u64, Ordering::Relaxed);

        let matches = self.matcher.is_match(line).unwrap_or(false);

        let include = match self.mode {
            FilterMode::Include => matches,
            FilterMode::Exclude => !matches,
        };

        if include {
            self.stats.lines_matched.fetch_add(1, Ordering::Relaxed);
        }

        include
    }

    pub fn stats(&self) -> (u64, u64, u64) {
        (
            self.stats.lines_scanned.load(Ordering::Relaxed),
            self.stats.lines_matched.load(Ordering::Relaxed),
            self.stats.bytes_processed.load(Ordering::Relaxed),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_include_mode() {
        let filter = FilterEngine::new("error", false, FilterMode::Include)
            .expect("Failed to create filter");

        assert!(filter.should_include(b"This is an error message"));
        assert!(!filter.should_include(b"This is a debug message"));
        assert!(filter.should_include(b"ERROR: Critical failure"));
    }

    #[test]
    fn test_filter_exclude_mode() {
        let filter = FilterEngine::new("healthcheck", true, FilterMode::Exclude)
            .expect("Failed to create filter");

        assert!(!filter.should_include(b"healthcheck: ok"));
        assert!(filter.should_include(b"Processing request"));
        assert!(!filter.should_include(b"Running healthcheck now"));
    }

    #[test]
    fn test_case_sensitive() {
        let filter = FilterEngine::new("Error", true, FilterMode::Include)
            .expect("Failed to create filter");

        assert!(filter.should_include(b"Error: something"));
        assert!(!filter.should_include(b"error: something"));
        assert!(!filter.should_include(b"ERROR: something"));
    }

    #[test]
    fn test_case_insensitive() {
        let filter = FilterEngine::new("error", false, FilterMode::Include)
            .expect("Failed to create filter");

        assert!(filter.should_include(b"Error: something"));
        assert!(filter.should_include(b"error: something"));
        assert!(filter.should_include(b"ERROR: something"));
    }

    #[test]
    fn test_case_insensitive_builder() {
        let filter = FilterEngine::new("error", false, FilterMode::Include)
            .expect("Failed to create filter");

        assert!(filter.should_include(b"ERROR: Critical failure"));
        assert!(filter.should_include(b"error: minor issue"));
    }

    #[test]
    fn test_invalid_regex() {
        let result = FilterEngine::new("[invalid", true, FilterMode::Include);
        assert!(result.is_err());
    }

    #[test]
    fn test_stats_tracking() {
        let filter = FilterEngine::new("test", false, FilterMode::Include)
            .expect("Failed to create filter");

        filter.should_include(b"test message");
        filter.should_include(b"another message");
        filter.should_include(b"test again");

        let (scanned, matched, bytes) = filter.stats();
        assert_eq!(scanned, 3);
        assert_eq!(matched, 2);
        assert!(bytes > 0);
    }
}
