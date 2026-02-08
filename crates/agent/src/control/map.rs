//! Map — pure error mapping helpers for control service.

use tonic::Status;
use crate::docker::client::DockerError;

/// Map DockerError to tonic Status
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
        _ => Status::internal(format!("Docker error: {}", err)),
    }
}
