//! Config — swarm config listing, creation, and deletion.

use std::collections::HashMap;
use tonic::Status;
use tracing::info;

use crate::docker::client::DockerClient;
use crate::proto::{
    SwarmConfigInfo,
    CreateConfigResponse, DeleteConfigResponse,
};

/// List all swarm configs (metadata only).
pub(crate) async fn list(docker: &DockerClient) -> Result<Vec<SwarmConfigInfo>, Status> {
    let configs = docker.list_configs().await
        .map_err(|e| {
            if matches!(e, crate::docker::client::DockerError::NotSwarmManager) {
                Status::permission_denied(format!("{}", e))
            } else {
                Status::internal(format!("Failed to list configs: {}", e))
            }
        })?;

    let config_infos: Vec<SwarmConfigInfo> = configs.iter().map(|c| {
        let spec = c.spec.as_ref();
        let created_at = c.created_at.as_ref()
            .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
            .map(|dt| dt.timestamp())
            .unwrap_or(0);
        let updated_at = c.updated_at.as_ref()
            .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
            .map(|dt| dt.timestamp())
            .unwrap_or(0);

        SwarmConfigInfo {
            id: c.id.clone().unwrap_or_default(),
            name: spec.and_then(|sp| sp.name.clone()).unwrap_or_default(),
            created_at,
            updated_at,
            labels: spec.and_then(|sp| sp.labels.clone()).unwrap_or_default(),
        }
    }).collect();

    info!("Listed {} swarm configs", config_infos.len());
    Ok(config_infos)
}

/// Create a new swarm config.
pub(crate) async fn create(
    docker: &DockerClient,
    name: &str,
    data: &[u8],
    labels: HashMap<String, String>,
) -> Result<CreateConfigResponse, Status> {
    match docker.create_config(name, data, labels).await {
        Ok(config_id) => {
            info!(name = %name, config_id = %config_id, "Config created");
            Ok(CreateConfigResponse {
                success: true,
                message: format!("Config {} created", name),
                config_id,
            })
        }
        Err(e) => Err(Status::internal(format!("Failed to create config: {}", e))),
    }
}

/// Delete a swarm config by ID.
pub(crate) async fn delete(docker: &DockerClient, config_id: &str) -> Result<DeleteConfigResponse, Status> {
    match docker.delete_config(config_id).await {
        Ok(()) => Ok(DeleteConfigResponse {
            success: true,
            message: format!("Config {} deleted", config_id),
        }),
        Err(e) => Err(Status::internal(format!("Failed to delete config: {}", e))),
    }
}
