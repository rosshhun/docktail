//! Logs module — log streaming, detection, grouping, and mapping.

pub mod detect;
pub mod map;
pub mod pattern;
pub mod group;
pub mod route;

pub use route::LogServiceImpl;
