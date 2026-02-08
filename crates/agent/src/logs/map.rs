//! Pure conversion functions for the log streaming RPC.
//!
//! Protobuf ↔ internal type mapping, parser selection, and request validation.

use prost_types::Timestamp as ProtoTimestamp;
use tonic::Status;

use crate::docker::stream::{LogStreamRequest as InternalLogStreamRequest, LogLevel};
use crate::filter::engine::FilterMode;
use crate::parser::{LogFormat, LogParser};
use crate::parser::formats::{JsonParser, LogfmtParser, PlainTextParser, SyslogParser, HttpLogParser};
use crate::parser::traits::ParsedLog;
use crate::proto::{
    LogStreamRequest,
    FilterMode as ProtoFilterMode,
    ParsedLog as ProtoParsedLog,
    RequestContext as ProtoRequestContext, ErrorContext as ProtoErrorContext,
    KeyValuePair, LogFormat as ProtoLogFormat,
};

/// Convert protobuf FilterMode to internal FilterMode.
/// Returns `None` for FILTER_MODE_NONE and FILTER_MODE_UNSPECIFIED,
/// meaning no filter should be applied even when a pattern is present.
pub fn convert_filter_mode(proto_mode: i32) -> Option<FilterMode> {
    match ProtoFilterMode::try_from(proto_mode) {
        Ok(ProtoFilterMode::Include) => Some(FilterMode::Include),
        Ok(ProtoFilterMode::Exclude) => Some(FilterMode::Exclude),
        _ => None, // NONE / Unspecified => no filtering
    }
}

/// Convert protobuf LogStreamRequest to internal request.
pub fn convert_request(req: LogStreamRequest) -> Result<InternalLogStreamRequest, Status> {
    // Validate that since <= until when both are provided
    if let (Some(since), Some(until)) = (req.since, req.until) {
        if since > until {
            return Err(Status::invalid_argument(
                format!("'since' ({}) must not be after 'until' ({})", since, until)
            ));
        }
    }

    let filter_mode = convert_filter_mode(req.filter_mode);

    Ok(InternalLogStreamRequest {
        container_id: req.container_id,
        since: req.since,
        until: req.until,
        follow: req.follow,
        filter_pattern: if filter_mode.is_some() { req.filter_pattern } else { None },
        filter_mode: filter_mode.unwrap_or(FilterMode::Include),
        tail_lines: req.tail_lines,
    })
}

/// Convert internal LogLevel to protobuf enum value.
pub fn convert_log_level(level: LogLevel) -> i32 {
    match level {
        LogLevel::Stdout => 1, // LOG_LEVEL_STDOUT
        LogLevel::Stderr => 2, // LOG_LEVEL_STDERR
    }
}

/// Get a parser for the given log format.
pub fn get_parser(format: LogFormat) -> Box<dyn LogParser> {
    match format {
        LogFormat::Json => Box::new(JsonParser::new()),
        LogFormat::Logfmt => Box::new(LogfmtParser),
        LogFormat::Syslog => Box::new(SyslogParser),
        LogFormat::HttpLog => Box::new(HttpLogParser),
        _ => Box::new(PlainTextParser),
    }
}

