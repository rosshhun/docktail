use crate::parser::traits::{FormatDetector, LogFormat, LogParser, DetectionResult, ParsedLog, ParseError};
use bytes::Bytes;

pub struct SyslogDetector;

impl FormatDetector for SyslogDetector {
    fn detect(&self, sample: &[u8]) -> DetectionResult {
        // Check for Syslog priority prefix <PRI>
        // Example: <165>1 2026-02-04T... (RFC 5424) or <34>Oct 11... (RFC 3164)
        
        if sample.iter().next() != Some(&b'<') {
            return DetectionResult::no_match();
        }

        let max_pri_len = 5;
        let p_end = sample.iter().take(max_pri_len + 1).position(|&c| c == b'>');

        if let Some(idx) = p_end {
            let pri_slice = &sample[1..idx];
            
            if !pri_slice.is_empty() && pri_slice.iter().all(|c| c.is_ascii_digit()) {
                return DetectionResult::new(LogFormat::Syslog, 0.85);
            }
        }

        DetectionResult::no_match()
    }

    fn format(&self) -> LogFormat {
        LogFormat::Syslog
    }
}

/// Parser for syslog messages (RFC 3164 and RFC 5424).
///
/// Extracts priority, severity, facility, hostname, app-name, and message
/// from syslog-formatted log lines.
pub struct SyslogParser;

/// Syslog severity levels (RFC 5424 §6.2.1)
const SYSLOG_SEVERITIES: [&str; 8] = [
    "emergency", "alert", "critical", "error",
    "warning", "notice", "info", "debug",
];

/// Syslog facility names (RFC 5424 §6.2.1)
const SYSLOG_FACILITIES: [&str; 24] = [
    "kern", "user", "mail", "daemon", "auth", "syslog", "lpr", "news",
    "uucp", "cron", "authpriv", "ftp", "ntp", "audit", "alert2", "clock",
    "local0", "local1", "local2", "local3", "local4", "local5", "local6", "local7",
];

impl LogParser for SyslogParser {
    fn parse(&self, raw: &[u8]) -> Result<ParsedLog, ParseError> {
        let text = std::str::from_utf8(raw)
            .map_err(|_| ParseError::NonUtf8)?;

        // Must start with <PRI>
        if !text.starts_with('<') {
            return Err(ParseError::InvalidFormat("Missing syslog priority".into()));
        }

        let pri_end = text.find('>')
            .ok_or_else(|| ParseError::InvalidFormat("Unterminated priority".into()))?;

        let pri_val: u32 = text[1..pri_end].parse()
            .map_err(|_| ParseError::InvalidFormat("Invalid priority value".into()))?;

        let facility_num = (pri_val >> 3) as usize;
        let severity_num = (pri_val & 0x07) as usize;

        let severity = SYSLOG_SEVERITIES.get(severity_num).map(|s| s.to_string());
        let facility = SYSLOG_FACILITIES.get(facility_num).map(|s| s.to_string());

        let remainder = &text[pri_end + 1..];

        // Try RFC 5424 first: version SP timestamp SP hostname SP app-name SP procid SP msgid
        let (hostname, app_name, message, timestamp) = if remainder.starts_with('1') && remainder.len() > 2 && remainder.as_bytes()[1] == b' ' {
            parse_rfc5424(&remainder[2..])
        } else {
            parse_rfc3164(remainder)
        };

        let mut fields = Vec::new();
        if let Some(ref fac) = facility {
            fields.push(("facility".to_string(), fac.clone()));
        }
        fields.push(("priority".to_string(), pri_val.to_string()));

        Ok(ParsedLog {
            level: severity,
            message,
            logger: app_name.or(hostname.clone()),
            timestamp,
            request: None,
            error: None,
            fields,
            raw_content: Bytes::copy_from_slice(raw),
        })
    }

    fn format(&self) -> LogFormat {
        LogFormat::Syslog
    }
}

/// Parse RFC 5424 remainder after "<PRI>1 "
fn parse_rfc5424(text: &str) -> (Option<String>, Option<String>, Option<String>, Option<chrono::DateTime<chrono::Utc>>) {
    let parts: Vec<&str> = text.splitn(7, ' ').collect();
    // parts: [timestamp, hostname, app-name, procid, msgid, structured-data..., msg...]

    let timestamp = parts.first()
        .filter(|s| **s != "-")
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc));

    let hostname = parts.get(1)
        .filter(|s| **s != "-")
        .map(|s| s.to_string());

    let app_name = parts.get(2)
        .filter(|s| **s != "-")
        .map(|s| s.to_string());

    // Message is everything after structured data (skip procid, msgid, SD)
    let message = if parts.len() >= 6 {
        // Find the message part (after structured data)
        let sd_and_msg = parts[5..].join(" ");
        // Skip structured data blocks [...]
        let msg = if sd_and_msg.starts_with('[') {
            // Find end of structured data
            sd_and_msg.rfind(']')
                .map(|idx| sd_and_msg[idx + 1..].trim().to_string())
                .filter(|s| !s.is_empty())
                .or_else(|| Some(sd_and_msg))
        } else if sd_and_msg.starts_with('-') {
            Some(sd_and_msg[1..].trim().to_string())
        } else {
            Some(sd_and_msg)
        };
        msg.filter(|s| !s.is_empty())
    } else if parts.len() > 5 {
        Some(parts[5..].join(" "))
    } else {
        None
    };

    (hostname, app_name, message, timestamp)
}

