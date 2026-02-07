pub mod traits;
pub mod detector;
pub mod cache;
pub mod metrics;
pub mod formats;
pub mod model;
mod ansi;
mod serde_utils;

pub use traits::LogParser;
pub use model::LogFormat;
pub use ansi::strip_ansi_codes;

pub const MAX_LINE_SIZE: usize = 1_048_576; // 1MB
pub const DETECTION_SAMPLE_SIZE: usize = 5; // Lines to sample for detection
pub const HIGH_CONFIDENCE_THRESHOLD: f32 = 0.95;
pub const MEDIUM_CONFIDENCE_THRESHOLD: f32 = 0.70;
pub const ADAPTIVE_REFINEMENT_SIZE: usize = 7;
