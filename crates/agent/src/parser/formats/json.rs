use crate::parser::traits::*;
use crate::parser::MAX_LINE_SIZE;
use bytes::Bytes;
use serde_json::Value;

/// Default maximum size to parse during detection (1KB)
/// Prevents DoS attacks with large JSON payloads
const DEFAULT_MAX_DETECTION_SIZE: usize = 1024;

/// Configuration for JSON parser
#[derive(Debug, Clone)]
pub struct JsonParserConfig {
    /// Maximum event size to prevent DoS attacks (default: 1MB)
    pub max_event_size: usize,
    /// Maximum size to fully parse during detection (default: 1KB)
    pub max_detection_size: usize,
    /// Whether to flatten nested objects/arrays as JSON strings (default: false)
    /// When false, nested structures are preserved as serialized JSON
    pub flatten_nested: bool,
}

impl Default for JsonParserConfig {
    fn default() -> Self {
        Self {
            max_event_size: MAX_LINE_SIZE,
            max_detection_size: DEFAULT_MAX_DETECTION_SIZE,
            flatten_nested: false,
        }
    }
}

/// JSON format detector
pub struct JsonDetector {
    config: JsonParserConfig,
}

impl JsonDetector {
    pub fn new() -> Self {
        Self {
            config: JsonParserConfig::default(),
        }
    }

    pub fn with_config(config: JsonParserConfig) -> Self {
        Self { config }
    }
}

impl Default for JsonDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl FormatDetector for JsonDetector {
    fn detect(&self, sample: &[u8]) -> DetectionResult {
        // Security: Reject oversized samples immediately
        if sample.len() > self.config.max_event_size {
            return DetectionResult::no_match();
        }

        // Quick reject: must start with '{'
        if !sample.starts_with(b"{") {
            return DetectionResult::no_match();
        }

        // Quick reject: must end with '}' (accounting for newline)
        let trimmed = trim_ascii_end(sample);
        if !trimmed.ends_with(b"}") {
            return DetectionResult::no_match();
        }

        // Smart detection: Full parse for small logs, heuristics for large logs
        if sample.len() <= self.config.max_detection_size {
            // Small log: Parse fully for accuracy
            match serde_json::from_slice::<Value>(sample) {
                Ok(value) => {
                    if value.is_object() {
                        let confidence = calculate_json_confidence(&value);
                        return DetectionResult::match_with_confidence(LogFormat::Json, confidence);
                    } else {
                        // JSON array or primitive - not a log format
                        return DetectionResult::low_confidence(0.3);
                    }
                }
                Err(_) => return DetectionResult::no_match(),
            }
        }

        // Large log: Use fast heuristic without parsing
        // Scan first N bytes for JSON field signatures
        let prefix = &sample[..self.config.max_detection_size];
        let mut score = 0.5; // Base score for valid structure (starts/ends with {})
        
        // Look for common log field patterns using fast byte search
        if has_json_field(prefix, "level") || has_json_field(prefix, "severity") || has_json_field(prefix, "lvl") {
            score += 0.2;
        }
        if has_json_field(prefix, "timestamp") || has_json_field(prefix, "time") || has_json_field(prefix, "ts") {
            score += 0.15;
        }
        if has_json_field(prefix, "message") || has_json_field(prefix, "msg") {
            score += 0.15;
        }
        if has_json_field(prefix, "logger") || has_json_field(prefix, "component") {
            score += 0.1;
        }

        // If we found common log fields, high confidence it's JSON
        if score >= 0.7 {
            DetectionResult::match_with_confidence(LogFormat::Json, score)
        } else {
            // Starts/ends with {}, but no common log fields found
            // Still JSON, but with lower confidence (let other detectors compete)
            DetectionResult::match_with_confidence(LogFormat::Json, score)
        }
    }

    fn format(&self) -> LogFormat {
        LogFormat::Json
    }
}

/// JSON parser
pub struct JsonParser {
    config: JsonParserConfig,
}

impl JsonParser {
    pub fn new() -> Self {
        Self {
            config: JsonParserConfig::default(),
        }
    }

    pub fn with_config(config: JsonParserConfig) -> Self {
        Self { config }
    }
}

impl Default for JsonParser {
    fn default() -> Self {
        Self::new()
    }
}

