use crate::parser::traits::{FormatDetector, LogFormat, DetectionResult};

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
}
