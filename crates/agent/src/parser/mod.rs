/// Log parsing and normalization module
/// 
/// This module provides production-grade log format detection and parsing
/// to convert raw log bytes into structured, normalized log entries.
/// 
/// # Architecture
/// 
/// - `traits.rs`: Core traits for detectors and parsers
/// - `detector.rs`: Format detection orchestrator with adaptive sampling
/// - `formats/`: Individual format parser implementations
/// - `cache.rs`: Per-container parser caching
/// - `metrics.rs`: Parsing performance metrics
/// 
/// # Safety Guarantees
/// 
/// All parsers implement:
/// - Bounded memory (no unbounded allocations)
/// - Panic safety (catch_unwind wrapper)
/// - Binary safety (handle non-UTF8 gracefully)
/// - Line size limits (prevent DoS)

pub mod traits;
pub mod detector;
pub mod cache;
pub mod metrics;
pub mod formats;
pub mod model;
mod ansi;
mod serde_utils;

// Re-export commonly used types
pub use traits::LogParser;
pub use model::LogFormat;
pub use ansi::strip_ansi_codes;

// Constants
pub const MAX_LINE_SIZE: usize = 1_048_576; // 1MB
pub const DETECTION_SAMPLE_SIZE: usize = 5; // Lines to sample for detection
pub const HIGH_CONFIDENCE_THRESHOLD: f32 = 0.95;
pub const MEDIUM_CONFIDENCE_THRESHOLD: f32 = 0.70;
pub const ADAPTIVE_REFINEMENT_SIZE: usize = 7;
