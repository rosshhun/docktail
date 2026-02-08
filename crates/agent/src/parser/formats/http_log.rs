use crate::parser::traits::{FormatDetector, LogFormat, LogParser, DetectionResult, ParsedLog, ParseError, RequestContext};
use bytes::Bytes;

pub struct HttpLogDetector;

impl FormatDetector for HttpLogDetector {
    fn detect(&self, sample: &[u8]) -> DetectionResult {
        let s = match std::str::from_utf8(sample) {
            Ok(v) => v,
            Err(_) => return DetectionResult::no_match(),
        };

        let open_bracket = match s.find('[') {
            Some(i) => i,
            None => return DetectionResult::no_match(),
        };
        
        let close_bracket = match s[open_bracket..].find(']') {
            Some(i) => open_bracket + i,
            None => return DetectionResult::no_match(),
        };
        
        let date_part = &s[open_bracket+1..close_bracket];
        if !date_part.contains('/') && !date_part.contains(':') {
             return DetectionResult::no_match();
        }

        let quote_start = match s[close_bracket..].find('"') {
            Some(i) => close_bracket + i,
            None => return DetectionResult::no_match(),
        };

        let request_part = &s[quote_start+1..];
        if request_part.starts_with("GET ") || 
           request_part.starts_with("POST ") || 
           request_part.starts_with("PUT ") || 
           request_part.starts_with("DELETE ") || 
           request_part.starts_with("HEAD ") || 
           request_part.starts_with("OPTIONS ") || 
           request_part.starts_with("PATCH ") {
            return DetectionResult::new(LogFormat::HttpLog, 0.85);
        }

        DetectionResult::no_match()
    }

    fn format(&self) -> LogFormat {
        LogFormat::HttpLog
    }
}

/// Parser for HTTP access logs (Common Log Format and Combined Log Format).
///
/// Extracts remote host, identity, user, timestamp, method, path, protocol,
/// status code, response size, referrer, and user-agent.
pub struct HttpLogParser;

impl LogParser for HttpLogParser {
    fn parse(&self, raw: &[u8]) -> Result<ParsedLog, ParseError> {
        let text = std::str::from_utf8(raw)
            .map_err(|_| ParseError::NonUtf8)?
            .trim();

        // Format: host ident authuser [date] "request" status bytes ["referrer" "user-agent"]
        // Example: 127.0.0.1 - frank [10/Oct/2000:13:55:36 -0700] "GET /apache_pb.gif HTTP/1.0" 200 2326

        let open_bracket = text.find('[')
            .ok_or_else(|| ParseError::InvalidFormat("Missing timestamp bracket".into()))?;
        let close_bracket = text[open_bracket..].find(']')
            .map(|i| open_bracket + i)
            .ok_or_else(|| ParseError::InvalidFormat("Unterminated timestamp bracket".into()))?;

        // Parse host, ident, authuser (before the bracket)
        let prefix = text[..open_bracket].trim();
        let prefix_parts: Vec<&str> = prefix.split_whitespace().collect();
        let remote_addr = prefix_parts.first().map(|s| s.to_string());
        let user = prefix_parts.get(2)
            .filter(|s| **s != "-")
            .map(|s| s.to_string());

        // The part after the closing bracket
        let after_bracket = &text[close_bracket + 1..].trim_start();

        // Extract the quoted request line: "METHOD /path HTTP/x.x"
        let (method, path, status_code, response_size, referrer, user_agent) = 
            if let Some(quote_start) = after_bracket.find('"') {
                let request_str = &after_bracket[quote_start + 1..];
                if let Some(quote_end) = request_str.find('"') {
                    let request_line = &request_str[..quote_end];
                    let after_request = request_str[quote_end + 1..].trim_start();

                    // Parse method and path from request line
                    let req_parts: Vec<&str> = request_line.splitn(3, ' ').collect();
                    let method = req_parts.first().map(|s| s.to_string());
                    let path = req_parts.get(1).map(|s| s.to_string());

                    // Parse status and bytes after the request
                    let status_parts: Vec<&str> = after_request.split_whitespace().collect();
                    let status_code = status_parts.first()
                        .and_then(|s| s.parse::<i32>().ok());
                    let response_size = status_parts.get(1)
                        .and_then(|s| s.parse::<i64>().ok());

                    // Try to extract referrer and user-agent (Combined Log Format)
                    let remaining = if status_parts.len() >= 2 {
                        after_request.splitn(3, ' ').nth(2).unwrap_or("")
                    } else {
                        ""
                    };

                    let (referrer, user_agent) = extract_quoted_fields(remaining);

                    (method, path, status_code, response_size, referrer, user_agent)
                } else {
                    (None, None, None, None, None, None)
                }
            } else {
                (None, None, None, None, None, None)
            };

        // Determine log level from status code
        let level = status_code.map(|code| {
            if code >= 500 { "error" }
            else if code >= 400 { "warn" }
            else if code >= 300 { "info" }
            else { "info" }
        }.to_string());

        // Build the message from the full request
        let message = Some(text.to_string());

        let request = if method.is_some() || path.is_some() || status_code.is_some() {
            Some(RequestContext {
                method,
                path,
                remote_addr: remote_addr.clone(),
                status_code,
                duration_ms: None,
                request_id: None,
            })
        } else {
            None
        };

        let mut fields = Vec::new();
        if let Some(ref u) = user {
            fields.push(("user".to_string(), u.clone()));
        }
        if let Some(size) = response_size {
            fields.push(("response_size".to_string(), size.to_string()));
        }
        if let Some(ref r) = referrer {
            fields.push(("referrer".to_string(), r.clone()));
        }
        if let Some(ref ua) = user_agent {
            fields.push(("user_agent".to_string(), ua.clone()));
        }

        Ok(ParsedLog {
            level,
            message,
            logger: None,
            timestamp: None, // CLF timestamps lack timezone consistency; leave to caller
            request,
            error: None,
            fields,
            raw_content: Bytes::copy_from_slice(raw),
        })
    }

