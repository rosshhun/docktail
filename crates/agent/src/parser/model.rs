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
    #[serde(
        serialize_with = "serialize_fields_as_map",
        deserialize_with = "deserialize_fields_from_map"
    )]
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
