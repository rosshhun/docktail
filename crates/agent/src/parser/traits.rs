pub use super::model::{
    DetectionResult, LogFormat, ParsedLog, ParseError, RequestContext, ErrorContext
};

/// Trait for format detectors
pub trait FormatDetector: Send + Sync {
    /// Detect format from a sample line
    fn detect(&self, sample: &[u8]) -> DetectionResult;
    
    /// The format this detector can detect
    fn format(&self) -> LogFormat;
}

/// Trait for log parsers
pub trait LogParser: Send + Sync {
    /// Parse a raw log line into structured data
    fn parse(&self, raw: &[u8]) -> Result<ParsedLog, ParseError>;
    
    /// The format this parser handles
    fn format(&self) -> LogFormat;
}
