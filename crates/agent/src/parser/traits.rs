pub use super::model::{
    DetectionResult, LogFormat, ParsedLog, ParseError, RequestContext, ErrorContext
};

pub trait FormatDetector: Send + Sync {
    fn detect(&self, sample: &[u8]) -> DetectionResult;    
    fn format(&self) -> LogFormat;
}

pub trait LogParser: Send + Sync {
    /// parse a raw log line into structured data
    fn parse(&self, raw: &[u8]) -> Result<ParsedLog, ParseError>;    
    fn format(&self) -> LogFormat;
}
