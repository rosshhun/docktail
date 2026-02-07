use std::sync::atomic::{AtomicU64, AtomicI64, Ordering};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricErrorType {
    /// Parse operation exceeded time limit
    Timeout,
    /// Parser panicked (caught via catch_unwind)
    Panic,
    /// Line exceeded MAX_LINE_SIZE
    TooLarge,
    /// Non-UTF8 content encountered
    NonUtf8,
    /// Other/generic parse errors
    Other,
}

/// wrapper that forces the wrapped data onto its own cache line(s).
#[repr(align(64))]
#[derive(Debug, Default)]
pub struct CacheAligned<T>(pub T);

#[derive(Debug, Default)]
pub struct DetectionMetrics {
    pub attempts: AtomicU64,
    pub success: AtomicU64,
    pub fallback: AtomicU64,
}

#[derive(Debug, Default)]
pub struct FormatMetrics {
    pub json: AtomicU64,
    pub logfmt: AtomicU64,
    pub syslog: AtomicU64,
    pub http: AtomicU64,
    pub plain: AtomicU64,
}

#[derive(Debug, Default)]
pub struct TotalMetrics {
    pub time_nanos: AtomicU64,
    pub count: AtomicU64,
}

#[derive(Debug, Default)]
pub struct ErrorMetrics {
    pub generic: AtomicU64,
    pub timeout: AtomicU64,
    pub panic: AtomicU64,
    pub too_large: AtomicU64,
    pub non_utf8: AtomicU64,
}

#[derive(Debug, Default)]
pub struct GaugeMetrics {
    pub active_containers: AtomicI64,
    pub disabled_containers: AtomicI64,
}

#[derive(Debug, Default)]
pub struct SystemMetrics {
    pub docker_consecutive_failures: AtomicU64,
}

#[derive(Debug, Default)]
pub struct ParsingMetrics {
    pub detection: CacheAligned<DetectionMetrics>,
    
    pub formats: CacheAligned<FormatMetrics>,
    
    pub totals: CacheAligned<TotalMetrics>,
    
    pub errors: CacheAligned<ErrorMetrics>,
    
    pub gauges: CacheAligned<GaugeMetrics>,

    pub system: CacheAligned<SystemMetrics>,
}

