use crate::parser::traits::*;
use bytes::Bytes;

/// Maximum event size (1MB)
const MAX_EVENT_SIZE: usize = 1_048_576;

/// Maximum detection sample size (prevent DoS)
const MAX_DETECTION_SIZE: usize = 1024;

/// Logfmt format detector
pub struct LogfmtDetector;

impl FormatDetector for LogfmtDetector {
    fn detect(&self, sample: &[u8]) -> DetectionResult {
        // Reject oversized samples immediately
        if sample.len() > MAX_EVENT_SIZE {
            return DetectionResult::no_match();
        }

        // Use only first 1KB for detection to avoid DoS
        let detection_sample = if sample.len() > MAX_DETECTION_SIZE {
            &sample[..MAX_DETECTION_SIZE]
        } else {
            sample
        };

        // 1. Tracing Format Detection (High Priority)
        if let Ok(text) = std::str::from_utf8(detection_sample) {
            if is_tracing_format(text) {
                return DetectionResult::match_with_confidence(LogFormat::Logfmt, 0.9);
            }
        }

        // 2. Logfmt Detection
        // Byte-level scan avoids UTF-8 validation cost on full string
        let mut score: f32 = 0.0;
        let mut pairs_count = 0;
        
        // Split by space to find potential "key=value" chunks
        for chunk in detection_sample.split(|b| b.is_ascii_whitespace()) {
            if chunk.is_empty() { 
                continue; 
            }
            
            // Check for '='
            let mut parts = chunk.splitn(2, |&b| b == b'=');
            if let (Some(key_bytes), Some(_val)) = (parts.next(), parts.next()) {
                if !key_bytes.is_empty() {
                    pairs_count += 1;
                    
                    // Check if key is a common log field (byte comparison is fast)
                    let key_match = matches!(key_bytes, 
                        b"level" | b"lvl" | b"severity" | 
                        b"ts" | b"time" | b"timestamp" | 
                        b"msg" | b"message" | 
                        b"logger" | b"component"
                    );
                    
                    if key_match {
                        score += 0.25;
                    }
                }
            }
        }

        if pairs_count >= 2 {
            score += 0.5;
            return DetectionResult::match_with_confidence(LogFormat::Logfmt, score.min(1.0));
        }

        DetectionResult::no_match()
    }

    fn format(&self) -> LogFormat {
        LogFormat::Logfmt
    }
}

/// Check if text matches tracing crate format: TIMESTAMP LEVEL TARGET: MESSAGE
/// handle multiple spaces between timestamp and level
fn is_tracing_format(text: &str) -> bool {
    // Use split_whitespace to handle multiple spaces
    let mut parts = text.split_whitespace();
    
    let ts = match parts.next() {
        Some(p) => p,
        None => return false,
    };
    
    // Check if it looks like a timestamp (basic ISO8601 check)
    if !ts.contains('T') && !ts.contains('-') && !ts.contains(':') {
        return false;
    }
    
    let level = match parts.next() {
        Some(p) => p,
        None => return false,
    };
    
    // Check strict level names
    let is_level = matches!(
        level.to_uppercase().as_str(),
        "TRACE" | "DEBUG" | "INFO" | "WARN" | "ERROR" | "FATAL"
    );
    
    if !is_level {
        return false;
    }
    
    // Check for target (usually ends with :)
    // Tracing output: "INFO target: message"
    if let Some(target) = parts.next() {
        if target.ends_with(':') {
            return true;
        }
    }
    
    false
}

/// Logfmt parser
pub struct LogfmtParser;

