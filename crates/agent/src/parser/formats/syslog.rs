use crate::parser::traits::{FormatDetector, LogFormat, DetectionResult};

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
}
