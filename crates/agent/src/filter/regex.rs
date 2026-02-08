//! Regex — regex-based log filtering helpers.
//!
//! Provides pre-built filter patterns, multi-pattern composition,
//! and a convenience builder on top of [`FilterEngine`].

use crate::filter::engine::{FilterEngine, FilterMode, FilterError};

/// Well-known log-level patterns for quick filtering.
pub struct Patterns;

impl Patterns {
    /// Matches common error indicators (case-insensitive).
    pub const ERROR: &'static str = r"(?i)\b(error|err|fatal|critical|crit|panic(?:ked)?|exception)\b";

    /// Matches common warning indicators (case-insensitive).
    pub const WARN: &'static str = r"(?i)\b(warn|warning)\b";

    /// Matches error OR warning level lines.
    pub const ERROR_OR_WARN: &'static str =
        r"(?i)\b(error|err|fatal|critical|crit|panic|exception|warn|warning)\b";

    /// Matches HTTP 5xx status codes (server errors).
    pub const HTTP_5XX: &'static str = r#"\b5\d{2}\b"#;

    /// Matches HTTP 4xx status codes (client errors).
    pub const HTTP_4XX: &'static str = r#"\b4\d{2}\b"#;

    /// Matches common stack-trace indicators.
    pub const STACK_TRACE: &'static str =
        r"(?i)(^\s+at\s|^caused by:|^traceback|^goroutine\s|thread '.*' panicked)";

    /// Matches common health-check / liveness log lines.
    pub const HEALTHCHECK: &'static str =
        r"(?i)(healthcheck|health.check|/health|/ready|/live|/ping)";
}

/// Build a [`FilterEngine`] that matches **any** of the given patterns.
///
/// Internally the patterns are joined with `|` into a single regex alternation
/// so the engine evaluates one compiled matcher (fast).
///
/// ```rust,ignore
/// let filter = multi_pattern(&["error", "panic"], FilterMode::Include)?;
/// assert!(filter.should_include(b"PANIC: stack overflow"));
/// ```
pub fn multi_pattern(
    patterns: &[&str],
    mode: FilterMode,
) -> Result<FilterEngine, FilterError> {
    if patterns.is_empty() {
        return Err(FilterError::InvalidRegex("at least one pattern required".into()));
    }
    let combined = if patterns.len() == 1 {
        patterns[0].to_string()
    } else {
        patterns.iter().map(|p| format!("(?:{})", p)).collect::<Vec<_>>().join("|")
    };
    FilterEngine::new(&combined, false, mode)
}

/// Convenience: build an include-mode filter from a single pattern.
pub fn include(pattern: &str) -> Result<FilterEngine, FilterError> {
    FilterEngine::new(pattern, false, FilterMode::Include)
}

/// Convenience: build an exclude-mode filter from a single pattern.
pub fn exclude(pattern: &str) -> Result<FilterEngine, FilterError> {
    FilterEngine::new(pattern, false, FilterMode::Exclude)
}

/// Convenience: error-only filter (include lines matching error patterns).
pub fn errors_only() -> Result<FilterEngine, FilterError> {
    FilterEngine::new(Patterns::ERROR, false, FilterMode::Include)
}

/// Convenience: hide health-check noise.
pub fn hide_healthchecks() -> Result<FilterEngine, FilterError> {
    FilterEngine::new(Patterns::HEALTHCHECK, false, FilterMode::Exclude)
}

// ── Tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_pattern() {
        let f = errors_only().unwrap();
        assert!(f.should_include(b"2024-01-01 ERROR: connection refused"));
        assert!(f.should_include(b"FATAL: out of memory"));
        assert!(f.should_include(b"thread 'main' panicked at ..."));
        assert!(!f.should_include(b"INFO: request handled in 23ms"));
        assert!(!f.should_include(b"DEBUG: entering function"));
    }

    #[test]
    fn test_warn_pattern() {
        let f = FilterEngine::new(Patterns::WARN, false, FilterMode::Include).unwrap();
        assert!(f.should_include(b"WARN: high memory usage"));
        assert!(f.should_include(b"[WARNING] Disk space low"));
        assert!(!f.should_include(b"INFO: all good"));
    }

    #[test]
    fn test_hide_healthchecks() {
        let f = hide_healthchecks().unwrap();
        assert!(!f.should_include(b"GET /health 200 OK"));
        assert!(!f.should_include(b"healthcheck: ok"));
        assert!(f.should_include(b"Processing user request"));
        assert!(f.should_include(b"ERROR: database timeout"));
    }

    #[test]
    fn test_multi_pattern() {
        let f = multi_pattern(
            &["error", "timeout", "refused"],
            FilterMode::Include,
        ).unwrap();
        assert!(f.should_include(b"connection refused"));
        assert!(f.should_include(b"request timeout after 30s"));
        assert!(f.should_include(b"ERROR: crash"));
        assert!(!f.should_include(b"INFO: success"));
    }

    #[test]
    fn test_multi_pattern_empty_errors() {
        let result = multi_pattern(&[], FilterMode::Include);
        assert!(result.is_err());
    }

    #[test]
    fn test_http_5xx() {
        let f = FilterEngine::new(Patterns::HTTP_5XX, false, FilterMode::Include).unwrap();
        assert!(f.should_include(b"GET /api/users 500 Internal Server Error"));
        assert!(f.should_include(b"POST /api/orders 502 Bad Gateway"));
        assert!(!f.should_include(b"GET /api/users 200 OK"));
        assert!(!f.should_include(b"GET /api/users 404 Not Found"));
    }

    #[test]
    fn test_include_convenience() {
        let f = include("database").unwrap();
        assert!(f.should_include(b"connecting to database"));
        assert!(!f.should_include(b"processing request"));
    }

    #[test]
    fn test_exclude_convenience() {
        let f = exclude("debug").unwrap();
        assert!(!f.should_include(b"DEBUG: entering loop"));
        assert!(f.should_include(b"ERROR: crash"));
    }
}