impl LogParser for LogfmtParser {
    fn parse(&self, raw: &[u8]) -> Result<ParsedLog, ParseError> {
        // Enforce size limit to prevent DoS
        if raw.len() > MAX_EVENT_SIZE {
            return Err(ParseError::LineTooLarge(raw.len(), MAX_EVENT_SIZE));
        }

        let text = std::str::from_utf8(raw)
            .map_err(|_| ParseError::NonUtf8)?
            .trim();

        // Try parsing as tracing format first
        if is_tracing_format(text) {
            return parse_tracing_format(text).ok_or(
                ParseError::ParseFailed("Failed to parse tracing format".into())
            );
        }

        // Parse directly into fields without intermediate HashMap
        let mut level = None;
        let mut message = None;
        let mut logger = None;
        let mut timestamp = None;
        let mut fields = Vec::new();
        let mut method = None;
        let mut path = None;
        let mut status_code = None;
        let mut duration_ms = None;
        let mut request_id = None;
        let mut remote_addr = None;
        let mut error_msg = None;

        let mut found_any = false;
        for (key, value) in parse_logfmt_iter(text) {
            found_any = true;
            match key.as_str() {
                "level" | "lvl" | "severity" => level = Some(value),
                "msg" | "message" | "text" => message = Some(value),
                "logger" | "name" | "component" => logger = Some(value),
                "ts" | "time" | "timestamp" => timestamp = parse_timestamp(&value),
                // Request context
                "method" => method = Some(value),
                "path" | "url" => path = Some(value),
                "status" => status_code = value.parse().ok(),
                "duration" => duration_ms = value.parse().ok(),
                "request_id" => request_id = Some(value),
                "remote_addr" | "ip" => remote_addr = Some(value),
                // Error context
                "error" | "err" => error_msg = Some(value),
                // Everything else goes to fields
                _ => fields.push((key, value)),
            }
        }

        if !found_any {
            return Err(ParseError::ParseFailed("No valid key=value pairs found".to_string()));
        }

        // Construct Request Context
        let request = if method.is_some() || path.is_some() || status_code.is_some() {
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
        };

        // Construct Error Context
        let error = if error_msg.is_some() {
            Some(ErrorContext {
                error_type: None,
                error_message: error_msg,
                stack_trace: Vec::new(),
                file: None,
                line: None,
            })
        } else {
            None
        };

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
        LogFormat::Logfmt
    }
}

// Helper functions

/// Optimized iterator that yields key-value pairs without allocating a HashMap
/// Uses manual peeking to avoid consuming delimiters
fn parse_logfmt_iter(text: &str) -> impl Iterator<Item = (String, String)> + '_ {
    let mut chars = text.chars().peekable();
    
    std::iter::from_fn(move || {
        loop {
            // 1. Skip whitespace
            while chars.peek().map_or(false, |c| c.is_whitespace()) {
                chars.next();
            }

            if chars.peek().is_none() {
                return None;
            }

            // 2. Parse Key (MANUAL LOOP to avoid consuming '=')
            let mut key = String::new();
            while let Some(&c) = chars.peek() {
                if c == '=' || c.is_whitespace() {
                    break; // Stop peeking, do not consume
                }
                key.push(c);
                chars.next(); // Consume the char we just added
            }

            if key.is_empty() {
                return None;
            }

            // 3. Expect '='
            if chars.peek() == Some(&'=') {
                chars.next(); // Consume '='
                
                // 4. Parse Value
                let value = if chars.peek() == Some(&'"') {
                    chars.next(); // Consume opening quote
                    let mut val = String::new();
                    let mut escaped = false;
                    
                    while let Some(c) = chars.next() {
                        if escaped {
                            val.push(c);
                            escaped = false;
                        } else if c == '\\' {
                            escaped = true;
                        } else if c == '"' {
                            break; // End of quote
                        } else {
                            val.push(c);
                        }
                    }
                    val
                } else {
                    // Unquoted: Read until whitespace
                    let mut val = String::new();
                    while let Some(&c) = chars.peek() {
                        if c.is_whitespace() {
                            break;
                        }
                        val.push(c);
                        chars.next();
                    }
                    val
                };
                
                return Some((key, value));
            } else {
                // Found a key but no '=', skip this token
                // Continue loop to try finding next pair
                continue;
            }
        }
    })
}


fn parse_timestamp(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    // Try parsing as RFC3339 first (most common in structured logs)
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .or_else(|| {
            // Try parsing as Unix timestamp (seconds or milliseconds)
            s.parse::<i64>().ok().and_then(|ts| {
                if ts > 1_000_000_000_000 {
                    // Milliseconds
                    chrono::DateTime::from_timestamp_millis(ts)
                } else {
                    // Seconds
                    chrono::DateTime::from_timestamp(ts, 0)
                }
            })
        })
}



