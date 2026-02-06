use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;
use tokio_stream::{Stream, StreamExt};
use tonic::{Request, Response, Status};
use prost_types::Timestamp as ProtoTimestamp;

use crate::docker::client::DockerError;
use crate::docker::stream::{LogStreamRequest as InternalLogStreamRequest, LogLevel, LogLine};
use crate::filter::engine::{FilterEngine, FilterMode};
use crate::state::SharedState;
use crate::parser::{LogFormat, LogParser, strip_ansi_codes};
use crate::parser::traits::ParsedLog;
use crate::parser::formats::{JsonParser, LogfmtParser, PlainTextParser};
use super::multiline::MultilineGrouper;

use super::proto::{
    log_service_server::LogService,
    LogStreamRequest, NormalizedLogEntry,
    FilterMode as ProtoFilterMode,
    ParsedLog as ProtoParsedLog, ParseMetadata as ProtoParseMetadata,
    RequestContext as ProtoRequestContext, ErrorContext as ProtoErrorContext,
    KeyValuePair, LogFormat as ProtoLogFormat,
};

/// Implementation of the LogService gRPC service
/// Handles log streaming with filtering, parsing, and time-travel support
pub struct LogServiceImpl {
    state: SharedState,
}

impl LogServiceImpl {
    pub fn new(state: SharedState) -> Self {
        Self { state }
    }

