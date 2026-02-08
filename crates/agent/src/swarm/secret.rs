//! Secret — swarm secret listing, creation, and deletion.

use std::collections::HashMap;
use tonic::Status;
use tracing::info;

use crate::docker::client::DockerClient;
use crate::proto::{
    SwarmSecretInfo,
    CreateSecretResponse, DeleteSecretResponse,
};

/// List all swarm secrets (metadata only — data is never returned).
pub(crate) async fn list(docker: &DockerClient) -> Result<Vec<SwarmSecretInfo>, Status> {
    let secrets = docker.list_secrets().await
        .map_err(|e| {
            if matches!(e, crate::docker::client::DockerError::NotSwarmManager) {
                Status::permission_denied(format!("{}", e))
            } else {
                Status::internal(format!("Failed to list secrets: {}", e))
            }
        })?;

    let secret_infos: Vec<SwarmSecretInfo> = secrets.iter().map(|s| {
        let spec = s.spec.as_ref();
        let created_at = s.created_at.as_ref()
            .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
            .map(|dt| dt.timestamp())
            .unwrap_or(0);
        let updated_at = s.updated_at.as_ref()
            .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
            .map(|dt| dt.timestamp())
            .unwrap_or(0);

        SwarmSecretInfo {
            id: s.id.clone().unwrap_or_default(),
            name: spec.and_then(|sp| sp.name.clone()).unwrap_or_default(),
            created_at,
            updated_at,
            labels: spec.and_then(|sp| sp.labels.clone()).unwrap_or_default(),
            driver: spec.and_then(|sp| sp.driver.as_ref())
                .map(|d| d.name.clone())
                .unwrap_or_default(),
        }
    }).collect();

    info!("Listed {} swarm secrets", secret_infos.len());
    Ok(secret_infos)
}

/// Create a new swarm secret.
pub(crate) async fn create(
    docker: &DockerClient,
    name: &str,
    data: &[u8],
    labels: HashMap<String, String>,
) -> Result<CreateSecretResponse, Status> {
    match docker.create_secret(name, data, labels).await {
        Ok(secret_id) => {
            info!(name = %name, secret_id = %secret_id, "Secret created");
            Ok(CreateSecretResponse {
                success: true,
                message: format!("Secret {} created", name),
                secret_id,
            })
        }
        Err(e) => Err(Status::internal(format!("Failed to create secret: {}", e))),
    }
}

/// Delete a swarm secret by ID.
pub(crate) async fn delete(docker: &DockerClient, secret_id: &str) -> Result<DeleteSecretResponse, Status> {
    match docker.delete_secret(secret_id).await {
        Ok(()) => Ok(DeleteSecretResponse {
            success: true,
            message: format!("Secret {} deleted", secret_id),
        }),
        Err(e) => Err(Status::internal(format!("Failed to delete secret: {}", e))),
    }
}
