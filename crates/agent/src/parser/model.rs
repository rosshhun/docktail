use std::time::Duration;
use thiserror::Error;
use serde::{Serialize, Deserialize};
use chrono::{DateTime, Utc};
use super::serde_utils::serialize_fields_as_map;


#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogFormat {
    /// JSON structured logs (most common)
    Json,
    /// Logfmt key=value format (popular in Go apps)
    Logfmt,
    /// Syslog format (RFC 3164 / RFC 5424)
    Syslog,
    /// Apache/Nginx access logs
    HttpLog,
    /// Plain text fallback (no structure)
    PlainText,
    /// Unknown/undetected format
    Unknown,
}

impl LogFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            LogFormat::Json => "json",
            LogFormat::Logfmt => "logfmt",
            LogFormat::Syslog => "syslog",
            LogFormat::HttpLog => "http_log",
            LogFormat::PlainText => "plain_text",
            LogFormat::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone)]
pub struct DetectionResult {
    pub format: LogFormat,
    /// Confidence level (0.0 - 1.0)
    /// - 0.0-0.5: Low confidence (might be wrong)
    /// - 0.5-0.8: Medium confidence (likely correct)
    /// - 0.8-1.0: High confidence (very likely correct)
    pub confidence: f32,
}

impl DetectionResult {
    pub fn new(format: LogFormat, confidence: f32) -> Self {
        Self {
            format,
            confidence: confidence.clamp(0.0, 1.0),
        }
    }

    pub fn no_match() -> Self {
        Self {
            format: LogFormat::Unknown,
            confidence: 0.0,
        }
    }

    pub fn low_confidence(confidence: f32) -> Self {
        Self {
            format: LogFormat::PlainText,
            confidence: confidence.clamp(0.0, 0.5),
        }
    }

    pub fn match_with_confidence(format: LogFormat, confidence: f32) -> Self {
        Self::new(format, confidence)
    }

    pub fn is_high_confidence(&self) -> bool {
        self.confidence >= super::HIGH_CONFIDENCE_THRESHOLD
    }