    fn format(&self) -> LogFormat {
        LogFormat::HttpLog
    }
}

/// Extract referrer and user-agent from remaining quoted fields.
fn extract_quoted_fields(text: &str) -> (Option<String>, Option<String>) {
    let mut referrer = None;
    let mut user_agent = None;

    let mut chars = text.chars();
    let mut fields_found = 0;

    while let Some(val) = extract_next_quoted(&mut chars) {
        match fields_found {
            0 => referrer = if val == "-" { None } else { Some(val) },
            1 => user_agent = if val == "-" { None } else { Some(val) },
            _ => break,
        }
        fields_found += 1;
    }

    (referrer, user_agent)
}

/// Extract the next quoted string from a char iterator.
fn extract_next_quoted(chars: &mut std::str::Chars<'_>) -> Option<String> {
    // Find opening quote
    loop {
        match chars.next() {
            Some('"') => break,
            Some(_) => continue,
            None => return None,
        }
    }
    // Read until closing quote
    let mut val = String::new();
    let mut escaped = false;
    for c in chars.by_ref() {
        if escaped {
            val.push(c);
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '"' {
            return Some(val);
        } else {
            val.push(c);
        }
    }
    if val.is_empty() { None } else { Some(val) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_common_log_format() {
        let detector = HttpLogDetector;
        let sample = b"127.0.0.1 - frank [10/Oct/2000:13:55:36 -0700] \"GET /apache_pb.gif HTTP/1.0\" 200 2326";
        let result = detector.detect(sample);
        assert_eq!(result.format, LogFormat::HttpLog);
        assert!(result.confidence > 0.8);
    }

    #[test]
    fn test_detect_combined_log_format() {
        let detector = HttpLogDetector;
        let sample = b"127.0.0.1 - - [29/Jan/2026:10:59:12 +0000] \"POST /api/v1/data HTTP/1.1\" 200 1024 \"-\" \"curl/7.68.0\"";
        let result = detector.detect(sample);
        assert_eq!(result.format, LogFormat::HttpLog);
        assert!(result.confidence > 0.8);
    }

    #[test]
    fn test_detect_no_match() {
        let detector = HttpLogDetector;
        let sample = b"Not an http log";
        let result = detector.detect(sample);
        assert_eq!(result.format, LogFormat::Unknown);
    }

    #[test]
    fn test_parse_common_log_format() {
        let parser = HttpLogParser;
        let sample = b"127.0.0.1 - frank [10/Oct/2000:13:55:36 -0700] \"GET /apache_pb.gif HTTP/1.0\" 200 2326";
        let parsed = parser.parse(sample).unwrap();
        assert_eq!(parsed.level, Some("info".to_string()));
        let req = parsed.request.unwrap();
        assert_eq!(req.method, Some("GET".to_string()));
        assert_eq!(req.path, Some("/apache_pb.gif".to_string()));
        assert_eq!(req.status_code, Some(200));
        assert_eq!(req.remote_addr, Some("127.0.0.1".to_string()));
        let user = parsed.fields.iter().find(|(k, _)| k == "user");
        assert_eq!(user.map(|(_, v)| v.as_str()), Some("frank"));
    }

    #[test]
    fn test_parse_combined_log_format() {
        let parser = HttpLogParser;
        let sample = b"127.0.0.1 - - [29/Jan/2026:10:59:12 +0000] \"POST /api/v1/data HTTP/1.1\" 200 1024 \"https://example.com\" \"curl/7.68.0\"";
        let parsed = parser.parse(sample).unwrap();
        let req = parsed.request.unwrap();
        assert_eq!(req.method, Some("POST".to_string()));
        assert_eq!(req.status_code, Some(200));
        let referrer = parsed.fields.iter().find(|(k, _)| k == "referrer");
        assert_eq!(referrer.map(|(_, v)| v.as_str()), Some("https://example.com"));
        let ua = parsed.fields.iter().find(|(k, _)| k == "user_agent");
        assert_eq!(ua.map(|(_, v)| v.as_str()), Some("curl/7.68.0"));
    }

    #[test]
    fn test_parse_error_status() {
        let parser = HttpLogParser;
        let sample = b"10.0.0.1 - - [01/Feb/2026:12:00:00 +0000] \"GET /missing HTTP/1.1\" 404 0";
        let parsed = parser.parse(sample).unwrap();
        assert_eq!(parsed.level, Some("warn".to_string()));
    }

    #[test]
    fn test_parse_server_error_status() {
        let parser = HttpLogParser;
        let sample = b"10.0.0.1 - - [01/Feb/2026:12:00:00 +0000] \"GET /crash HTTP/1.1\" 500 0";
        let parsed = parser.parse(sample).unwrap();
        assert_eq!(parsed.level, Some("error".to_string()));
    }

    #[test]
    fn test_parse_no_bracket() {
        let parser = HttpLogParser;
        let sample = b"Just some random text without brackets";
        assert!(parser.parse(sample).is_err());
    }
}
