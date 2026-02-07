use super::traits::*;
use super::formats::*;

/// Format detector orchestrator with adaptive sampling
/// 1. Quick detection on first line (fast path)
/// 2. Multi-line sampling if confidence is low
/// 3. Adaptive refinement for uncertain cases
pub struct FormatDetectorOrchestrator {
    detectors: Vec<Box<dyn FormatDetector>>,
}

impl FormatDetectorOrchestrator {
    pub fn new() -> Self {
        let detectors: Vec<Box<dyn FormatDetector>> = vec![
            // Order matters! More specific detectors first
            Box::new(JsonDetector::new()),
            Box::new(LogfmtDetector),
            Box::new(SyslogDetector),
            Box::new(HttpLogDetector),
            Box::new(PlainTextDetector), // Fallback (always matches with low confidence)
        ];

        Self { detectors }
    }

    pub fn detect_single(&self, sample: &[u8]) -> DetectionResult {
        self.run_detectors(sample)
    }

    pub fn detect_multi(&self, samples: &[&[u8]]) -> DetectionResult {
        if samples.is_empty() {
            return DetectionResult::new(LogFormat::PlainText, 0.1);
        }

        let results: Vec<DetectionResult> = samples
            .iter()
            .map(|sample| self.run_detectors(sample))
            .collect();

        self.majority_vote(results)
    }

    pub fn detect_adaptive(&self, samples: &[&[u8]]) -> DetectionResult {
        if samples.is_empty() {
            return DetectionResult::new(LogFormat::PlainText, 0.1);
        }

        let initial_sample_size = samples.len().min(super::DETECTION_SAMPLE_SIZE);
        let initial_samples = &samples[..initial_sample_size];
        let initial_result = self.detect_multi(initial_samples);

        if initial_result.is_high_confidence() && initial_result.format != LogFormat::PlainText {
            return initial_result;
        }

    
        if samples.len() > initial_sample_size {
            let refinement_size = samples.len().min(super::ADAPTIVE_REFINEMENT_SIZE);
            let refinement_samples = &samples[..refinement_size];
            let refined_result = self.detect_multi(refinement_samples);

            if refined_result.format != LogFormat::PlainText && refined_result.format != LogFormat::Unknown {
                return refined_result;
            }
        
            if initial_result.format != LogFormat::PlainText && initial_result.format != LogFormat::Unknown {
                return initial_result;
            }

            if refined_result.confidence >= initial_result.confidence {
                return refined_result;
            }
        }

        initial_result
    }

    fn run_detectors(&self, sample: &[u8]) -> DetectionResult {
        let mut best_result = DetectionResult::no_match();

        for detector in &self.detectors {
            let result = detector.detect(sample);
            
            if result.confidence > best_result.confidence {
                best_result = result;
                if best_result.confidence >= 0.99 {
                    break;
                }
            }
        }

        best_result
    }

    fn majority_vote(&self, results: Vec<DetectionResult>) -> DetectionResult {
        use std::collections::HashMap;

        if results.is_empty() {
            return DetectionResult::new(LogFormat::PlainText, 0.1);
        }

        let total_results = results.len();
        
        let mut votes: HashMap<LogFormat, Vec<f32>> = HashMap::new();
        for result in results {
            votes.entry(result.format)
                .or_insert_with(Vec::new)
                .push(result.confidence);
        }

        let mut best_format = LogFormat::PlainText;
        let mut best_score = 0.0f32;


        let mut formats: Vec<_> = votes.keys().cloned().collect();
        formats.sort();

        for format in formats {
            if let Some(confidences) = votes.get(&format) {
                let avg_confidence: f32 = confidences.iter().sum::<f32>() / confidences.len() as f32;
                let vote_count = confidences.len();
                
                let score = (vote_count as f32 / total_results as f32) * avg_confidence;

                if score > best_score {
                    best_score = score;
                    best_format = format;
                }
            }
        }

        let avg_confidence = votes.get(&best_format)
            .map(|c| c.iter().sum::<f32>() / c.len() as f32)
            .unwrap_or(0.1);

        DetectionResult::new(best_format, avg_confidence)
    }
}