/// Parse RFC 3164 remainder after "<PRI>"
fn parse_rfc3164(text: &str) -> (Option<String>, Option<String>, Option<String>, Option<chrono::DateTime<chrono::Utc>>) {
    // RFC 3164 format: "Oct 11 22:14:15 hostname app[pid]: message"
    // The timestamp is "Mon DD HH:MM:SS" (always 15 chars)

    let timestamp = None; // RFC 3164 timestamps lack year/timezone, unreliable to parse

    // Try to split: "Oct 11 22:14:15 hostname app: message"
    // Skip the timestamp part (first 15+ chars)
    let after_ts = if text.len() > 16 {
        let space_count = text.chars().take(20).filter(|&c| c == ' ').count();
        if space_count >= 2 {
            // Find the third space (after "Mon DD HH:MM:SS")
            let mut spaces = 0;
            let mut idx = 0;
            for (i, c) in text.char_indices() {
                if c == ' ' {
                    spaces += 1;
                    if spaces == 3 {
                        idx = i + 1;
                        break;
                    }
                }
            }
            if idx > 0 { &text[idx..] } else { text }
        } else {
            text
        }
    } else {
        text
    };

    // Split "hostname app[pid]: message" or "hostname app: message"
    let parts: Vec<&str> = after_ts.splitn(3, ' ').collect();

    let hostname = parts.first().map(|s| s.to_string());

    let (app_name, message) = if parts.len() >= 2 {
        let tag = parts[1];
        // Tag might be "app:" or "app[pid]:"
        let app = tag.split('[').next()
            .unwrap_or(tag)
            .trim_end_matches(':')
            .to_string();
        let msg = if parts.len() >= 3 {
            Some(parts[2..].join(" ").trim_start_matches(": ").to_string())
        } else {
            None
        };
        (Some(app), msg)
    } else {
        (None, Some(after_ts.to_string()))
    };

    (hostname, app_name, message, timestamp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_rfc3164() {
        let detector = SyslogDetector;
        let sample = b"<34>Oct 11 22:14:15 mymachine su: 'su root' failed for lonvick on /dev/pts/8";
        let result = detector.detect(sample);
        assert_eq!(result.format, LogFormat::Syslog);
        assert!(result.confidence > 0.8);
    }

    #[test]
    fn test_detect_rfc5424() {
        let detector = SyslogDetector;
        let sample = b"<165>1 2003-10-11T22:14:15.003Z mymachine.example.com evntslog - ID47 [exampleSDID@32473 iut=\"3\" eventSource=\"Application\" eventID=\"1011\"] BOMAn application event log entry...";
        let result = detector.detect(sample);
        assert_eq!(result.format, LogFormat::Syslog);
        assert!(result.confidence > 0.8);
    }

    #[test]
    fn test_detect_no_match() {
        let detector = SyslogDetector;
        let sample = b"Not a syslog message";
        let result = detector.detect(sample);
        assert_eq!(result.format, LogFormat::Unknown);
    }

    #[test]
    fn test_parse_rfc3164() {
        let parser = SyslogParser;
        let sample = b"<34>Oct 11 22:14:15 mymachine su: 'su root' failed for lonvick on /dev/pts/8";
        let parsed = parser.parse(sample).unwrap();
        // pri=34 => facility=4 (auth), severity=2 (critical)
        assert_eq!(parsed.level, Some("critical".to_string()));
        assert!(parsed.message.is_some());
        let msg = parsed.message.unwrap();
        assert!(msg.contains("su root"), "Expected message to contain 'su root', got: {}", msg);
    }

    #[test]
    fn test_parse_rfc5424() {
        let parser = SyslogParser;
        let sample = b"<165>1 2003-10-11T22:14:15.003Z mymachine.example.com evntslog - ID47 [exampleSDID@32473] BOMAn application event log entry";
        let parsed = parser.parse(sample).unwrap();
        // pri=165 => facility=20 (local4), severity=5 (notice)
        assert_eq!(parsed.level, Some("notice".to_string()));
        assert!(parsed.timestamp.is_some());
        assert_eq!(parsed.logger, Some("evntslog".to_string()));
    }

    #[test]
    fn test_parse_extracts_facility() {
        let parser = SyslogParser;
        let sample = b"<34>Oct 11 22:14:15 host app: test";
        let parsed = parser.parse(sample).unwrap();
        let facility = parsed.fields.iter().find(|(k, _)| k == "facility");
        assert!(facility.is_some());
        assert_eq!(facility.unwrap().1, "auth");
    }

    #[test]
    fn test_parse_invalid_no_priority() {
        let parser = SyslogParser;
        let sample = b"No priority here";
        assert!(parser.parse(sample).is_err());
    }

    #[test]
    fn test_parse_invalid_bad_priority() {
        let parser = SyslogParser;
        let sample = b"<abc>bad priority";
        assert!(parser.parse(sample).is_err());
    }
}