impl ParsingMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub fn record_detection(&self, success: bool) {
        self.detection.0.attempts.fetch_add(1, Ordering::Relaxed);
        if success {
            self.detection.0.success.fetch_add(1, Ordering::Relaxed);
        } else {
            self.detection.0.fallback.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn record_parse(&self, format: super::LogFormat, time_nanos: u64) {
        use super::LogFormat;
        
        self.totals.0.count.fetch_add(1, Ordering::Relaxed);
        self.totals.0.time_nanos.fetch_add(time_nanos, Ordering::Relaxed);
        
        match format {
            LogFormat::Json => self.formats.0.json.fetch_add(1, Ordering::Relaxed),
            LogFormat::Logfmt => self.formats.0.logfmt.fetch_add(1, Ordering::Relaxed),
            LogFormat::Syslog => self.formats.0.syslog.fetch_add(1, Ordering::Relaxed),
            LogFormat::HttpLog => self.formats.0.http.fetch_add(1, Ordering::Relaxed),
            LogFormat::PlainText | LogFormat::Unknown => {
                self.formats.0.plain.fetch_add(1, Ordering::Relaxed)
            }
        };
    }

    #[inline]
    pub fn record_error(&self, error_type: MetricErrorType) {
        match error_type {
            MetricErrorType::Timeout => self.errors.0.timeout.fetch_add(1, Ordering::Relaxed),
            MetricErrorType::Panic => self.errors.0.panic.fetch_add(1, Ordering::Relaxed),
            MetricErrorType::TooLarge => self.errors.0.too_large.fetch_add(1, Ordering::Relaxed),
            MetricErrorType::NonUtf8 => self.errors.0.non_utf8.fetch_add(1, Ordering::Relaxed),
            MetricErrorType::Other => self.errors.0.generic.fetch_add(1, Ordering::Relaxed),
        };
    }


    #[inline]
    pub fn inc_active_containers(&self) {
        self.gauges.0.active_containers.fetch_add(1, Ordering::Relaxed);
    }
    
    #[inline]
    pub fn dec_active_containers(&self) {
        self.gauges.0.active_containers.fetch_sub(1, Ordering::Relaxed);
    }
    
    #[inline]
    pub fn inc_disabled_containers(&self) {
        self.gauges.0.disabled_containers.fetch_add(1, Ordering::Relaxed);
    }
    
    #[inline]
    pub fn dec_disabled_containers(&self) {
        self.gauges.0.disabled_containers.fetch_sub(1, Ordering::Relaxed);
    }

    
    #[inline]
    pub fn set_docker_failures(&self, count: u64) {
        self.system.0.docker_consecutive_failures.store(count, Ordering::Relaxed);
    }
    
    pub fn snapshot(&self) -> MetricsSnapshot {
        let total_parsed = self.totals.0.count.load(Ordering::Relaxed);
        let total_time_ns = self.totals.0.time_nanos.load(Ordering::Relaxed);
        
        let total_errors = self.errors.0.generic.load(Ordering::Relaxed)
            + self.errors.0.timeout.load(Ordering::Relaxed)
            + self.errors.0.panic.load(Ordering::Relaxed)
            + self.errors.0.too_large.load(Ordering::Relaxed)
            + self.errors.0.non_utf8.load(Ordering::Relaxed);
            
        let total_attempts = total_parsed + total_errors;
        
        MetricsSnapshot {
            // Detection
            detection_attempts: self.detection.0.attempts.load(Ordering::Relaxed),
            detection_success: self.detection.0.success.load(Ordering::Relaxed),
            detection_fallback: self.detection.0.fallback.load(Ordering::Relaxed),
            
            // Parsing by format
            json_parsed: self.formats.0.json.load(Ordering::Relaxed),
            logfmt_parsed: self.formats.0.logfmt.load(Ordering::Relaxed),
            syslog_parsed: self.formats.0.syslog.load(Ordering::Relaxed),
            http_parsed: self.formats.0.http.load(Ordering::Relaxed),
            plain_parsed: self.formats.0.plain.load(Ordering::Relaxed),
            
            // Totals
            total_parsed,
            avg_parse_time_us: if total_parsed > 0 {
                (total_time_ns as f64 / total_parsed as f64) / 1000.0
            } else {
                0.0
            },
            
            // Errors
            parse_errors: self.errors.0.generic.load(Ordering::Relaxed),
            parse_timeouts: self.errors.0.timeout.load(Ordering::Relaxed),
            parse_panics: self.errors.0.panic.load(Ordering::Relaxed),
            lines_too_large: self.errors.0.too_large.load(Ordering::Relaxed),
            non_utf8_content: self.errors.0.non_utf8.load(Ordering::Relaxed),
            success_rate: if total_attempts > 0 {
                total_parsed as f64 / total_attempts as f64
            } else {
                1.0
            },
            
            // Gauges
            active_containers: self.gauges.0.active_containers.load(Ordering::Relaxed),
            disabled_containers: self.gauges.0.disabled_containers.load(Ordering::Relaxed),

            // System
            docker_consecutive_failures: self.system.0.docker_consecutive_failures.load(Ordering::Relaxed),
        }
    }
}


#[derive(Debug, Clone, Serialize)]
pub struct MetricsSnapshot {
    // Detection
    pub detection_attempts: u64,
    pub detection_success: u64,
    pub detection_fallback: u64,
    
    // Format counts
    pub json_parsed: u64,
    pub logfmt_parsed: u64,
    pub syslog_parsed: u64,
    pub http_parsed: u64,
    pub plain_parsed: u64,
    
    // Performance
    pub total_parsed: u64,
    pub avg_parse_time_us: f64,
    
    // Errors
    pub parse_errors: u64,
    pub parse_timeouts: u64,
    pub parse_panics: u64,
    pub lines_too_large: u64,
    pub non_utf8_content: u64,
    pub success_rate: f64,
    
    // Gauges
    pub active_containers: i64,
    pub disabled_containers: i64,

    // System
    pub docker_consecutive_failures: u64,
}

impl MetricsSnapshot {
    