impl LogParser for JsonParser {
    fn parse(&self, raw: &[u8]) -> Result<ParsedLog, ParseError> {
        // Security: Enforce size limit to prevent DoS
        if raw.len() > self.config.max_event_size {
            return Err(ParseError::LineTooLarge(raw.len(), self.config.max_event_size));
        }

        // Parse JSON
        let value: Value = serde_json::from_slice(raw)
            .map_err(|e| ParseError::ParseFailed(format!("Invalid JSON: {}", e)))?;

        let obj = value.as_object()
            .ok_or_else(|| ParseError::InvalidFormat("JSON is not an object".to_string()))?;

        // Extract common log fields
        let level = extract_string_field(obj, &["level", "lvl", "severity", "loglevel"]);
        let message = extract_string_field(obj, &["message", "msg", "text", "log"]);
        let logger = extract_string_field(obj, &["logger", "name", "component", "service"]);
        
        // Extract timestamp (various formats)
        let timestamp = extract_timestamp(obj);

        // Extract request context if present
        let request = extract_request_context(obj);

        // Extract error context if present
        let error = extract_error_context(obj);

        // Extract all other fields
        let fields = extract_additional_fields(
            obj,
            &[
                "level", "lvl", "severity", "loglevel",
                "message", "msg", "text", "log",
                "logger", "name", "component", "service",
                "timestamp", "time", "ts", "@timestamp",
                "method", "path", "status", "status_code",
                "error", "err", "exception",
            ],
            self.config.flatten_nested,
        );

        Ok(ParsedLog {
            level,
            message,
            logger,
            timestamp,
            request,
            error,
            fields,
            raw_content: Bytes::copy_from_slice(raw),
        })
    }

    fn format(&self) -> LogFormat {
        LogFormat::Json
    }
}

// Helper functions

fn trim_ascii_end(bytes: &[u8]) -> &[u8] {
    let mut end = bytes.len();
    while end > 0 && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    &bytes[..end]
}

/// Fast byte-level search for JSON field pattern: "key": or "key" :
/// This is a heuristic for detection and much faster than parsing
/// Handles both compact and pretty-printed JSON
/// Example: {"message": "I have \"level\": here"} won't match "level" as a field
fn has_json_field(chunk: &[u8], key: &str) -> bool {
    let key_bytes = key.as_bytes();
    let min_pattern_len = key_bytes.len() + 3; // "key":
    
    if chunk.len() < min_pattern_len {
        return false;
    }
    
    // State machine to track if we're inside a string value
    let mut in_string_value = false;
    let mut escape_next = false;
    let mut after_colon = false;
    
    // Search for "key" followed by optional whitespace and ":"
    // This handles: "key":, "key" :, "key": , "key" : 
    let mut i = 0;
    while i < chunk.len() {
        let byte = chunk[i];
        
        // Handle escape sequences
        if escape_next {
            escape_next = false;
            i += 1;
            continue;
        }
        
        if byte == b'\\' {
            escape_next = true;
            i += 1;
            continue;
        }
        
        // Track string values (content after ":")
        if byte == b'"' {
            if after_colon {
                // Toggle in_string_value when we see quotes after a colon
                in_string_value = !in_string_value;
            } else if !in_string_value {
                // This could be a field name
                // Check if key matches
                if i + 1 + key_bytes.len() < chunk.len()
                    && &chunk[i + 1..i + 1 + key_bytes.len()] == key_bytes
                    && chunk[i + 1 + key_bytes.len()] == b'"'
                {
                    // Found potential key, now look for colon
                    let after_key = i + 1 + key_bytes.len() + 1;
                    let mut pos = after_key;
                    while pos < chunk.len() && chunk[pos].is_ascii_whitespace() {
                        pos += 1;
                    }
                    
                    if pos < chunk.len() && chunk[pos] == b':' {
                        return true;
                    }
                }
            }
        } else if byte == b':' && !in_string_value {
            after_colon = true;
        } else if (byte == b',' || byte == b'}') && !in_string_value {
            after_colon = false;
        }
        
        i += 1;
    }
    
    false
}