    pub fn is_medium_confidence(&self) -> bool {
        self.confidence >= super::MEDIUM_CONFIDENCE_THRESHOLD
    }
}

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("Invalid format: {0}")]
    InvalidFormat(String),

    #[error("Timeout after {0:?}")]
    Timeout(Duration),

    #[error("Line too large: {0} bytes (max: {1} bytes)")]
    LineTooLarge(usize, usize),

    #[error("Non-UTF8 content")]
    NonUtf8,

    #[error("Parser panic: {0}")]
    ParserPanic(String),

    #[error("Parse failed: {0}")]
    ParseFailed(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct ParsedLog {
    /// Log level (info, warn, error, debug, etc.)
    pub level: Option<String>,
    
    /// Main log message
    pub message: Option<String>,
    
    /// Logger name (e.g., "app.users.service")
    pub logger: Option<String>,
    
    /// Application-provided timestamp (may differ from Docker timestamp)
    /// Serializes as ISO-8601 string automatically
    pub timestamp: Option<DateTime<Utc>>,
    
    /// Request context (for web apps)
    pub request: Option<RequestContext>,
    
    pub error: Option<ErrorContext>,
    
    /// Additional structured fields (key-value pairs)
    /// Serialized as a JSON object for efficient downstream processing
    #[serde(serialize_with = "serialize_fields_as_map")]
    pub fields: Vec<(String, String)>,
    
    /// Original raw content (always preserved)
    /// Skipped during serialization to save bandwidth - raw logs stored separately
    #[serde(skip)]
    pub raw_content: bytes::Bytes,
}

impl ParsedLog {
    /// Create a plain text log entry (no parsing)
    pub fn plain_text(raw: bytes::Bytes) -> Self {
        Self {
            level: None,
            message: None,
            logger: None,
            timestamp: None,
            request: None,
            error: None,
            fields: Vec::new(),
            raw_content: raw,
        }
    }

    /// Create from raw bytes with basic message extraction
    pub fn with_message(raw: bytes::Bytes, message: String) -> Self {
        Self {
            level: None,
            message: Some(message),
            logger: None,
            timestamp: None,
            request: None,
            error: None,
            fields: Vec::new(),
            raw_content: raw,
        }
    }
}

/// HTTP request context
#[derive(Debug, Clone, Serialize)]
pub struct RequestContext {
    pub method: Option<String>,        // GET, POST, etc.
    pub path: Option<String>,          // /api/users
    pub remote_addr: Option<String>,   // Client IP
    pub status_code: Option<i32>,      // HTTP status
    pub duration_ms: Option<i64>,      // Request duration
    pub request_id: Option<String>,    // Correlation ID
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorContext {
    pub error_type: Option<String>,    // Exception class
    pub error_message: Option<String>,
    pub stack_trace: Vec<String>,
    pub file: Option<String>,
    pub line: Option<i32>,
}


#[derive(Debug, Clone, Serialize)]
pub struct ParseMetadata {
    pub detected_format: LogFormat,
    pub parse_success: bool,
    pub parse_error: Option<String>,
    pub parse_time_nanos: i64,
}

impl ParseMetadata {
    pub fn success(format: LogFormat, parse_time_nanos: i64) -> Self {
        Self {
            detected_format: format,
            parse_success: true,
            parse_error: None,
            parse_time_nanos,
        }
    }

    pub fn failed(format: LogFormat, error: ParseError, parse_time_nanos: i64) -> Self {
        Self {
            detected_format: format,
            parse_success: false,
            parse_error: Some(error.to_string()),
            parse_time_nanos,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── LogFormat ────────────────────────────────────────────────

    #[test]
    fn test_log_format_as_str() {
        assert_eq!(LogFormat::Json.as_str(), "json");
        assert_eq!(LogFormat::Logfmt.as_str(), "logfmt");
        assert_eq!(LogFormat::Syslog.as_str(), "syslog");
        assert_eq!(LogFormat::HttpLog.as_str(), "http_log");
        assert_eq!(LogFormat::PlainText.as_str(), "plain_text");
        assert_eq!(LogFormat::Unknown.as_str(), "unknown");
    }

    #[test]
    fn test_log_format_serde_round_trip() {
        let format = LogFormat::Json;
        let json = serde_json::to_string(&format).unwrap();
        let deserialized: LogFormat = serde_json::from_str(&json).unwrap();
        assert_eq!(format, deserialized);
    }

    #[test]
    fn test_log_format_serde_snake_case() {
        let json = serde_json::to_string(&LogFormat::PlainText).unwrap();
        assert_eq!(json, r#""plain_text""#);

        let json = serde_json::to_string(&LogFormat::HttpLog).unwrap();
        assert_eq!(json, r#""http_log""#);
    }

    #[test]
    fn test_log_format_ordering() {
        // LogFormat derives Ord, so verify ordering is consistent
        assert!(LogFormat::Json < LogFormat::Logfmt);
        assert!(LogFormat::PlainText < LogFormat::Unknown);
    }

    // ── DetectionResult ──────────────────────────────────────────

    #[test]
    fn test_detection_result_new_clamps_confidence() {
        let result = DetectionResult::new(LogFormat::Json, 1.5);
        assert_eq!(result.confidence, 1.0);

        let result = DetectionResult::new(LogFormat::Json, -0.5);
        assert_eq!(result.confidence, 0.0);

        let result = DetectionResult::new(LogFormat::Json, 0.75);
        assert_eq!(result.confidence, 0.75);
    }

    #[test]
    fn test_detection_result_no_match() {
        let result = DetectionResult::no_match();
        assert!(matches!(result.format, LogFormat::Unknown));
        assert_eq!(result.confidence, 0.0);
    }

    #[test]
    fn test_detection_result_low_confidence_clamps_to_half() {
        let result = DetectionResult::low_confidence(0.8);
        assert!(matches!(result.format, LogFormat::PlainText));
        assert_eq!(result.confidence, 0.5); // clamped to [0.0, 0.5]
    }

    #[test]
    fn test_detection_result_low_confidence_in_range() {
        let result = DetectionResult::low_confidence(0.3);
        assert_eq!(result.confidence, 0.3);
    }

    #[test]
    fn test_detection_result_match_with_confidence() {
        let result = DetectionResult::match_with_confidence(LogFormat::Logfmt, 0.85);
        assert!(matches!(result.format, LogFormat::Logfmt));
        assert_eq!(result.confidence, 0.85);
    }

    #[test]
    fn test_detection_result_is_high_confidence() {
        let high = DetectionResult::new(LogFormat::Json, 0.96);
        assert!(high.is_high_confidence());

        let low = DetectionResult::new(LogFormat::Json, 0.5);
        assert!(!low.is_high_confidence());
    }

    #[test]
    fn test_detection_result_is_medium_confidence() {
        let medium = DetectionResult::new(LogFormat::Json, 0.75);
        assert!(medium.is_medium_confidence());

        let low = DetectionResult::new(LogFormat::Json, 0.5);
        assert!(!low.is_medium_confidence());
    }

    // ── ParseError ───────────────────────────────────────────────

    #[test]
    fn test_parse_error_display() {
        let err = ParseError::InvalidFormat("bad data".to_string());
        assert_eq!(err.to_string(), "Invalid format: bad data");

        let err = ParseError::Timeout(Duration::from_millis(500));
        assert!(err.to_string().contains("500ms"));

        let err = ParseError::LineTooLarge(2000000, 1048576);
        assert!(err.to_string().contains("2000000"));

        let err = ParseError::NonUtf8;
        assert_eq!(err.to_string(), "Non-UTF8 content");

        let err = ParseError::ParserPanic("segfault".to_string());
        assert!(err.to_string().contains("segfault"));

        let err = ParseError::ParseFailed("reason".to_string());
        assert!(err.to_string().contains("reason"));
    }

    // ── ParsedLog ────────────────────────────────────────────────

    #[test]
    fn test_parsed_log_plain_text() {
        let raw = bytes::Bytes::from("hello world");
        let log = ParsedLog::plain_text(raw.clone());
        assert!(log.level.is_none());
        assert!(log.message.is_none());
        assert!(log.logger.is_none());
        assert!(log.timestamp.is_none());
        assert!(log.request.is_none());
        assert!(log.error.is_none());
        assert!(log.fields.is_empty());
        assert_eq!(log.raw_content, raw);
    }

    #[test]
    fn test_parsed_log_with_message() {
        let raw = bytes::Bytes::from("INFO: started");
        let log = ParsedLog::with_message(raw.clone(), "started".to_string());
        assert_eq!(log.message, Some("started".to_string()));
        assert_eq!(log.raw_content, raw);
        assert!(log.level.is_none());
    }

    #[test]
    fn test_parsed_log_serialization_skips_raw_content() {
        let raw = bytes::Bytes::from("raw data");
        let log = ParsedLog::plain_text(raw);
        let json = serde_json::to_string(&log).unwrap();
        assert!(!json.contains("raw_content"), "raw_content should be skipped in serialization");
        assert!(!json.contains("raw data"));
    }

    // ── ParseMetadata ────────────────────────────────────────────

    #[test]
    fn test_parse_metadata_success() {
        let meta = ParseMetadata::success(LogFormat::Json, 1500);
        assert!(meta.parse_success);
        assert!(meta.parse_error.is_none());
        assert_eq!(meta.parse_time_nanos, 1500);
        assert!(matches!(meta.detected_format, LogFormat::Json));
    }

    #[test]
    fn test_parse_metadata_failed() {
        let err = ParseError::NonUtf8;
        let meta = ParseMetadata::failed(LogFormat::Unknown, err, 200);
        assert!(!meta.parse_success);
        assert!(meta.parse_error.is_some());
        assert!(meta.parse_error.unwrap().contains("Non-UTF8"));
        assert_eq!(meta.parse_time_nanos, 200);
    }
}
