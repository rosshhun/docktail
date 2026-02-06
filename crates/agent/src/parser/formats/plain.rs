use crate::parser::traits::*;
use crate::parser::MAX_LINE_SIZE;
use bytes::Bytes;

/// Plain text detector (fallback - always matches with low confidence)
pub struct PlainTextDetector;

impl FormatDetector for PlainTextDetector {
    fn detect(&self, _sample: &[u8]) -> DetectionResult {
        // Plain text is the safe fallback
        // Always matches with low confidence so other detectors take precedence
        DetectionResult::new(LogFormat::PlainText, 0.1)
    }

    fn format(&self) -> LogFormat {
        LogFormat::PlainText
    }
}

/// Plain text parser (pass-through with minimal processing)
pub struct PlainTextParser;

impl LogParser for PlainTextParser {
    fn parse(&self, raw: &[u8]) -> Result<ParsedLog, ParseError> {
        // SECURITY: Enforce size limit to prevent DoS via massive log lines
        if raw.len() > MAX_LINE_SIZE {
            return Err(ParseError::LineTooLarge(raw.len(), MAX_LINE_SIZE));
        }

        // Plain text parsing is just wrapping in ParsedLog
        // Try to extract a message if it's valid UTF-8
        let message = std::str::from_utf8(raw)
            .ok()
            .map(|s| s.trim_end().to_string())
            .filter(|s| !s.is_empty());

        Ok(ParsedLog {
            level: None,
            message,
            logger: None,
            timestamp: None,
            request: None,
            error: None,
            fields: Vec::new(),
            raw_content: Bytes::copy_from_slice(raw),
        })
    }

    fn format(&self) -> LogFormat {
        LogFormat::PlainText
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plain_text_detector() {
        let detector = PlainTextDetector;
        
        let samples: Vec<&[u8]> = vec![
            b"Just some plain text",
            b"No structure here",
            b"Could be anything",
        ];

        for sample in samples {
            let result = detector.detect(sample);
            assert_eq!(result.format, LogFormat::PlainText);
            assert!(result.confidence < 0.5); // Low confidence
        }
    }

    #[test]
    fn test_plain_text_parser() {
        let parser = PlainTextParser;
        
        let sample = b"This is a plain text log line";
        let parsed = parser.parse(sample).unwrap();
        
        assert_eq!(parsed.message, Some("This is a plain text log line".to_string()));
        assert_eq!(parsed.level, None);
        assert_eq!(parsed.fields.len(), 0);
    }

    #[test]
    fn test_plain_text_parser_non_utf8() {
        let parser = PlainTextParser;
        
        let binary = b"\xFF\xFE\x00\x01";
        let parsed = parser.parse(binary).unwrap();
        
        // Should still succeed, just no message extracted
        assert_eq!(parsed.message, None);
        assert_eq!(parsed.raw_content.as_ref(), binary);
    }

    #[test]
    fn test_plain_text_parser_size_limit() {
        let parser = PlainTextParser;
        
        // Create a log line that exceeds MAX_LINE_SIZE
        let oversized = vec![b'X'; MAX_LINE_SIZE + 1];
        let result = parser.parse(&oversized);
        
        // Should fail with LineTooLarge error
        assert!(matches!(result, Err(ParseError::LineTooLarge(_, _))));
        
        // Just under the limit should succeed
        let just_under = vec![b'Y'; MAX_LINE_SIZE];
        let parsed = parser.parse(&just_under).unwrap();
        assert!(parsed.message.is_some());
    }
}