/// Convert internal ParsedLog to protobuf.
pub fn convert_parsed_log(parsed: ParsedLog) -> ProtoParsedLog {
    ProtoParsedLog {
        level: parsed.level,
        message: parsed.message,
        logger: parsed.logger,
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

/// Convert LogFormat to protobuf enum value.
pub fn convert_log_format(format: LogFormat) -> i32 {
    match format {
        LogFormat::Json => ProtoLogFormat::Json as i32,
        LogFormat::Logfmt => ProtoLogFormat::Logfmt as i32,
        LogFormat::PlainText => ProtoLogFormat::PlainText as i32,
        LogFormat::Syslog => ProtoLogFormat::Syslog as i32,
        LogFormat::HttpLog => ProtoLogFormat::HttpLog as i32,
        LogFormat::Unknown => ProtoLogFormat::Unknown as i32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docker::stream::LogLevel;

    // ── convert_filter_mode ──────────────────────────────────────

    #[test]
    fn test_convert_filter_mode_include() {
        let mode = convert_filter_mode(ProtoFilterMode::Include as i32);
        assert!(mode.is_some());
        assert!(matches!(mode.unwrap(), FilterMode::Include));
    }

    #[test]
    fn test_convert_filter_mode_exclude() {
        let mode = convert_filter_mode(ProtoFilterMode::Exclude as i32);
        assert!(mode.is_some());
        assert!(matches!(mode.unwrap(), FilterMode::Exclude));
    }

    #[test]
    fn test_convert_filter_mode_none_returns_none() {
        let mode = convert_filter_mode(ProtoFilterMode::None as i32);
        assert!(mode.is_none());
    }

    #[test]
    fn test_convert_filter_mode_unspecified_returns_none() {
        let mode = convert_filter_mode(ProtoFilterMode::Unspecified as i32);
        assert!(mode.is_none());
    }

    #[test]
    fn test_convert_filter_mode_invalid_value_returns_none() {
        let mode = convert_filter_mode(999);
        assert!(mode.is_none());
    }

    // ── convert_request ──────────────────────────────────────────

    fn make_proto_request(
        container_id: &str,
        follow: bool,
        since: Option<i64>,
        until: Option<i64>,
        filter_pattern: Option<String>,
        filter_mode: i32,
        tail_lines: Option<u32>,
    ) -> LogStreamRequest {
        LogStreamRequest {
            container_id: container_id.to_string(),
            follow,
            since,
            until,
            filter_pattern,
            filter_mode,
            tail_lines,
            timestamps: false,
            disable_parsing: false,
        }
    }

    #[test]
    fn test_convert_request_basic() {
        let req = make_proto_request("abc123", true, None, None, None, ProtoFilterMode::None as i32, Some(100));
        let internal = convert_request(req).unwrap();
        assert_eq!(internal.container_id, "abc123");
        assert!(internal.follow);
        assert_eq!(internal.tail_lines, Some(100));
        assert!(internal.filter_pattern.is_none());
    }

    #[test]
    fn test_convert_request_since_after_until_fails() {
        let req = make_proto_request("abc123", false, Some(2000), Some(1000), None, ProtoFilterMode::None as i32, None);
        let result = convert_request(req);
        assert!(result.is_err());
    }

    #[test]
    fn test_convert_request_since_equals_until_ok() {
        let req = make_proto_request("abc123", false, Some(1000), Some(1000), None, ProtoFilterMode::None as i32, None);
        assert!(convert_request(req).is_ok());
    }

    #[test]
    fn test_convert_request_filter_pattern_cleared_when_no_mode() {
        let req = make_proto_request("abc123", false, None, None, Some("error".to_string()), ProtoFilterMode::None as i32, None);
        let internal = convert_request(req).unwrap();
        // Pattern should be cleared when filter_mode is None
        assert!(internal.filter_pattern.is_none());
    }

    #[test]
    fn test_convert_request_filter_pattern_preserved_with_include() {
        let req = make_proto_request("abc123", false, None, None, Some("error".to_string()), ProtoFilterMode::Include as i32, None);
        let internal = convert_request(req).unwrap();
        assert_eq!(internal.filter_pattern, Some("error".to_string()));
    }

    // ── convert_log_level ────────────────────────────────────────

    #[test]
    fn test_convert_log_level_stdout() {
        assert_eq!(convert_log_level(LogLevel::Stdout), 1);
    }

    #[test]
    fn test_convert_log_level_stderr() {
        assert_eq!(convert_log_level(LogLevel::Stderr), 2);
    }

    // ── convert_log_format ───────────────────────────────────────

    #[test]
    fn test_convert_log_format_all_variants() {
        assert_eq!(convert_log_format(LogFormat::Json), ProtoLogFormat::Json as i32);
        assert_eq!(convert_log_format(LogFormat::Logfmt), ProtoLogFormat::Logfmt as i32);
        assert_eq!(convert_log_format(LogFormat::PlainText), ProtoLogFormat::PlainText as i32);
        assert_eq!(convert_log_format(LogFormat::Syslog), ProtoLogFormat::Syslog as i32);
        assert_eq!(convert_log_format(LogFormat::HttpLog), ProtoLogFormat::HttpLog as i32);
        assert_eq!(convert_log_format(LogFormat::Unknown), ProtoLogFormat::Unknown as i32);
    }

    // ── get_parser ───────────────────────────────────────────────

    #[test]
    fn test_get_parser_json() {
        let parser = get_parser(LogFormat::Json);
        assert!(matches!(parser.format(), LogFormat::Json));
    }

    #[test]
    fn test_get_parser_logfmt() {
        let parser = get_parser(LogFormat::Logfmt);
        assert!(matches!(parser.format(), LogFormat::Logfmt));
    }

    #[test]
    fn test_get_parser_syslog_returns_syslog_parser() {
        let parser = get_parser(LogFormat::Syslog);
        assert!(matches!(parser.format(), LogFormat::Syslog));
    }

    #[test]
    fn test_get_parser_httplog_returns_httplog_parser() {
        let parser = get_parser(LogFormat::HttpLog);
        assert!(matches!(parser.format(), LogFormat::HttpLog));
    }

    #[test]
    fn test_get_parser_unknown_falls_through_to_plain() {
        let parser = get_parser(LogFormat::Unknown);
        assert!(matches!(parser.format(), LogFormat::PlainText));

        let parser = get_parser(LogFormat::PlainText);
        assert!(matches!(parser.format(), LogFormat::PlainText));
    }

    // ── convert_parsed_log ───────────────────────────────────────

    #[test]
    fn test_convert_parsed_log_empty() {
        let parsed = ParsedLog::plain_text(bytes::Bytes::from("hello"));
        let proto = convert_parsed_log(parsed);
        assert!(proto.level.is_none());
        assert!(proto.message.is_none());
        assert!(proto.logger.is_none());
        assert!(proto.timestamp.is_none());
        assert!(proto.request.is_none());
        assert!(proto.error.is_none());
        assert!(proto.fields.is_empty());
    }

    #[test]
    fn test_convert_parsed_log_with_fields() {
        let mut parsed = ParsedLog::plain_text(bytes::Bytes::from("test"));
        parsed.fields = vec![
            ("key1".to_string(), "val1".to_string()),
            ("key2".to_string(), "val2".to_string()),
        ];
        let proto = convert_parsed_log(parsed);
        assert_eq!(proto.fields.len(), 2);
        assert_eq!(proto.fields[0].key, "key1");
        assert_eq!(proto.fields[0].value, "val1");
    }

    #[test]
    fn test_convert_parsed_log_with_timestamp() {
        let dt: chrono::DateTime<chrono::Utc> = chrono::DateTime::parse_from_rfc3339("2024-01-15T10:30:00Z").unwrap().into();
        let mut parsed = ParsedLog::plain_text(bytes::Bytes::from("test"));
        parsed.timestamp = Some(dt);
        let proto = convert_parsed_log(parsed);
        assert!(proto.timestamp.is_some());
        let ts = proto.timestamp.unwrap();
        assert_eq!(ts.seconds, dt.timestamp());
        assert_eq!(ts.nanos, 0);
    }
}
