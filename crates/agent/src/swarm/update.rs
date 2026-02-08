//! Update — swarm cluster settings update.

use tonic::Status;
use tracing::info;

use crate::docker::client::{DockerClient, SwarmInspectResult};
use crate::proto::SwarmUpdateResponse;

/// Apply swarm-level settings (autolock, raft, cert expiry, token rotation).
pub(crate) async fn update_swarm(
    docker: &DockerClient,
    autolock: Option<bool>,
    task_history_limit: Option<i64>,
    snapshot_interval: Option<u64>,
    heartbeat_tick: Option<u64>,
    election_tick: Option<u64>,
    cert_expiry_ns: Option<i64>,
    rotate_worker_token: bool,
    rotate_manager_token: bool,
    rotate_manager_unlock_key: bool,
) -> Result<SwarmUpdateResponse, Status> {
    info!("Updating swarm settings");

    let swarm = match docker.swarm_inspect().await
        .map_err(|e| Status::internal(format!("Failed to inspect swarm: {}", e)))? {
        SwarmInspectResult::Manager(s) => s,
        SwarmInspectResult::Worker => return Err(Status::failed_precondition(
            "This node is a worker, not a manager. Swarm updates require a manager node."
        )),
        SwarmInspectResult::NotInSwarm => return Err(Status::failed_precondition("Not in swarm mode")),
    };

    let version = swarm.version.as_ref()
        .and_then(|v| v.index)
        .ok_or_else(|| Status::internal("Swarm has no version"))? as i64;

    let mut spec = swarm.spec.unwrap_or_default();

    if let Some(al) = autolock {
        let enc = spec.encryption_config.get_or_insert_with(Default::default);
        enc.auto_lock_managers = Some(al);
    }

    if let Some(thl) = task_history_limit {
        let orch = spec.orchestration.get_or_insert_with(Default::default);
        orch.task_history_retention_limit = Some(thl);
    }

    if let Some(si) = snapshot_interval {
        let raft = spec.raft.get_or_insert_with(Default::default);
        raft.snapshot_interval = Some(si);
    }

    if let Some(ht) = heartbeat_tick {
        let raft = spec.raft.get_or_insert_with(Default::default);
        raft.heartbeat_tick = Some(ht as i64);
    }

    if let Some(et) = election_tick {
        let raft = spec.raft.get_or_insert_with(Default::default);
        raft.election_tick = Some(et as i64);
    }

    if let Some(ce) = cert_expiry_ns {
        let ca = spec.ca_config.get_or_insert_with(Default::default);
        ca.node_cert_expiry = Some(ce);
    }

    match docker.swarm_update(
        spec,
        version,
        rotate_worker_token,
        rotate_manager_token,
        rotate_manager_unlock_key,
    ).await {
        Ok(()) => Ok(SwarmUpdateResponse {
            success: true,
            message: "Swarm settings updated successfully".to_string(),
        }),
        Err(e) => Ok(SwarmUpdateResponse {
            success: false,
            message: format!("Failed to update swarm: {}", e),
        }),
    }
}
