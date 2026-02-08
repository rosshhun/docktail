//! Conf module — configuration model, loading, TLS, and multiline grouping config.

pub mod model;
pub mod load;
pub mod tls;
pub mod group;

pub use model::{AgentConfig, MultilineConfig, ContainerMultilineConfig};