impl Default for FormatDetectorOrchestrator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_single_json() {
        let orchestrator = FormatDetectorOrchestrator::new();
        let sample = br#"{"level":"info","msg":"hello"}"#;
        
        let result = orchestrator.detect_single(sample);
        assert_eq!(result.format, LogFormat::Json);
        assert!(result.confidence > 0.7);
    }

    #[test]
    fn test_detect_single_logfmt() {
        let orchestrator = FormatDetectorOrchestrator::new();
        let sample = b"level=info msg=hello ts=2026-01-29";
        
        let result = orchestrator.detect_single(sample);
        assert_eq!(result.format, LogFormat::Logfmt);
        assert!(result.confidence > 0.7);
    }

    #[test]
    fn test_detect_single_plain() {
        let orchestrator = FormatDetectorOrchestrator::new();
        let sample = b"Just some plain text";
        
        let result = orchestrator.detect_single(sample);
        assert_eq!(result.format, LogFormat::PlainText);
    }

    #[test]
    fn test_detect_multi_json() {
        let orchestrator = FormatDetectorOrchestrator::new();
        let samples: Vec<&[u8]> = vec![
            br#"{"level":"info","msg":"line1"}"#,
            br#"{"level":"warn","msg":"line2"}"#,
            br#"{"level":"error","msg":"line3"}"#,
        ];
        
        let result = orchestrator.detect_multi(&samples);
        assert_eq!(result.format, LogFormat::Json);
        assert!(result.confidence > 0.7);
    }

    #[test]
    fn test_detect_multi_mixed() {
        let orchestrator = FormatDetectorOrchestrator::new();
        let samples: Vec<&[u8]> = vec![
            br#"{"level":"info","msg":"json"}"#,
            b"plain text line",
            br#"{"level":"warn","msg":"json again"}"#,
        ];
        
        let result = orchestrator.detect_multi(&samples);
        assert_eq!(result.format, LogFormat::Json);
    }

    #[test]
    fn test_adaptive_detection_high_confidence() {
        let orchestrator = FormatDetectorOrchestrator::new();
        let samples: Vec<&[u8]> = vec![
            br#"{"level":"info","msg":"line1","timestamp":1234567890,"logger":"app"}"#,
            br#"{"level":"info","msg":"line2","timestamp":1234567891,"logger":"app"}"#,
            br#"{"level":"info","msg":"line3","timestamp":1234567892,"logger":"app"}"#,
            br#"{"level":"info","msg":"line4","timestamp":1234567893,"logger":"app"}"#,
            br#"{"level":"info","msg":"line5","timestamp":1234567894,"logger":"app"}"#,
        ];
        
        let result = orchestrator.detect_adaptive(&samples);
        assert_eq!(result.format, LogFormat::Json);
        assert!(result.is_high_confidence());
    }

    #[test]
    fn test_adaptive_ignores_startup_banner() {
        let orchestrator = FormatDetectorOrchestrator::new();
        let samples: Vec<&[u8]> = vec![
            b"   _    _  __   __ ",  // ASCII Art
            b"  | |  | | \\ \\ / / ",
            b"  | |__| |  \\ V /  ",
            b"Starting application...", // Plain text
            br#"{"level":"info","msg":"System initialized"}"#, // Real log
            br#"{"level":"info","msg":"Listening on 8080"}"#,
        ];
        
        let result = orchestrator.detect_adaptive(&samples);
        assert_eq!(result.format, LogFormat::Json);
    }
    #[test]
    fn test_detect_orchestrator_syslog() {
        let orchestrator = FormatDetectorOrchestrator::new();
        let sample = b"<34>Oct 11 22:14:15 mymachine su: 'su root' failed for lonvick on /dev/pts/8";
        
        let result = orchestrator.detect_single(sample);
        assert_eq!(result.format, LogFormat::Syslog);
    }

    #[test]
    fn test_detect_orchestrator_httplog() {
        let orchestrator = FormatDetectorOrchestrator::new();
        let sample = b"127.0.0.1 - - [29/Jan/2026:10:59:12 +0000] \"GET /index.html HTTP/1.1\" 200 4096";
        
        let result = orchestrator.detect_single(sample);
        assert_eq!(result.format, LogFormat::HttpLog);
    }}
