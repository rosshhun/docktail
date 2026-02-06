use thiserror::Error;
use async_graphql::ErrorExtensions;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("Container not found: {0}")]
    #[allow(dead_code)]
    ContainerNotFound(String),

    #[error("Agent not found: {0}")]
    AgentNotFound(String),

    #[error("Agent unavailable: {0}")]
    AgentUnavailable(String),

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Authentication failed: {0}")]
    #[allow(dead_code)]
    Unauthorized(String),

    #[error("Forbidden: {0}")]
    #[allow(dead_code)]
    Forbidden(String),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("gRPC error: {0}")]
    Grpc(#[from] tonic::Status),

    #[error("Configuration error: {0}")]
    Config(#[from] anyhow::Error),
}

// Convenience type alias
#[allow(dead_code)]
pub type ApiResult<T> = Result<T, ApiError>;

// GraphQL integration: Add structured error codes to ApiError
impl ApiError {
    /// Convert ApiError to async_graphql::Error with structured error codes.
    /// Internal errors are sanitized to avoid leaking backend details.
    pub fn extend(self) -> async_graphql::Error {
        let (code, message) = match &self {
            ApiError::ContainerNotFound(_) => ("CONTAINER_NOT_FOUND", self.to_string()),
            ApiError::AgentNotFound(_) => ("AGENT_NOT_FOUND", self.to_string()),
            ApiError::AgentUnavailable(_) => ("AGENT_UNAVAILABLE", self.to_string()),
            ApiError::Unauthorized(_) => ("UNAUTHORIZED", self.to_string()),
            ApiError::Forbidden(_) => ("FORBIDDEN", self.to_string()),
            ApiError::InvalidRequest(_) => ("BAD_REQUEST", self.to_string()),
            ApiError::Internal(ref detail) => {
                // Log the full detail server-side but don't expose to client
                tracing::error!("Internal error: {}", detail);
                ("INTERNAL_SERVER_ERROR", "An internal error occurred".to_string())
            }
            ApiError::Grpc(ref status) => {
                // Log gRPC details server-side but sanitize for client
                tracing::error!("gRPC error: {}", status);
                ("GRPC_ERROR", "A backend communication error occurred".to_string())
            }
            ApiError::Config(ref err) => {
                tracing::error!("Config error: {}", err);
                ("CONFIG_ERROR", "A configuration error occurred".to_string())
            }
        };

        async_graphql::Error::new(message)
            .extend_with(|_err, e| e.set("code", code))
    }
}