fn calculate_json_confidence(value: &Value) -> f32 {
    let obj = match value.as_object() {
        Some(o) => o,
        None => return 0.0,
    };
    
    let mut score: f32 = 0.5; // Base score for valid JSON object

    // Common log fields (boost confidence)
    let level_fields = ["level", "lvl", "severity"];
    let message_fields = ["message", "msg", "text"];
    let time_fields = ["timestamp", "time", "ts", "@timestamp"];
    let logger_fields = ["logger", "name", "component"];

    if level_fields.iter().any(|f| obj.contains_key(*f)) {
        score += 0.15;
    }
    if message_fields.iter().any(|f| obj.contains_key(*f)) {
        score += 0.15;
    }
    if time_fields.iter().any(|f| obj.contains_key(*f)) {
        score += 0.1;
    }
    if logger_fields.iter().any(|f| obj.contains_key(*f)) {
        score += 0.1;
    }

    score.min(1.0)
}

fn extract_string_field(obj: &serde_json::Map<String, Value>, field_names: &[&str]) -> Option<String> {
    for field in field_names {
        if let Some(value) = obj.get(*field) {
            let result = match value {
                Value::String(s) => Some(s.clone()),
                Value::Number(n) => Some(n.to_string()),
                Value::Bool(true) => Some("true".to_string()),
                Value::Bool(false) => Some("false".to_string()),
                _ => None,
            };
            
            if result.is_some() {
                return result;
            }
        }
    }
    None
}

fn extract_timestamp(obj: &serde_json::Map<String, Value>) -> Option<chrono::DateTime<chrono::Utc>> {
    let time_fields = ["timestamp", "time", "ts", "@timestamp"];
    
    for field in time_fields {
        if let Some(value) = obj.get(field) {
            let result = match value {
                Value::Number(n) => {
                    // Try as Unix timestamp (seconds or milliseconds)
                    n.as_i64().and_then(|ts| {
                        if ts > 1_000_000_000_000 {
                            // Milliseconds
                            chrono::DateTime::from_timestamp_millis(ts)
                        } else {
                            // Seconds
                            chrono::DateTime::from_timestamp(ts, 0)
                        }
                    })
                },
                Value::String(s) => {
                    // Try parsing ISO 8601 first
                    chrono::DateTime::parse_from_rfc3339(s)
                        .ok()
                        .map(|dt| dt.with_timezone(&chrono::Utc))
                        .or_else(|| {
                            // Try as Unix timestamp string
                            s.parse::<i64>().ok().and_then(|ts| {
                                if ts > 1_000_000_000_000 {
                                    chrono::DateTime::from_timestamp_millis(ts)
                                } else {
                                    chrono::DateTime::from_timestamp(ts, 0)
                                }
                            })
                        })
                }
                _ => None,
            };
            
            if result.is_some() {
                return result;
            }
        }
    }
    None
}

fn extract_request_context(obj: &serde_json::Map<String, Value>) -> Option<RequestContext> {
    // Look for HTTP request fields
    let method = extract_string_field(obj, &["method", "http_method", "request_method"]);
    let path = extract_string_field(obj, &["path", "url", "uri", "request_uri"]);
    let remote_addr = extract_string_field(obj, &["remote_addr", "ip", "client_ip", "remote_ip"]);
    let status_code = extract_string_field(obj, &["status", "status_code", "http_status"])
        .and_then(|s| s.parse::<i32>().ok());
    let duration_ms = extract_string_field(obj, &["duration", "duration_ms", "response_time"])
        .and_then(|s| s.parse::<i64>().ok());
    let request_id = extract_string_field(obj, &["request_id", "trace_id", "correlation_id"]);

    // Only create if we have at least one field
    if method.is_some() || path.is_some() || status_code.is_some() {
        Some(RequestContext {
            method,
            path,
            remote_addr,
            status_code,
            duration_ms,
            request_id,
        })
    } else {
        None
    }
}

