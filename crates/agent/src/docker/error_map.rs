//! Shared Docker error → gRPC status mapping.
//!
//! Single source of truth for converting [`DockerError`] into [`tonic::Status`].
//! Used by control, shell, stats, and swarm services.

use tonic::Status;
use super::client::DockerError;

/// Map a [`DockerError`] to the appropriate [`tonic::Status`].
///
/// Mapping rules:
/// - `ContainerNotFound` → `NOT_FOUND`
/// - `PermissionDenied` → `PERMISSION_DENIED`
/// - `ConnectionFailed` → `UNAVAILABLE`
/// - `NotSwarmManager` → `FAILED_PRECONDITION`
/// - Everything else → `INTERNAL`
pub fn map_docker_error(err: DockerError) -> Status {
    match &err {
        DockerError::ContainerNotFound(id) => {
            Status::not_found(format!("Container not found: {}", id))
        }
        DockerError::PermissionDenied => {
            Status::permission_denied("Permission denied")
        }
        DockerError::ConnectionFailed(msg) => {
            Status::unavailable(format!("Docker daemon unavailable: {}", msg))
        }
        DockerError::NotSwarmManager => {
            Status::failed_precondition(format!("{}", err))
        }
        _ => Status::internal(format!("Docker error: {}", err)),
    }
}

/// Convenience: map a `DockerError` into a `tonic::Status` with an extra
/// contextual prefix (e.g. the container ID or operation being performed).
pub fn map_docker_error_with_context(context: &str, err: DockerError) -> Status {
    match &err {
        DockerError::ContainerNotFound(_) => {
            Status::not_found(format!("{}: {}", context, err))
        }
        DockerError::PermissionDenied => {
            Status::permission_denied(format!("{}: {}", context, err))
        }
        DockerError::ConnectionFailed(_) => {
            Status::unavailable(format!("{}: {}", context, err))
        }
        DockerError::NotSwarmManager => {
            Status::failed_precondition(format!("{}: {}", context, err))
        }
        _ => Status::internal(format!("{}: {}", context, err)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_container_not_found() {
        let err = DockerError::ContainerNotFound("abc123".to_string());
        let status = map_docker_error(err);
        assert_eq!(status.code(), tonic::Code::NotFound);
        assert!(status.message().contains("abc123"));
    }

    #[test]
    fn test_map_permission_denied() {
        let err = DockerError::PermissionDenied;
        let status = map_docker_error(err);
        assert_eq!(status.code(), tonic::Code::PermissionDenied);
    }

    #[test]
    fn test_map_connection_failed() {
        let err = DockerError::ConnectionFailed("socket gone".to_string());
        let status = map_docker_error(err);
        assert_eq!(status.code(), tonic::Code::Unavailable);
        assert!(status.message().contains("socket gone"));
    }

    #[test]
    fn test_map_stream_closed() {
        let err = DockerError::StreamClosed;
        let status = map_docker_error(err);
        assert_eq!(status.code(), tonic::Code::Internal);
    }

    #[test]
    fn test_map_unsupported_log_driver() {
        let err = DockerError::UnsupportedLogDriver("syslog".to_string());
        let status = map_docker_error(err);
        assert_eq!(status.code(), tonic::Code::Internal);
        assert!(status.message().contains("syslog"));
    }

    #[test]
    fn test_map_not_swarm_manager() {
        let err = DockerError::NotSwarmManager;
        let status = map_docker_error(err);
        assert_eq!(status.code(), tonic::Code::FailedPrecondition);
        assert!(status.message().contains("swarm manager"));
    }

    #[test]
    fn test_map_with_context() {
        let err = DockerError::ContainerNotFound("xyz".to_string());
        let status = map_docker_error_with_context("inspecting container", err);
        assert_eq!(status.code(), tonic::Code::NotFound);
        assert!(status.message().contains("inspecting container"));
        assert!(status.message().contains("xyz"));
    }
}
