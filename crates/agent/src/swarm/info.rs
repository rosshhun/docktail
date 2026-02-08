//! Info — swarm info, inspect, init, join, leave, unlock operations.

use tonic::Status;
use tracing::{debug, info, warn};

use crate::docker::client::{DockerClient, SwarmInspectResult};
use crate::proto::{
    SwarmInfo, SwarmInfoResponse,
    SwarmInitResponse, SwarmJoinResponse, SwarmLeaveResponse,
    SwarmUnlockKeyResponse, SwarmUnlockResponse,
};

/// Fetches swarm membership and identity information.
///
/// Returns `SwarmInfoResponse` with `is_swarm_mode`, `swarm` info, etc.
pub(crate) async fn get_info(docker: &DockerClient) -> Result<SwarmInfoResponse, String> {
    match docker.swarm_inspect().await {
        Ok(SwarmInspectResult::Manager(swarm)) => {
            let swarm_id = swarm.id.unwrap_or_default();

            let created_at = swarm.created_at.as_ref()
                .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
                .map(|dt| dt.timestamp())
                .unwrap_or(0);
            let updated_at = swarm.updated_at.as_ref()
                .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
                .map(|dt| dt.timestamp())
                .unwrap_or(0);

            let nodes = docker.list_nodes().await.unwrap_or_default();

            let managers = nodes.iter().filter(|n| {
                n.spec.as_ref()
                    .and_then(|s| s.role.as_ref())
                    .map(|r| matches!(r, bollard::models::NodeSpecRoleEnum::MANAGER))
                    .unwrap_or(false)
            }).count() as u32;

            let workers = nodes.iter().filter(|n| {
                n.spec.as_ref()
                    .and_then(|s| s.role.as_ref())
                    .map(|r| matches!(r, bollard::models::NodeSpecRoleEnum::WORKER))
                    .unwrap_or(false)
            }).count() as u32;

            let sys_info = docker.system_info().await.ok();
            let (node_id, node_addr) = sys_info
                .as_ref()
                .and_then(|info| info.swarm.as_ref())
                .map(|swarm_info| {
                    (
                        swarm_info.node_id.clone().unwrap_or_default(),
                        swarm_info.node_addr.clone().unwrap_or_default(),
                    )
                })
                .unwrap_or_default();

            let info = SwarmInfo {
                swarm_id,
                node_id,
                node_addr,
                is_manager: true,
                managers,
                workers,
                created_at,
                updated_at,
            };

            Ok(SwarmInfoResponse {
                is_swarm_mode: true,
                swarm: Some(info),
            })
        }
        Ok(SwarmInspectResult::Worker) => {
            debug!("Node is a swarm worker (not a manager)");
            let sys_info = docker.system_info().await.ok();
            let swarm_info_field = sys_info.as_ref().and_then(|info| info.swarm.as_ref());

            let (node_id, node_addr) = swarm_info_field
                .map(|si| (
                    si.node_id.clone().unwrap_or_default(),
                    si.node_addr.clone().unwrap_or_default(),
                ))
                .unwrap_or_default();

            let info = SwarmInfo {
                swarm_id: String::new(),
                node_id,
                node_addr,
                is_manager: false,
                managers: 0,
                workers: 0,
                created_at: 0,
                updated_at: 0,
            };

            Ok(SwarmInfoResponse {
                is_swarm_mode: true,
                swarm: Some(info),
            })
        }
        Ok(SwarmInspectResult::NotInSwarm) => {
            debug!("Not in swarm mode");
            Ok(SwarmInfoResponse {
                is_swarm_mode: false,
                swarm: None,
            })
        }
        Err(e) => {
            warn!("Failed to get swarm info: {}", e);
            Err(format!("Failed to get swarm info: {}", e))
        }
    }
}

/// Initialize a new swarm.
pub(crate) async fn init(
    docker: &DockerClient,
    listen_addr: &str,
    advertise_addr: &str,
    force_new_cluster: bool,
) -> Result<SwarmInitResponse, Status> {
    let listen_addr = if listen_addr.is_empty() { "0.0.0.0:2377" } else { listen_addr };
    info!(listen_addr = %listen_addr, "Initializing swarm");
    match docker.swarm_init(listen_addr, advertise_addr, force_new_cluster).await {
        Ok(node_id) => {
            info!(node_id = %node_id, "Swarm initialized");
            Ok(SwarmInitResponse {
                success: true,
                message: "Swarm initialized successfully".to_string(),
                node_id,
            })
        }
        Err(e) => Err(Status::internal(format!("Failed to initialize swarm: {}", e))),
    }
}

/// Join an existing swarm.
pub(crate) async fn join(
    docker: &DockerClient,
    remote_addrs: Vec<String>,
    join_token: &str,
    listen_addr: &str,
    advertise_addr: &str,
) -> Result<SwarmJoinResponse, Status> {
    let listen_addr = if listen_addr.is_empty() { "0.0.0.0:2377" } else { listen_addr };
    info!(remote_addrs = ?remote_addrs, "Joining swarm");
    match docker.swarm_join(remote_addrs, join_token, listen_addr, advertise_addr).await {
        Ok(()) => {
            info!("Successfully joined swarm");
            Ok(SwarmJoinResponse {
                success: true,
                message: "Successfully joined swarm".to_string(),
            })
        }
        Err(e) => Err(Status::internal(format!("Failed to join swarm: {}", e))),
    }
}

/// Leave the current swarm.
pub(crate) async fn leave(docker: &DockerClient, force: bool) -> Result<SwarmLeaveResponse, Status> {
    info!(force = force, "Leaving swarm");
    match docker.swarm_leave(force).await {
        Ok(()) => {
            info!("Successfully left swarm");
            Ok(SwarmLeaveResponse {
                success: true,
                message: "Successfully left swarm".to_string(),
            })
        }
        Err(e) => Err(Status::internal(format!("Failed to leave swarm: {}", e))),
    }
}

/// Retrieve the swarm unlock key.
pub(crate) async fn unlock_key(docker: &DockerClient) -> Result<SwarmUnlockKeyResponse, Status> {
    info!("Retrieving swarm unlock key");
    match docker.swarm_unlock_key().await {
        Ok(key) => Ok(SwarmUnlockKeyResponse {
            success: true,
            unlock_key: key,
            message: String::new(),
        }),
        Err(e) => Ok(SwarmUnlockKeyResponse {
            success: false,
            unlock_key: String::new(),
            message: format!("{}", e),
        }),
    }
}

/// Unlock a locked swarm.
pub(crate) async fn unlock(docker: &DockerClient, key: &str) -> Result<SwarmUnlockResponse, Status> {
    info!("Unlocking swarm");
    match docker.swarm_unlock(key).await {
        Ok(()) => Ok(SwarmUnlockResponse {
            success: true,
            message: "Swarm unlocked successfully".to_string(),
        }),
        Err(e) => Ok(SwarmUnlockResponse {
            success: false,
            message: format!("{}", e),
        }),
    }
}
