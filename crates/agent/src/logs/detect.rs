//! Log format detection: label override → cache → single-line heuristic.
//!
//! Runs once per container on the first log line, then the result is cached.

use crate::parser::{LogFormat, cache::ParserCache, metrics::ParsingMetrics};
use std::collections::HashMap;

/// Resolve the log format for a container.
///
/// Priority chain (highest → lowest):
/// 1. Explicit Docker label `docktail.log_format` (user intent, always wins)
/// 2. Parser cache (already detected for this container in a prior stream)
/// 3. Single-line heuristic via [`quick_detect_format`]
pub fn resolve_format(
    container_id: &str,
    labels: &HashMap<String, String>,
    parser_cache: &ParserCache,
    first_line: &[u8],
    metrics: &ParsingMetrics,
) -> LogFormat {
    // 1. Explicit label override: docktail.log_format=json|logfmt|plain
    if let Some(label_val) = labels.get("docktail.log_format") {
        let format = match label_val.to_lowercase().as_str() {
            "json" => LogFormat::Json,
            "logfmt" => LogFormat::Logfmt,
            "syslog" => LogFormat::Syslog,
            "plain" | "plaintext" | "plain_text" | "text" => LogFormat::PlainText,
            _ => LogFormat::PlainText, // Unknown label value → safe default
        };
        parser_cache.set_format(container_id.to_string(), format);
        metrics.record_detection(true);
        return format;
    }

    if let Some(cached) = parser_cache.get_format(container_id) {
        return cached;
    }

    // Single-line heuristic: fast byte-level check on first line
    let format = quick_detect_format(first_line);
    parser_cache.set_format(container_id.to_string(), format);
    metrics.record_detection(format != LogFormat::Unknown);
    format
}