    pub fn to_metadata_map(&self) -> std::collections::HashMap<String, String> {
        let mut map = std::collections::HashMap::new();
        
        map.insert("total_parsed".to_string(), self.total_parsed.to_string());
        map.insert("success_rate".to_string(), format!("{:.2}", self.success_rate));
        map.insert("avg_parse_time_us".to_string(), format!("{:.2}", self.avg_parse_time_us));
        
        map.insert("parse_errors".to_string(), self.parse_errors.to_string());
        map.insert("parse_timeouts".to_string(), self.parse_timeouts.to_string());
        map.insert("parse_panics".to_string(), self.parse_panics.to_string());
        
        map.insert("docker_failures".to_string(), self.docker_consecutive_failures.to_string());

        map.insert("json_parsed".to_string(), self.json_parsed.to_string());
        map.insert("logfmt_parsed".to_string(), self.logfmt_parsed.to_string());
        map.insert("plain_parsed".to_string(), self.plain_parsed.to_string());
        
        map.insert("active_containers".to_string(), self.active_containers.to_string());
        map.insert("disabled_containers".to_string(), self.disabled_containers.to_string());
        
        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_metrics_are_empty() {
        let metrics = ParsingMetrics::new();
        let snap = metrics.snapshot();
        
        assert_eq!(snap.total_parsed, 0);
        assert_eq!(snap.parse_errors, 0);
        assert_eq!(snap.avg_parse_time_us, 0.0);
        assert_eq!(snap.success_rate, 1.0);
    }

    #[test]
    fn test_record_detection() {
        let metrics = ParsingMetrics::new();
        metrics.record_detection(true);
        metrics.record_detection(false);
        metrics.record_detection(true);
        
        let snap = metrics.snapshot();
        assert_eq!(snap.detection_attempts, 3);
        assert_eq!(snap.detection_success, 2);
        assert_eq!(snap.detection_fallback, 1);
    }

    #[test]
    fn test_record_parse_counts_and_times() {
        let metrics = ParsingMetrics::new();
        
        metrics.record_parse(crate::parser::LogFormat::Json, 1000);
        metrics.record_parse(crate::parser::LogFormat::Logfmt, 2000);
        
        let snap = metrics.snapshot();
        assert_eq!(snap.total_parsed, 2);
        assert_eq!(snap.json_parsed, 1);
        assert_eq!(snap.logfmt_parsed, 1);
        
        assert!((snap.avg_parse_time_us - 1.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_error_recording_and_success_rate() {
        let metrics = ParsingMetrics::new();
        
        metrics.record_parse(crate::parser::LogFormat::Json, 100);
        metrics.record_parse(crate::parser::LogFormat::Json, 100);
        
        metrics.record_error(MetricErrorType::Timeout);
        metrics.record_error(MetricErrorType::NonUtf8);
        
        let snap = metrics.snapshot();
        
        assert_eq!(snap.total_parsed, 2);
        assert_eq!(snap.parse_timeouts, 1);
        assert_eq!(snap.non_utf8_content, 1);
        
        assert_eq!(snap.success_rate, 0.5);
    }

    #[test]
    fn test_gauges() {
        let metrics = ParsingMetrics::new();
        
        metrics.inc_active_containers();
        metrics.inc_active_containers();
        metrics.dec_active_containers();
        
        metrics.inc_disabled_containers();
        
        let snap = metrics.snapshot();
        assert_eq!(snap.active_containers, 1);
        assert_eq!(snap.disabled_containers, 1);
    }

    #[test]
    fn test_formatting_grouping() {
        let metrics = ParsingMetrics::new();
        
        metrics.record_parse(crate::parser::LogFormat::Json, 100);
        metrics.record_parse(crate::parser::LogFormat::Syslog, 100);
        metrics.record_parse(crate::parser::LogFormat::HttpLog, 100);
        metrics.record_parse(crate::parser::LogFormat::PlainText, 100);
        metrics.record_parse(crate::parser::LogFormat::Unknown, 100);
        
        let snap = metrics.snapshot();
        assert_eq!(snap.json_parsed, 1);
        assert_eq!(snap.syslog_parsed, 1);
        assert_eq!(snap.http_parsed, 1);
        assert_eq!(snap.plain_parsed, 2); // PlainText + Unknown
    }
}