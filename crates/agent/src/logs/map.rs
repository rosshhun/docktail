//! Pure conversion functions for the log streaming RPC.
//!
//! Protobuf ↔ internal type mapping, parser selection, and request validation.

use prost_types::Timestamp as ProtoTimestamp;
use tonic::Status;

use crate::docker::stream::{LogStreamRequest as InternalLogStreamRequest, LogLevel};
use crate::filter::engine::FilterMode;
use crate::parser::{LogFormat, LogParser};
use crate::parser::formats::{JsonParser, LogfmtParser, PlainTextParser};
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