/// Parse tracing crate format: TIMESTAMP LEVEL TARGET: MESSAGE
/// Example: "2026-01-30T03:18:50.827498Z  INFO cluster: Starting Docktail"
/// Fixed to handle multiple spaces
fn parse_tracing_format(text: &str) -> Option<ParsedLog> {
    // FIX: use split_whitespace to handle multiple spaces
    let mut parts = text.split_whitespace();
    
    let ts_str = parts.next()?;
    let timestamp = parse_timestamp(ts_str);
    
    let level_str = parts.next()?;
    let level = Some(level_str.to_lowercase());
    
    let target_str = parts.next()?;
    let logger = if target_str.ends_with(':') {
        Some(target_str.trim_end_matches(':').to_string())
    } else {
        None
    };

    // The rest is the message
    // Join remaining parts to reconstruct message with spaces
    let message: Vec<&str> = parts.collect();
    let message_str = if message.is_empty() {
        None
    } else {
        Some(message.join(" "))
    };
    
    Some(ParsedLog {
        level,
        message: message_str,
        logger,
        timestamp,
        request: None,
        error: None,
        fields: Vec::new(),
        raw_content: Bytes::copy_from_slice(text.as_bytes()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tracing_format_detection() {
        let detector = LogfmtDetector;
        
        let sample = b"2026-01-30T03:18:50.827498Z  INFO cluster: Starting Docktail";
        let result = detector.detect(sample);
        assert_eq!(result.format, LogFormat::Logfmt);
        assert!(result.confidence > 0.8);
    }

    #[test]
    fn test_tracing_format_parsing() {
        let parser = LogfmtParser;
        
        let sample = b"2026-01-30T03:18:50.827498Z  INFO cluster: Starting Docktail Cluster API v0.0.1";
        let result = parser.parse(sample);
        
        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert_eq!(parsed.level, Some("info".to_string()));
        assert_eq!(parsed.logger, Some("cluster".to_string()));
        assert_eq!(parsed.message, Some("Starting Docktail Cluster API v0.0.1".to_string()));
    }

    #[test]
    fn test_logfmt_detector_valid() {
        let detector = LogfmtDetector;

        let samples: Vec<&[u8]> = vec![
            b"level=info msg=hello ts=2026-01-29",
            b"severity=error message=\"something failed\" component=api",
            b"lvl=debug text=\"processing request\" duration=150",
        ];

        for sample in samples {
            let result = detector.detect(sample);
            assert_eq!(result.format, LogFormat::Logfmt);
            assert!(result.confidence > 0.5);
        }
    }

    #[test]
    fn test_logfmt_detector_invalid() {
        let detector = LogfmtDetector;

        let samples: Vec<&[u8]> = vec![
            b"no equals signs here",
            b"just some text",
            b"",
        ];

        for sample in samples {
            let result = detector.detect(sample);
            assert!(result.format != LogFormat::Logfmt || result.confidence < 0.5);
        }
    }

    #[test]
    fn test_logfmt_parser_basic() {
        let parser = LogfmtParser;

        let sample = b"level=info msg=hello logger=app.test";
        let parsed = parser.parse(sample).unwrap();

        assert_eq!(parsed.level, Some("info".to_string()));
        assert_eq!(parsed.message, Some("hello".to_string()));
        assert_eq!(parsed.logger, Some("app.test".to_string()));
    }

    #[test]
    fn test_logfmt_parser_quoted_values() {
        let parser = LogfmtParser;

        let sample = b"level=info msg=\"hello world\" path=\"/api/users\"";
        let parsed = parser.parse(sample).unwrap();

        assert_eq!(parsed.message, Some("hello world".to_string()));
    }

    #[test]
    fn test_parse_logfmt_garbage_skipping() {
        let parser = LogfmtParser;
        let sample = b"key1=value1 garbage key2=value2";
        let parsed = parser.parse(sample).unwrap();
        
        // Find key1 and key2 in fields
        let find_field = |key: &str| parsed.fields.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());
        
        assert_eq!(find_field("key1"), Some("value1"));
        assert_eq!(find_field("key2"), Some("value2"));
    }

    #[test]
    fn test_logfmt_parser_comprehensive() {
        let parser = LogfmtParser;
        
        let cases = vec![
            (
                r#"key="" empty_val="#,
                vec![("key", ""), ("empty_val", "")],
            ),
            (
                r#"mixed=value quoted="with spaces" escaped="with \"quotes\" inside""#,
                vec![
                    ("mixed", "value"),
                    ("quoted", "with spaces"),
                    ("escaped", "with \"quotes\" inside"),
                ],
            ),
            (
                r#"unicode="🧊" key.with.dots=value_with_underscores"#,
                vec![("unicode", "🧊"), ("key.with.dots", "value_with_underscores")],
            ),
            (
                r#"     leading_space=true    trailing_space=true   "#,
                vec![("leading_space", "true"), ("trailing_space", "true")],
            ),
        ];

        for (input, expected) in cases {
            let parsed = parser.parse(input.as_bytes()).unwrap();
            
            for (expected_key, expected_val) in expected {
                let found = parsed.fields.iter()
                    .find(|(k, _)| k == expected_key)
                    .map(|(_, v)| v.as_str())
                    .unwrap_or_else(|| panic!("Key {} not found in input: {}", expected_key, input));
                
                assert_eq!(found, expected_val, "Value mismatch for key {} in input: {}", expected_key, input);
            }
        }
    }
}