    /// 1. Explicit label override  (user intent, always wins)
    /// 2. Parser cache              (already detected for this container)
    /// 3. Single-line heuristic     (fast, first-line only, no buffering)
    ///
    /// This replaces multi-sample buffered detection.
    /// make a decision on the first line, cache it, and if parsing fails later just yield raw.
    fn resolve_format(
        container_id: &str,
        labels: &std::collections::HashMap<String, String>,
        parser_cache: &crate::parser::cache::ParserCache,
        first_line: &[u8],
        metrics: &crate::parser::metrics::ParsingMetrics,
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

        // 2. Cache hit: format was already resolved in a previous stream
        if let Some(cached) = parser_cache.get_format(container_id) {
            return cached;
        }

        // 3. Single-line heuristic: fast byte-level check on first line
        let format = Self::quick_detect_format(first_line);
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
    fn quick_detect_format(line: &[u8]) -> LogFormat {
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

    /// Convert protobuf FilterMode to internal FilterMode
    fn convert_filter_mode(proto_mode: i32) -> FilterMode {
        match ProtoFilterMode::try_from(proto_mode) {
            Ok(ProtoFilterMode::Include) => FilterMode::Include,
            Ok(ProtoFilterMode::Exclude) => FilterMode::Exclude,
            _ => FilterMode::Include, // Default to Include for safety
        }
    }

    /// Convert protobuf LogStreamRequest to internal request
    fn convert_request(req: LogStreamRequest) -> Result<InternalLogStreamRequest, Status> {
        // Validate that since <= until when both are provided
        if let (Some(since), Some(until)) = (req.since, req.until) {
            if since > until {
                return Err(Status::invalid_argument(
                    format!("'since' ({}) must not be after 'until' ({})", since, until)
                ));
            }
        }

        let filter_mode = Self::convert_filter_mode(req.filter_mode);

        Ok(InternalLogStreamRequest {
            container_id: req.container_id,
            since: req.since,
            until: req.until,
            follow: req.follow,
            filter_pattern: req.filter_pattern,
            filter_mode,
            tail_lines: req.tail_lines,
        })
    }

    /// Convert internal LogLevel to protobuf enum value
    fn convert_log_level(level: LogLevel) -> i32 {
        match level {
            LogLevel::Stdout => 1, // LOG_LEVEL_STDOUT
            LogLevel::Stderr => 2, // LOG_LEVEL_STDERR
        }
    }

    /// Get parser for a specific format
    fn get_parser(format: LogFormat) -> Box<dyn LogParser> {
        match format {
            LogFormat::Json => Box::new(JsonParser::new()),
            LogFormat::Logfmt => Box::new(LogfmtParser),
            _ => Box::new(PlainTextParser),
        }
    }

    /// Convert internal ParsedLog to protobuf
    fn convert_parsed_log(parsed: ParsedLog) -> ProtoParsedLog {
        ProtoParsedLog {
            level: parsed.level,
            message: parsed.message,
            logger: parsed.logger,
            // Convert DateTime<Utc> to protobuf Timestamp
            timestamp: parsed.timestamp.map(|dt| ProtoTimestamp {
                seconds: dt.timestamp(),
                nanos: dt.timestamp_subsec_nanos() as i32,
            }),
            request: parsed.request.map(|r| ProtoRequestContext {
                method: r.method,
                path: r.path,
                remote_addr: r.remote_addr,
                status_code: r.status_code,
                duration_ms: r.duration_ms,
                request_id: r.request_id,
            }),
            error: parsed.error.map(|e| ProtoErrorContext {
                error_type: e.error_type,
                error_message: e.error_message,
                stack_trace: e.stack_trace,
                file: e.file,
                line: e.line,
            }),
            fields: parsed.fields.into_iter()
                .map(|(k, v)| KeyValuePair { key: k, value: v })
                .collect(),
        }
    }

    /// Convert LogFormat to protobuf enum
    fn convert_log_format(format: LogFormat) -> i32 {
        match format {
            LogFormat::Json => ProtoLogFormat::Json as i32,
            LogFormat::Logfmt => ProtoLogFormat::Logfmt as i32,
            LogFormat::PlainText => ProtoLogFormat::PlainText as i32,
            LogFormat::Syslog => ProtoLogFormat::Syslog as i32,
            LogFormat::HttpLog => ProtoLogFormat::HttpLog as i32,
            LogFormat::Unknown => ProtoLogFormat::Unknown as i32,
        }
    }
}

#[tonic::async_trait]
impl LogService for LogServiceImpl {
    type StreamLogsStream = Pin<Box<dyn Stream<Item = Result<NormalizedLogEntry, Status>> + Send>>;

    async fn stream_logs(
        &self,
        request: Request<LogStreamRequest>,
    ) -> Result<Response<Self::StreamLogsStream>, Status> {
        let req = request.into_inner();
        let container_id = req.container_id.trim().to_string();
        let disable_parsing = req.disable_parsing;

        if container_id.is_empty() {
            return Err(Status::invalid_argument("container_id must not be empty"));
        }

        // Convert protobuf request to internal request
        let mut req_with_trimmed_id = req.clone();
        req_with_trimmed_id.container_id = container_id.clone();
        let internal_req = Self::convert_request(req_with_trimmed_id)
            .map_err(|e| Status::invalid_argument(format!("Invalid request: {}", e)))?;

        // Create filter if pattern is provided
        let filter = if let Some(pattern) = &req.filter_pattern {
            let filter_mode = Self::convert_filter_mode(req.filter_mode);
            
            match FilterEngine::new(pattern, false, filter_mode) {
                Ok(engine) => Some(Arc::new(engine)),
                Err(e) => {
                    return Err(Status::invalid_argument(format!("Invalid regex pattern: {}", e)));
                }
            }
        } else {
            None
        };

        // Get container labels for per-container multiline configuration
        let container_info = self.state.docker
            .inspect_container(&container_id)
            .await
            .map_err(|e| Status::internal(format!("Failed to inspect container: {}", e)))?;

        // Get log stream from Docker client with filter
        let mut log_stream = self.state.docker
            .stream_logs(internal_req, filter.clone())
            .await
            .map_err(|e| match e {
                DockerError::ContainerNotFound(msg) => Status::not_found(msg),
                DockerError::PermissionDenied => Status::permission_denied("Permission denied"),
                DockerError::UnsupportedLogDriver(msg) => Status::failed_precondition(msg),
                _ => Status::internal(format!("Docker error: {}", e)),
            })?;

        // Clone parser_cache and metrics for use in stream
        let parser_cache = Arc::clone(&self.state.parser_cache);
        let metrics = Arc::clone(&self.state.metrics);
        let container_labels = container_info.labels.clone();
        
        // Create multiline grouper with config from state, applying container overrides
        let container_config = self.state.config.multiline.for_container(
            &container_info.name,
            &container_info.labels
        );
        let mut grouper = if container_config.enabled {
            Some(MultilineGrouper::new(&container_config))
        } else {
            None
        };

        // Create the response stream
        // No buffering. Resolve format on first line, then
        // process every subsequent line immediately. Parse failures yield raw content.
        let response_stream = async_stream::stream! {
            // Parser state: resolved lazily on first line, then reused
            let mut format_resolved = false;
            let mut current_format = LogFormat::PlainText;
            let mut current_parser: Option<Box<dyn LogParser>> = None;

            let mut timeout_interval = tokio::time::interval(tokio::time::Duration::from_millis(150));
            timeout_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                let result = tokio::select! {
                    item = log_stream.next() => {
                        match item {
                            Some(r) => r,
                            None => break, // Stream ended
                        }
                    }
                    _ = timeout_interval.tick() => {
                        // Periodic timeout check for pending multiline groups
                        if let Some(ref mut g) = grouper {
                            while let Some(pending) = g.check_timeout() {
                                yield Ok(pending);
                            }
                        }
                        continue;
                    }
                };

                match result {
                    Ok(log_response) => {
                        let log_line = LogLine {
                            timestamp: log_response.timestamp,
                            stream_type: log_response.log_level,
                            content: log_response.content,
                        };
                        let sequence = log_response.sequence;

                        // Docker timestamp is already stripped by convert_bollard_log in client.rs.
                        // Do NOT call strip_docker_timestamp again here — it would eat
                        // the application's own timestamp prefix (e.g. tracing, log4j).
                        // Step 1: Strip ANSI escape codes
                        let cleaned = strip_ansi_codes(&log_line.content);
                        let cleaned_bytes = cleaned.as_ref();

                        // Resolve format on first line (one-time cost)
                        // label → cache → heuristic
                        if !format_resolved && !disable_parsing && !parser_cache.is_disabled(&container_id) {
                            current_format = Self::resolve_format(
                                &container_id,
                                &container_labels,
                                &parser_cache,
                                cleaned_bytes,
                                &metrics,
                            );
                            current_parser = Some(Self::get_parser(current_format));
                            format_resolved = true;

                            // Structured formats are self-contained per line — skip multiline grouping
                            if matches!(current_format, LogFormat::Json | LogFormat::Logfmt) {
                                if let Some(ref mut g) = grouper {
                                    g.set_passthrough(true);
                                }
                            }
                        }

                        // Parse the log line
                        let (parsed, metadata) = if disable_parsing {
                            (None, ProtoParseMetadata {
                                detected_format: ProtoLogFormat::Unknown as i32,
                                parse_success: false,
                                parse_error: Some("Parsing disabled".to_string()),
                                parse_time_nanos: 0,
                            })
                        } else if parser_cache.is_disabled(&container_id) {
                            (None, ProtoParseMetadata {
                                detected_format: ProtoLogFormat::PlainText as i32,
                                parse_success: false,
                                parse_error: Some("Parsing disabled for container".to_string()),
                                parse_time_nanos: 0,
                            })
                        } else if let Some(parser) = &current_parser {
                            let parse_start = Instant::now();
                            match parser.parse(cleaned_bytes) {
                                Ok(parsed_log) => {
                                    let parse_time = parse_start.elapsed().as_nanos() as u64;
                                    metrics.record_parse(current_format, parse_time);
                                    (
                                        Some(Self::convert_parsed_log(parsed_log)),
                                        ProtoParseMetadata {
                                            detected_format: Self::convert_log_format(current_format),
                                            parse_success: true,
                                            parse_error: None,
                                            parse_time_nanos: i64::try_from(parse_time).unwrap_or(i64::MAX),
                                        }
                                    )
                                }
                                Err(e) => {
                                    // parse failure → yield raw, don't crash.
                                    // Metrics track error rate; operators can investigate.
                                    metrics.record_error(crate::parser::metrics::MetricErrorType::Other);
                                    let elapsed_nanos = parse_start.elapsed().as_nanos();
                                    (None, ProtoParseMetadata {
                                        detected_format: Self::convert_log_format(current_format),
                                        parse_success: false,
                                        parse_error: Some(e.to_string()),
                                        parse_time_nanos: i64::try_from(elapsed_nanos).unwrap_or(i64::MAX),
                                    })
                                }
                            }
                        } else {
                            (None, ProtoParseMetadata {
                                detected_format: ProtoLogFormat::PlainText as i32,
                                parse_success: false,
                                parse_error: None,
                                parse_time_nanos: 0,
                            })
                        };

                        let entry = NormalizedLogEntry {
                            container_id: container_id.clone(),
                            timestamp_nanos: log_line.timestamp,
                            log_level: Self::convert_log_level(log_line.stream_type),
                            sequence,
                            raw_content: cleaned_bytes.to_vec(),
                            parsed,
                            metadata: Some(metadata),
                            grouped_lines: Vec::new(),
                            line_count: 1,
                            is_grouped: false,
                        };

                        // Multiline grouping
                        if let Some(ref mut g) = grouper {
                            for grouped in g.process(entry) {
                                yield Ok(grouped);
                            }
                        } else {
                            yield Ok(entry);
                        }
                    }
                    Err(e) => {
                        // Flush pending multiline group on error
                        if let Some(ref mut g) = grouper {
                            while let Some(pending) = g.flush() {
                                yield Ok(pending);
                            }
                        }
                        yield Err(Status::internal(format!("Stream error: {}", e)));
                        break;
                    }
                }
            }

            // Flush any pending multiline group at end of stream (loop broke)
            // Use while-let to drain both deferred entries and pending groups
            if let Some(ref mut g) = grouper {
                while let Some(pending) = g.flush() {
                    yield Ok(pending);
                }
            }
        };

        Ok(Response::new(Box::pin(response_stream)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::cache::ParserCache;
    use crate::parser::metrics::ParsingMetrics;
    use std::collections::HashMap;

    // ─────────────────────────────────────────────────────────
    // quick_detect_format: Edge Cases
    // ─────────────────────────────────────────────────────────

    #[test]
    fn detect_empty_input() {
        // An empty log line (e.g., container printed blank line)
        assert_eq!(LogServiceImpl::quick_detect_format(b""), LogFormat::PlainText);
    }

    #[test]
    fn detect_whitespace_only() {
        // Container outputs whitespace (e.g., padding lines)
        assert_eq!(LogServiceImpl::quick_detect_format(b"   "), LogFormat::PlainText);
        assert_eq!(LogServiceImpl::quick_detect_format(b"\t\n"), LogFormat::PlainText);
    }

    #[test]
    fn detect_valid_json_object() {
        let line = br#"{"level":"error","msg":"connection refused","ts":"2026-01-01T00:00:00Z"}"#;
        assert_eq!(LogServiceImpl::quick_detect_format(line), LogFormat::Json);
    }

    #[test]
    fn detect_json_with_trailing_newline() {
        // Docker often appends \n to log lines
        let line = b"{\"level\":\"info\"}\n";
        assert_eq!(LogServiceImpl::quick_detect_format(line), LogFormat::Json);
    }

    #[test]
    fn detect_json_with_trailing_crlf() {
        // Windows containers may use \r\n
        let line = b"{\"msg\":\"hello\"}\r\n";
        assert_eq!(LogServiceImpl::quick_detect_format(line), LogFormat::Json);
    }

    #[test]
    fn detect_json_with_spaces() {
        // Pretty-printed JSON with leading/trailing spaces shouldn't happen
        // but `{...}` check should still work since we trim trailing whitespace
        let line = b"{\"msg\": \"hello\"}  ";
        assert_eq!(LogServiceImpl::quick_detect_format(line), LogFormat::Json);
    }

    #[test]
    fn detect_not_json_only_opening_brace() {
        // A log that starts with { but doesn't end with } is NOT JSON
        let line = b"{incomplete json";
        assert_eq!(LogServiceImpl::quick_detect_format(line), LogFormat::PlainText);
    }

    #[test]
    fn detect_not_json_curly_in_message() {
        // Plain text that happens to contain { } but not at start/end
        let line = b"Error: expected {token} but got null";
        assert_eq!(LogServiceImpl::quick_detect_format(line), LogFormat::PlainText);
    }

    #[test]
    fn detect_logfmt_basic() {
        let line = b"level=info msg=\"server started\" port=8080";
        assert_eq!(LogServiceImpl::quick_detect_format(line), LogFormat::Logfmt);
    }

    #[test]
    fn detect_logfmt_go_style() {
        // Common Go app output (zerolog, logrus)
        let line = b"ts=2026-01-01T00:00:00Z caller=main.go:42 level=info msg=\"ready\"";
        assert_eq!(LogServiceImpl::quick_detect_format(line), LogFormat::Logfmt);
    }

    #[test]
    fn detect_not_logfmt_single_equals() {
        // A single key=value is NOT enough to be logfmt (could be a shell assignment)
        let line = b"PATH=/usr/bin";
        assert_eq!(LogServiceImpl::quick_detect_format(line), LogFormat::PlainText);
    }

    #[test]
    fn detect_not_logfmt_equals_in_url() {
        // URLs contain = in query strings, shouldn't trigger logfmt
        let line = b"GET /api?page=1 HTTP/1.1";
        assert_eq!(LogServiceImpl::quick_detect_format(line), LogFormat::PlainText);
    }

    #[test]
    fn detect_plain_text_server_banner() {
        let line = b"Nginx v1.24.0 starting...";
        assert_eq!(LogServiceImpl::quick_detect_format(line), LogFormat::PlainText);
    }

    #[test]
    fn detect_plain_text_stack_trace_line() {
        let line = b"    at com.example.App.main(App.java:42)";
        assert_eq!(LogServiceImpl::quick_detect_format(line), LogFormat::PlainText);
    }

    #[test]
    fn detect_single_byte_brace() {
        // Just "{" alone: after trimming, starts with { but doesn't end with }
        // so it's NOT JSON — it's a lone brace (e.g., from a code snippet log)
        assert_eq!(LogServiceImpl::quick_detect_format(b"{"), LogFormat::PlainText);
        assert_eq!(LogServiceImpl::quick_detect_format(b"}"), LogFormat::PlainText);
    }

    #[test]
    fn detect_empty_json_object() {
        // {} is valid JSON but not useful as a log line
        // Our heuristic will still classify it as JSON — this is correct because
        // the parser will handle it gracefully, and it IS valid JSON
        assert_eq!(LogServiceImpl::quick_detect_format(b"{}"), LogFormat::Json);
    }

    #[test]
    fn detect_binary_garbage() {
        // Non-UTF8 binary data from a container (e.g., compiled output)
        let line: &[u8] = &[0xFF, 0xFE, 0x00, 0x01, 0x80, 0x90];
        assert_eq!(LogServiceImpl::quick_detect_format(line), LogFormat::PlainText);
    }

    #[test]
    fn detect_very_long_plain_text() {
        // 10KB plain text line — should NOT cause performance issues
        let line = vec![b'A'; 10_000];
        assert_eq!(LogServiceImpl::quick_detect_format(&line), LogFormat::PlainText);
    }

    #[test]
    fn detect_logfmt_with_equals_at_start() {
        // Edge: "=value key=value" — first char is = (space before =)
        // The window check: w[0] != b' ' && w[1] == b'=', so " =" is filtered out
        let line = b"=broken key=value another=pair";
        // "=broken" has w[0] as start-of-line then '=' — depends on preceding char
        // At position 0, there is no window starting before '='. 
        // windows(2) gives: ['=','b'], ['b','r'], ... ['k','e'], ['e','y'], ['y','='], ...
        // 'y'!=' ' && '='=='=' → match. 'r'!=' ' && '='=='=' → match for "another="
        // So we get >= 2 matches → Logfmt
        assert_eq!(LogServiceImpl::quick_detect_format(line), LogFormat::Logfmt);
    }

    // ─────────────────────────────────────────────────────────
    // resolve_format: Priority & Caching Edge Cases
    // ─────────────────────────────────────────────────────────

    #[test]
    fn resolve_label_overrides_heuristic() {
        // User says "json" via label, even though first line looks like plain text
        let cache = ParserCache::new();
        let metrics = ParsingMetrics::new();
        let mut labels = HashMap::new();
        labels.insert("docktail.log_format".to_string(), "json".to_string());

        let format = LogServiceImpl::resolve_format(
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

        let format = LogServiceImpl::resolve_format(
            "c1", &labels, &cache, b"anything", &metrics,
        );
        assert_eq!(format, LogFormat::Json);
    }

    #[test]
    fn resolve_label_unknown_value_defaults_plain() {
        let cache = ParserCache::new();
        let metrics = ParsingMetrics::new();
        let mut labels = HashMap::new();
        labels.insert("docktail.log_format".to_string(), "xml".to_string()); // unsupported

        let format = LogServiceImpl::resolve_format(
            "c1", &labels, &cache, b"anything", &metrics,
        );
        assert_eq!(format, LogFormat::PlainText, "Unknown label value → PlainText");
    }

    #[test]
    fn resolve_label_plaintext_variants() {
        // All text/plain variants should work
        for variant in &["plain", "plaintext", "plain_text", "text"] {
            let cache = ParserCache::new();
            let metrics = ParsingMetrics::new();
            let mut labels = HashMap::new();
            labels.insert("docktail.log_format".to_string(), variant.to_string());

            let format = LogServiceImpl::resolve_format(
                "c1", &labels, &cache, b"{\"json\":true}", &metrics,
            );
            assert_eq!(format, LogFormat::PlainText, "Variant '{}' should → PlainText", variant);
        }
    }

    #[test]
    fn resolve_cache_hit_skips_heuristic() {
        // Pre-populate cache as if a previous stream already detected JSON
        let cache = ParserCache::new();
        let metrics = ParsingMetrics::new();
        cache.set_format("c1".to_string(), LogFormat::Json);

        // This line looks like plain text, but cache wins
        let format = LogServiceImpl::resolve_format(
            "c1", &HashMap::new(), &cache, b"plain text line", &metrics,
        );
        assert_eq!(format, LogFormat::Json, "Cache hit should override heuristic");
    }

    #[test]
    fn resolve_heuristic_json_first_line() {
        let cache = ParserCache::new();
        let metrics = ParsingMetrics::new();

        let format = LogServiceImpl::resolve_format(
            "c1", &HashMap::new(), &cache,
            br#"{"level":"info","msg":"started"}"#, &metrics,
        );
        assert_eq!(format, LogFormat::Json);
        // Verify it was cached for subsequent lines
        assert_eq!(cache.get_format("c1"), Some(LogFormat::Json));
    }

    #[test]
    fn resolve_heuristic_logfmt_first_line() {
        let cache = ParserCache::new();
        let metrics = ParsingMetrics::new();

        let format = LogServiceImpl::resolve_format(
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

        let format = LogServiceImpl::resolve_format(
            "c1", &HashMap::new(), &cache,
            b"2026-01-01 INFO  Application started", &metrics,
        );
        assert_eq!(format, LogFormat::PlainText);
        assert_eq!(cache.get_format("c1"), Some(LogFormat::PlainText));
    }

    #[test]
    fn resolve_disabled_container_returns_no_cache() {
        // When parsing is disabled for a container, cache returns None,
        // so resolve_format falls through to heuristic
        let cache = ParserCache::new();
        let metrics = ParsingMetrics::new();
        cache.set_format("c1".to_string(), LogFormat::Json);
        cache.disable_parsing("c1");

        // is_disabled check happens in stream_logs BEFORE resolve_format is called,
        // so resolve_format itself doesn't see disabled state.
        // But if it DID get called, cache.get_format returns None → falls to heuristic
        let format = LogServiceImpl::resolve_format(
            "c1", &HashMap::new(), &cache,
            br#"{"level":"error"}"#, &metrics,
        );
        // Since cache is disabled → get_format returns None → heuristic runs
        assert_eq!(format, LogFormat::Json, "Heuristic should detect JSON");
    }

    #[test]
    fn resolve_label_wins_over_cache() {
        // Even if cache says Logfmt, label saying JSON should win
        let cache = ParserCache::new();
        let metrics = ParsingMetrics::new();
        cache.set_format("c1".to_string(), LogFormat::Logfmt);

        let mut labels = HashMap::new();
        labels.insert("docktail.log_format".to_string(), "json".to_string());

        let format = LogServiceImpl::resolve_format(
            "c1", &labels, &cache,
            b"plain text line", &metrics,
        );
        assert_eq!(format, LogFormat::Json, "Label always wins");
        // Cache should now be updated to JSON
        assert_eq!(cache.get_format("c1"), Some(LogFormat::Json));
    }

    #[test]
    fn resolve_empty_first_line() {
        // Container's first log line is empty (e.g., blank line before banner)
        let cache = ParserCache::new();
        let metrics = ParsingMetrics::new();

        let format = LogServiceImpl::resolve_format(
            "c1", &HashMap::new(), &cache, b"", &metrics,
        );
        assert_eq!(format, LogFormat::PlainText);
    }

    #[test]
    fn resolve_multiple_containers_independent() {
        // Each container should get its own cached format
        let cache = ParserCache::new();
        let metrics = ParsingMetrics::new();

        LogServiceImpl::resolve_format(
            "json-app", &HashMap::new(), &cache,
            br#"{"msg":"hello"}"#, &metrics,
        );
        LogServiceImpl::resolve_format(
            "logfmt-app", &HashMap::new(), &cache,
            b"level=info msg=hello", &metrics,
        );
        LogServiceImpl::resolve_format(
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

        // First call → heuristic detects JSON and caches
        LogServiceImpl::resolve_format(
            "c1", &HashMap::new(), &cache,
            br#"{"level":"info"}"#, &metrics,
        );

        // Second call with a plain text line → should still return JSON (cached)
        let format = LogServiceImpl::resolve_format(
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
        // JSON array is valid JSON but NOT a log format
        let line = b"[1, 2, 3]";
        assert_eq!(LogServiceImpl::quick_detect_format(line), LogFormat::PlainText);
    }

    #[test]
    fn detect_nested_braces_in_text() {
        // Template engine output like Jinja/Handlebars
        let line = b"Rendering template {{user.name}} failed";
        assert_eq!(LogServiceImpl::quick_detect_format(line), LogFormat::PlainText);
    }

    #[test]
    fn detect_logfmt_with_spaces_around_equals() {
        // "key = value" is NOT logfmt (logfmt requires no spaces around =)
        let line = b"key = value another = pair";
        // windows: ' '!=' ' && '='=='=' → match... but ' ' before = means w[0]==' '
        // Actually: "y " → no, " =" → w[0]=' ', w[1]='=' → filtered. "e " → no
        // So no key=value pairs detected
        assert_eq!(LogServiceImpl::quick_detect_format(line), LogFormat::PlainText);
    }

    #[test]
    fn detect_syslog_like_line() {
        // Syslog is not detected by quick_detect (handled by label or PlainText parser)
        let line = b"<134>Jan  1 00:00:00 myhost myapp[1234]: connection established";
        assert_eq!(LogServiceImpl::quick_detect_format(line), LogFormat::PlainText);
    }

    #[test]
    fn detect_docker_compose_prefix() {
        // docker-compose prepends container name
        let line = b"web-1  | {\"level\":\"info\"}";
        // Doesn't start with {, so not JSON
        assert_eq!(LogServiceImpl::quick_detect_format(line), LogFormat::PlainText);
    }

    #[test]
    fn detect_json_with_ansi_prefix() {
        // ANSI codes before JSON (should be stripped BEFORE calling detect)
        // But if they aren't, we should not crash
        let line = b"\x1b[32m{\"level\":\"info\"}\x1b[0m";
        // Doesn't start with {, so PlainText — correct, ANSI stripping
        assert_eq!(LogServiceImpl::quick_detect_format(line), LogFormat::PlainText);
    }

    #[test]
    fn detect_very_large_json() {
        // 100KB JSON object — should still be detected as JSON (just checks first/last byte)
        let mut line = Vec::with_capacity(100_000);
        line.push(b'{');
        line.extend_from_slice(b"\"key\":\"");
        line.extend(vec![b'x'; 99_980]);
        line.extend_from_slice(b"\"}");
        assert_eq!(LogServiceImpl::quick_detect_format(&line), LogFormat::Json);
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

        LogServiceImpl::resolve_format("c1", &labels, &cache, b"", &metrics);

        let snap = metrics.snapshot();
        assert_eq!(snap.detection_attempts, 1);
        assert_eq!(snap.detection_success, 1);
    }

    #[test]
    fn resolve_metrics_recorded_on_heuristic() {
        let cache = ParserCache::new();
        let metrics = ParsingMetrics::new();

        LogServiceImpl::resolve_format(
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

        // This should hit cache — no new detection recorded
        LogServiceImpl::resolve_format(
            "c1", &HashMap::new(), &cache, b"anything", &metrics,
        );

        let snap = metrics.snapshot();
        assert_eq!(snap.detection_attempts, 0, "Cache hit should not record detection");
    }
}