fn extract_error_context(obj: &serde_json::Map<String, Value>) -> Option<ErrorContext> {
    // Look for error fields
    let error_type = extract_string_field(obj, &["error_type", "exception", "error_class"]);
    let error_message = extract_string_field(obj, &["error", "err", "error_message", "exception_message"]);
    
    let mut stack_trace = Vec::new();
    let stack_keys = ["stack_trace", "stacktrace", "stack"];
    
    for key in stack_keys {
        if let Some(v) = obj.get(key) {
            let extracted = match v {
                Value::Array(arr) => {
                    let lines: Vec<String> = arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect();
                    if !lines.is_empty() { Some(lines) } else { None }
                },
                Value::String(s) => {
                     let lines: Vec<String> = s.lines().map(|l| l.to_string()).collect();
                     if !lines.is_empty() { Some(lines) } else { None }
                },
                _ => None,
            };
            
            if let Some(lines) = extracted {
                stack_trace = lines;
                break;
            }
        }
    }

    let file = extract_string_field(obj, &["file", "filename", "source_file"]);
    let line = extract_string_field(obj, &["line", "line_number"])
        .and_then(|s| s.parse::<i32>().ok());

    // Only create if we have an error
    if error_type.is_some() || error_message.is_some() || !stack_trace.is_empty() {
        Some(ErrorContext {
            error_type,
            error_message,
            stack_trace,
            file,
            line,
        })
    } else {
        None
    }
}

