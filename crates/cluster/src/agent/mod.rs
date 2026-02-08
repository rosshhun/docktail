pub mod client;
pub mod pool;
pub mod registry;
pub mod discovery;

pub use client::AgentGrpcClient;
pub use pool::{AgentConnection, AgentPool, AgentSource, HealthStatus, SwarmRole};
pub use registry::AgentRegistry;

use thiserror::Error;

/// Standard Result type for the Agent module
pub type Result<T> = std::result::Result<T, AgentError>;

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("gRPC transport error: {0}")]
    Transport(#[from] tonic::transport::Error),

    #[error("gRPC status error: {0}")]
    Status(#[from] tonic::Status),

    #[error("Agent not found: {0}")]
    #[allow(dead_code)]
    NotFound(String),

    #[error("Agent unhealthy: {0}")]
    #[allow(dead_code)]
    Unhealthy(String),

    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("TLS configuration error: {0}")]
    Tls(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
}