/// Fast single-line format detection (no buffering, no allocation).
/// - First byte `{` + last byte `}` → JSON
/// - Contains multiple `key=value` pairs → Logfmt
/// - Everything else → PlainText (safe default)
///
/// This runs ONCE per container on the first log line.
pub fn quick_detect_format(line: &[u8]) -> LogFormat {
    if line.is_empty() {
        return LogFormat::PlainText;
    }

    // Trim both leading and trailing whitespace so that indented JSON
    // (e.g. after ANSI stripping leaves leading spaces) is still detected.
    let start = line.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(line.len());
    let end = line.iter().rposition(|b| !b.is_ascii_whitespace()).map(|p| p + 1).unwrap_or(0);
    let trimmed = if start < end { &line[start..end] } else { return LogFormat::PlainText; };

    // JSON: starts with '{', ends with '}'
    if trimmed.starts_with(b"{") && trimmed.ends_with(b"}") {
        return LogFormat::Json;
    }

    // Logfmt: contains multiple key=value pairs separated by spaces
    // e.g. "level=info msg=\"hello\" ts=2026-01-01"
    // Require the character before '=' to be alphanumeric or underscore
    // to avoid false positives on operators (>=, <=, ==, !=) and URLs (?a=1&b=2)
    if trimmed.windows(2).filter(|w| (w[0].is_ascii_alphanumeric() || w[0] == b'_') && w[1] == b'=').count() >= 2 {
        return LogFormat::Logfmt;
    }

    LogFormat::PlainText
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::cache::ParserCache;
    use crate::parser::metrics::ParsingMetrics;

    // ─────────────────────────────────────────────────────────
    // quick_detect_format: Edge Cases
    // ─────────────────────────────────────────────────────────

    #[test]
    fn detect_empty_input() {
        assert_eq!(quick_detect_format(b""), LogFormat::PlainText);
    }

    #[test]
    fn detect_whitespace_only() {
        assert_eq!(quick_detect_format(b"   "), LogFormat::PlainText);
        assert_eq!(quick_detect_format(b"\t\n"), LogFormat::PlainText);
    }

    #[test]
    fn detect_valid_json_object() {
        let line = br#"{"level":"error","msg":"connection refused","ts":"2026-01-01T00:00:00Z"}"#;
        assert_eq!(quick_detect_format(line), LogFormat::Json);
    }

    #[test]
    fn detect_json_with_trailing_newline() {
        let line = b"{\"level\":\"info\"}\n";
        assert_eq!(quick_detect_format(line), LogFormat::Json);
    }

    #[test]
    fn detect_json_with_trailing_crlf() {
        let line = b"{\"msg\":\"hello\"}\r\n";
        assert_eq!(quick_detect_format(line), LogFormat::Json);
    }

    #[test]
    fn detect_json_with_spaces() {
        let line = b"{\"msg\": \"hello\"}  ";
        assert_eq!(quick_detect_format(line), LogFormat::Json);
    }

    #[test]
    fn detect_not_json_only_opening_brace() {
        let line = b"{incomplete json";
        assert_eq!(quick_detect_format(line), LogFormat::PlainText);
    }

    #[test]
    fn detect_not_json_curly_in_message() {
        let line = b"Error: expected {token} but got null";
        assert_eq!(quick_detect_format(line), LogFormat::PlainText);
    }

    #[test]
    fn detect_logfmt_basic() {
        let line = b"level=info msg=\"server started\" port=8080";
        assert_eq!(quick_detect_format(line), LogFormat::Logfmt);
    }

    #[test]
    fn detect_logfmt_go_style() {
        let line = b"ts=2026-01-01T00:00:00Z caller=main.go:42 level=info msg=\"ready\"";
        assert_eq!(quick_detect_format(line), LogFormat::Logfmt);
    }

    #[test]
    fn detect_not_logfmt_single_equals() {
        let line = b"PATH=/usr/bin";
        assert_eq!(quick_detect_format(line), LogFormat::PlainText);
    }

    #[test]
    fn detect_not_logfmt_equals_in_url() {
        let line = b"GET /api?page=1 HTTP/1.1";
        assert_eq!(quick_detect_format(line), LogFormat::PlainText);
    }

    #[test]
    fn detect_plain_text_server_banner() {
        let line = b"Nginx v1.24.0 starting...";
        assert_eq!(quick_detect_format(line), LogFormat::PlainText);
    }

    #[test]
    fn detect_plain_text_stack_trace_line() {
        let line = b"    at com.example.App.main(App.java:42)";
        assert_eq!(quick_detect_format(line), LogFormat::PlainText);
    }

    #[test]
    fn detect_single_byte_brace() {
        assert_eq!(quick_detect_format(b"{"), LogFormat::PlainText);
        assert_eq!(quick_detect_format(b"}"), LogFormat::PlainText);
    }

    #[test]
    fn detect_empty_json_object() {
        assert_eq!(quick_detect_format(b"{}"), LogFormat::Json);
    }

    #[test]
    fn detect_binary_garbage() {
        let line: &[u8] = &[0xFF, 0xFE, 0x00, 0x01, 0x80, 0x90];
        assert_eq!(quick_detect_format(line), LogFormat::PlainText);
    }

    #[test]
    fn detect_very_long_plain_text() {
        let line = vec![b'A'; 10_000];
        assert_eq!(quick_detect_format(&line), LogFormat::PlainText);
    }

    #[test]
    fn detect_logfmt_with_equals_at_start() {
        let line = b"=broken key=value another=pair";
        assert_eq!(quick_detect_format(line), LogFormat::Logfmt);
    }

    // ─────────────────────────────────────────────────────────
    // resolve_format: Priority & Caching Edge Cases
    // ─────────────────────────────────────────────────────────

    #[test]
    fn resolve_label_overrides_heuristic() {
        let cache = ParserCache::new();
        let metrics = ParsingMetrics::new();
        let mut labels = HashMap::new();
        labels.insert("docktail.log_format".to_string(), "json".to_string());

        let format = resolve_format(
            "container-1", &labels, &cache, b"Server started!", &metrics,
        );

        assert_eq!(format, LogFormat::Json, "Label should override heuristic");
        assert_eq!(cache.get_format("container-1"), Some(LogFormat::Json), "Should be cached");
    }

    #[test]
    fn resolve_label_case_insensitive() {
        let cache = ParserCache::new();
        let metrics = ParsingMetrics::new();
        let mut labels = HashMap::new();
        labels.insert("docktail.log_format".to_string(), "JSON".to_string());

        let format = resolve_format(
            "c1", &labels, &cache, b"anything", &metrics,
        );
        assert_eq!(format, LogFormat::Json);
    }

    #[test]
    fn resolve_label_unknown_value_defaults_plain() {
        let cache = ParserCache::new();
        let metrics = ParsingMetrics::new();
        let mut labels = HashMap::new();
        labels.insert("docktail.log_format".to_string(), "xml".to_string());

        let format = resolve_format(
            "c1", &labels, &cache, b"anything", &metrics,
        );
        assert_eq!(format, LogFormat::PlainText, "Unknown label value → PlainText");
    }

    #[test]
    fn resolve_label_plaintext_variants() {
        for variant in &["plain", "plaintext", "plain_text", "text"] {
            let cache = ParserCache::new();
            let metrics = ParsingMetrics::new();
            let mut labels = HashMap::new();
            labels.insert("docktail.log_format".to_string(), variant.to_string());

            let format = resolve_format(
                "c1", &labels, &cache, b"{\"json\":true}", &metrics,
            );
            assert_eq!(format, LogFormat::PlainText, "Variant '{}' should → PlainText", variant);
        }
    }

    #[test]
    fn resolve_cache_hit_skips_heuristic() {
        let cache = ParserCache::new();
        let metrics = ParsingMetrics::new();
        cache.set_format("c1".to_string(), LogFormat::Json);

        let format = resolve_format(
            "c1", &HashMap::new(), &cache, b"plain text line", &metrics,
        );
        assert_eq!(format, LogFormat::Json, "Cache hit should override heuristic");
    }

    #[test]
    fn resolve_heuristic_json_first_line() {
        let cache = ParserCache::new();
        let metrics = ParsingMetrics::new();

        let format = resolve_format(
            "c1", &HashMap::new(), &cache,
            br#"{"level":"info","msg":"started"}"#, &metrics,
        );
        assert_eq!(format, LogFormat::Json);
        assert_eq!(cache.get_format("c1"), Some(LogFormat::Json));
    }

    #[test]
    fn resolve_heuristic_logfmt_first_line() {
        let cache = ParserCache::new();
        let metrics = ParsingMetrics::new();

        let format = resolve_format(
            "c1", &HashMap::new(), &cache,
            b"level=info msg=\"ready\" port=3000", &metrics,
        );
        assert_eq!(format, LogFormat::Logfmt);
        assert_eq!(cache.get_format("c1"), Some(LogFormat::Logfmt));
    }

    #[test]
    fn resolve_heuristic_plain_text_first_line() {
        let cache = ParserCache::new();
        let metrics = ParsingMetrics::new();

        let format = resolve_format(
            "c1", &HashMap::new(), &cache,
            b"2026-01-01 INFO  Application started", &metrics,
        );
        assert_eq!(format, LogFormat::PlainText);
        assert_eq!(cache.get_format("c1"), Some(LogFormat::PlainText));
    }

    #[test]
    fn resolve_disabled_container_returns_no_cache() {
        let cache = ParserCache::new();
        let metrics = ParsingMetrics::new();
        cache.set_format("c1".to_string(), LogFormat::Json);
        cache.disable_parsing("c1");

        let format = resolve_format(
            "c1", &HashMap::new(), &cache,
            br#"{"level":"error"}"#, &metrics,
        );
        assert_eq!(format, LogFormat::Json, "Heuristic should detect JSON");
    }

    #[test]
    fn resolve_label_wins_over_cache() {
        let cache = ParserCache::new();
        let metrics = ParsingMetrics::new();
        cache.set_format("c1".to_string(), LogFormat::Logfmt);

        let mut labels = HashMap::new();
        labels.insert("docktail.log_format".to_string(), "json".to_string());

        let format = resolve_format(
            "c1", &labels, &cache,
            b"plain text line", &metrics,
        );
        assert_eq!(format, LogFormat::Json, "Label always wins");
        assert_eq!(cache.get_format("c1"), Some(LogFormat::Json));
    }

    #[test]
    fn resolve_empty_first_line() {
        let cache = ParserCache::new();
        let metrics = ParsingMetrics::new();

        let format = resolve_format(
            "c1", &HashMap::new(), &cache, b"", &metrics,
        );
        assert_eq!(format, LogFormat::PlainText);
    }

    #[test]
    fn resolve_multiple_containers_independent() {
        let cache = ParserCache::new();
        let metrics = ParsingMetrics::new();

        resolve_format(
            "json-app", &HashMap::new(), &cache,
            br#"{"msg":"hello"}"#, &metrics,
        );
        resolve_format(
            "logfmt-app", &HashMap::new(), &cache,
            b"level=info msg=hello", &metrics,
        );
        resolve_format(
            "plain-app", &HashMap::new(), &cache,
            b"Server started", &metrics,
        );

        assert_eq!(cache.get_format("json-app"), Some(LogFormat::Json));
        assert_eq!(cache.get_format("logfmt-app"), Some(LogFormat::Logfmt));
        assert_eq!(cache.get_format("plain-app"), Some(LogFormat::PlainText));
    }

    #[test]
    fn resolve_second_call_uses_cache() {
        let cache = ParserCache::new();
        let metrics = ParsingMetrics::new();

        resolve_format(
            "c1", &HashMap::new(), &cache,
            br#"{"level":"info"}"#, &metrics,
        );

        let format = resolve_format(
            "c1", &HashMap::new(), &cache,
            b"this is plain text", &metrics,
        );
        assert_eq!(format, LogFormat::Json, "Second call should use cache, not re-detect");
    }

    // ─────────────────────────────────────────────────────────
    // Adversarial / Tricky Edge Cases
    // ─────────────────────────────────────────────────────────

    #[test]
    fn detect_json_array_not_object() {
        let line = b"[1, 2, 3]";
        assert_eq!(quick_detect_format(line), LogFormat::PlainText);
    }

    #[test]
    fn detect_nested_braces_in_text() {
        let line = b"Rendering template {{user.name}} failed";
        assert_eq!(quick_detect_format(line), LogFormat::PlainText);
    }

    #[test]
    fn detect_logfmt_with_spaces_around_equals() {
        let line = b"key = value another = pair";
        assert_eq!(quick_detect_format(line), LogFormat::PlainText);
    }

    #[test]
    fn detect_syslog_like_line() {
        let line = b"<134>Jan  1 00:00:00 myhost myapp[1234]: connection established";
        assert_eq!(quick_detect_format(line), LogFormat::PlainText);
    }

    #[test]
    fn detect_docker_compose_prefix() {
        let line = b"web-1  | {\"level\":\"info\"}";
        assert_eq!(quick_detect_format(line), LogFormat::PlainText);
    }

    #[test]
    fn detect_json_with_ansi_prefix() {
        let line = b"\x1b[32m{\"level\":\"info\"}\x1b[0m";
        assert_eq!(quick_detect_format(line), LogFormat::PlainText);
    }

    #[test]
    fn detect_very_large_json() {
        let mut line = Vec::with_capacity(100_000);
        line.push(b'{');
        line.extend_from_slice(b"\"key\":\"");
        line.extend(vec![b'x'; 99_980]);
        line.extend_from_slice(b"\"}");
        assert_eq!(quick_detect_format(&line), LogFormat::Json);
    }

    // ─────────────────────────────────────────────────────────
    // Metrics Tracking Verification
    // ─────────────────────────────────────────────────────────

    #[test]
    fn resolve_metrics_recorded_on_label() {
        let cache = ParserCache::new();
        let metrics = ParsingMetrics::new();
        let mut labels = HashMap::new();
        labels.insert("docktail.log_format".to_string(), "json".to_string());

        resolve_format("c1", &labels, &cache, b"", &metrics);

        let snap = metrics.snapshot();
        assert_eq!(snap.detection_attempts, 1);
        assert_eq!(snap.detection_success, 1);
    }

    #[test]
    fn resolve_metrics_recorded_on_heuristic() {
        let cache = ParserCache::new();
        let metrics = ParsingMetrics::new();

        resolve_format(
            "c1", &HashMap::new(), &cache,
            br#"{"level":"info"}"#, &metrics,
        );

        let snap = metrics.snapshot();
        assert_eq!(snap.detection_attempts, 1);
        assert_eq!(snap.detection_success, 1);
    }

    #[test]
    fn resolve_no_metrics_on_cache_hit() {
        let cache = ParserCache::new();
        let metrics = ParsingMetrics::new();
        cache.set_format("c1".to_string(), LogFormat::Json);

        resolve_format(
            "c1", &HashMap::new(), &cache, b"anything", &metrics,
        );

        let snap = metrics.snapshot();
        assert_eq!(snap.detection_attempts, 0, "Cache hit should not record detection");
    }
}