fn extract_additional_fields(
    obj: &serde_json::Map<String, Value>,
    excluded_fields: &[&str],
    flatten_nested: bool,
) -> Vec<(String, String)> {
    // Pre-allocate with estimated capacity to reduce reallocations
    let estimated_capacity = obj.len().saturating_sub(excluded_fields.len());
    let mut fields = Vec::with_capacity(estimated_capacity);
    
    for (key, value) in obj.iter() {
        // Skip excluded fields
        if excluded_fields.contains(&key.as_str()) {
            continue;
        }
        
        // Extract value efficiently - avoid unnecessary allocations
        let value_str = match value {
            Value::String(s) => {
                // For strings, we must clone since we need owned String
                // But serde_json already has it allocated
                s.clone()
            }
            Value::Number(n) => {
                // Numbers must be converted to string
                n.to_string()
            }
            Value::Bool(true) => {
                // Use static strings for booleans to avoid allocation
                "true".to_string()
            }
            Value::Bool(false) => {
                "false".to_string()
            }
            Value::Null => {
                // Static string for null
                "null".to_string()
            }
            // FIXED: Preserve nested structures instead of dropping them
            Value::Object(_) | Value::Array(_) => {
                if flatten_nested {
                    // Legacy behavior: skip nested structures
                    continue;
                } else {
                    // Serialize nested structures as compact JSON
                    // This preserves full data fidelity while staying human-readable
                    // Downstream systems can parse this back into structured data
                    match serde_json::to_string(value) {
                        Ok(json_str) => json_str,
                        Err(_) => {
                            // Fallback: skip if serialization fails (shouldn't happen)
                            continue;
                        }
                    }
                }
            }
        };
        
        fields.push((key.clone(), value_str));
    }
    
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_detector_valid() {
        let detector = JsonDetector::new();

        let samples: Vec<&[u8]> = vec![
            br#"{"level":"info","msg":"hello"}"#,
            br#"{"timestamp":"2026-01-29","text":"data"}"#,
            br#"{"severity":"error","message":"failed"}"#,
        ];

        for sample in samples {
            let result = detector.detect(sample);
            assert_eq!(result.format, LogFormat::Json);
            assert!(result.confidence > 0.5, "Expected high confidence for {:?}", 
                std::str::from_utf8(sample));
        }
    }

    #[test]
    fn test_json_detector_invalid() {
        let detector = JsonDetector::new();

        let samples: Vec<&[u8]> = vec![
            b"not json at all",
            b"{incomplete",
            b"[]", // Array, not object
            b"123", // Number
            b"\"string\"", // String
        ];

        for sample in samples {
            let result = detector.detect(sample);
            assert!(result.format != LogFormat::Json || result.confidence < 0.5);
        }
    }

    #[test]
    fn test_json_parser_basic() {
        let parser = JsonParser::new();

        let sample = br#"{"level":"info","msg":"hello world","logger":"app.test"}"#;
        let parsed = parser.parse(sample).unwrap();

        assert_eq!(parsed.level, Some("info".to_string()));
        assert_eq!(parsed.message, Some("hello world".to_string()));
        assert_eq!(parsed.logger, Some("app.test".to_string()));
    }

    #[test]
    fn test_json_parser_with_request() {
        let parser = JsonParser::new();

        let sample = br#"{"level":"info","msg":"request","method":"GET","path":"/api/users","status":200}"#;
        let parsed = parser.parse(sample).unwrap();

        assert!(parsed.request.is_some());
        let req = parsed.request.unwrap();
        assert_eq!(req.method, Some("GET".to_string()));
        assert_eq!(req.path, Some("/api/users".to_string()));
        assert_eq!(req.status_code, Some(200));
    }

    #[test]
    fn test_json_parser_malformed() {
        let parser = JsonParser::new();

        let malformed = br#"{"level":"inf"#;
        let result = parser.parse(malformed);
        
        assert!(result.is_err());
    }

    #[test]
    fn test_json_detector_large_log() {
        let detector = JsonDetector::new();

        // Create a large JSON log (>1KB) to test heuristic detection
        let large_json = format!(
            r#"{{"level":"info","message":"{}","timestamp":"2026-01-30T12:00:00Z","service":"test"}}"#,
            "x".repeat(2000) // 2KB of data in message field
        );

        let result = detector.detect(large_json.as_bytes());
        
        // Should detect as JSON using heuristics (not full parse)
        assert_eq!(result.format, LogFormat::Json);
        assert!(result.confidence >= 0.7, "Expected high confidence for large JSON log");
        
        // Verify it can still be parsed
        let parser = JsonParser::new();
        let parsed = parser.parse(large_json.as_bytes()).unwrap();
        assert_eq!(parsed.level, Some("info".to_string()));
    }

    #[test]
    fn test_json_detector_rejects_oversized() {
        let detector = JsonDetector::new();

        // Create an oversized log (>1MB)
        let oversized = format!(
            r#"{{"message":"{}"}}"#,
            "x".repeat(2_000_000) // 2MB
        );

        let result = detector.detect(oversized.as_bytes());
        
        // Should reject immediately for security
        assert_ne!(result.format, LogFormat::Json);
        
        // Parser should also reject
        let parser = JsonParser::new();
        let parse_result = parser.parse(oversized.as_bytes());
        assert!(parse_result.is_err());
        if let Err(e) = parse_result {
            assert!(matches!(e, ParseError::LineTooLarge(_, _)));
        }
    }

    #[test]
    fn test_json_detector_pretty_printed() {
        let detector = JsonDetector::new();

        // Create a large pretty-printed JSON (with spaces around colons)
        let pretty_json = format!(
            r#"{{
                "level" : "info",
                "message" : "{}",
                "timestamp" : "2026-01-30T12:00:00Z",
                "service" : "test"
            }}"#,
            "x".repeat(1500) // Make it >1KB to trigger heuristic path
        );

        let result = detector.detect(pretty_json.as_bytes());
        
        // Should detect even with spaces around colons
        assert_eq!(result.format, LogFormat::Json);
        assert!(result.confidence >= 0.7, "Should handle pretty-printed JSON");
        
        // Verify it can still be parsed
        let parser = JsonParser::new();
        let parsed = parser.parse(pretty_json.as_bytes()).unwrap();
        assert_eq!(parsed.level, Some("info".to_string()));
    }

    #[test]
    fn test_nested_json_preservation() {
        let parser = JsonParser::new();

        // JSON with nested objects and arrays
        let sample = br#"{"level":"info","msg":"test","user":{"id":123,"name":"Alice","roles":["admin","user"]},"metadata":{"region":"us-east","datacenter":"dc1"}}"#;
        let parsed = parser.parse(sample).unwrap();

        // Check that nested structures are preserved as JSON strings
        let user_field = parsed.fields.iter().find(|(k, _)| k == "user");
        assert!(user_field.is_some(), "User field should be present");
        
        if let Some((_, user_json)) = user_field {
            // Verify it's valid JSON that can be parsed back
            let user_value: Value = serde_json::from_str(user_json).unwrap();
            assert_eq!(user_value["id"], 123);
            assert_eq!(user_value["name"], "Alice");
            assert!(user_value["roles"].is_array());
        }

        let metadata_field = parsed.fields.iter().find(|(k, _)| k == "metadata");
        assert!(metadata_field.is_some(), "Metadata field should be present");
    }

    #[test]
    fn test_flatten_nested_config() {
        let config = JsonParserConfig {
            flatten_nested: true,
            ..Default::default()
        };
        let parser = JsonParser::with_config(config);

        // JSON with nested objects
        let sample = br#"{"level":"info","msg":"test","user":{"id":123,"name":"Alice"}}"#;
        let parsed = parser.parse(sample).unwrap();

        // With flatten_nested=true, nested structures should be skipped
        let user_field = parsed.fields.iter().find(|(k, _)| k == "user");
        assert!(user_field.is_none(), "Nested user field should be skipped when flatten_nested=true");
    }

    #[test]
    fn test_heuristic_false_positive_fix() {
        let detector = JsonDetector::new();

        // Create a JSON with "level" in a string value, not as a field
        // The FIXED heuristic correctly ignores these and doesn't treat them as fields
        let json_with_level_in_string = format!(
            r#"{{"message":"I am ignoring the \"level\": error here, and the \"timestamp\": too","text":"{}"}}"#,
            "x".repeat(1500) // Make it >1KB to trigger heuristic path
        );

        let result = detector.detect(json_with_level_in_string.as_bytes());
        
        // Should still detect as JSON (it's valid JSON structure starting with { and ending with })
        assert_eq!(result.format, LogFormat::Json);
        // Confidence should be lower since we don't have actual log field names
        // The fixed heuristic won't be fooled by "level" and "timestamp" inside string values
        // So we should only get the base structure score (0.5) without field bonuses
        assert!(result.confidence >= 0.5 && result.confidence < 0.7, 
            "Expected base confidence ({}) for JSON without detected log fields", result.confidence);
    }

    #[test]
    fn test_custom_max_event_size() {
        let config = JsonParserConfig {
            max_event_size: 100, // Very small limit for testing
            ..Default::default()
        };
        let parser = JsonParser::with_config(config.clone());
        let detector = JsonDetector::with_config(config);

        let large_json = format!(r#"{{"message":"{}"}}"#, "x".repeat(200));

        // Detector should reject
        let detect_result = detector.detect(large_json.as_bytes());
        assert_ne!(detect_result.format, LogFormat::Json);

        // Parser should reject
        let parse_result = parser.parse(large_json.as_bytes());
        assert!(parse_result.is_err());
        if let Err(ParseError::LineTooLarge(actual, limit)) = parse_result {
            assert_eq!(limit, 100);
            assert!(actual > 100);
        } else {
            panic!("Expected LineTooLarge error");
        }
    }

    #[test]
    fn test_deeply_nested_structures() {
        let parser = JsonParser::new();

        // Complex nested structure like you'd see in production logs
        let sample = br#"{
            "level":"info",
            "msg":"request processed",
            "user": {
                "id": 12345,
                "profile": {
                    "name": "Alice",
                    "email": "alice@example.com",
                    "preferences": {
                        "theme": "dark",
                        "notifications": ["email", "sms"]
                    }
                }
            },
            "request": {
                "headers": {
                    "user-agent": "Mozilla/5.0",
                    "accept": "application/json"
                }
            }
        }"#;

        let parsed = parser.parse(sample).unwrap();

        // Verify nested structures are preserved
        let user_field = parsed.fields.iter().find(|(k, _)| k == "user").unwrap();
        let user_value: Value = serde_json::from_str(&user_field.1).unwrap();
        
        // Navigate deep into the structure
        assert_eq!(user_value["profile"]["name"], "Alice");
        assert_eq!(user_value["profile"]["preferences"]["theme"], "dark");
        assert!(user_value["profile"]["preferences"]["notifications"].is_array());
    }

    #[test]
    fn test_array_preservation() {
        let parser = JsonParser::new();

        let sample = br#"{"level":"info","msg":"test","tags":["production","api","v2"],"counts":[1,2,3,4,5]}"#;
        let parsed = parser.parse(sample).unwrap();

        // Arrays should be preserved as JSON
        let tags_field = parsed.fields.iter().find(|(k, _)| k == "tags").unwrap();
        let tags_value: Value = serde_json::from_str(&tags_field.1).unwrap();
        assert!(tags_value.is_array());
        assert_eq!(tags_value[0], "production");

        let counts_field = parsed.fields.iter().find(|(k, _)| k == "counts").unwrap();
        let counts_value: Value = serde_json::from_str(&counts_field.1).unwrap();
        assert!(counts_value.is_array());
        assert_eq!(counts_value.as_array().unwrap().len(), 5);
    }
}
