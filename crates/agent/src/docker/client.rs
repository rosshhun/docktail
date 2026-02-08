//! Docker client — core struct, constructor, error types.
//!
//! Domain methods live in sibling modules (`container`, `image`, `volume`,
//! `network`, `swarm`, `shell`, `event`) which add `impl DockerClient` blocks.

use bollard::Docker;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DockerError {
    #[error("Docker connection failed: {0}")]
    ConnectionFailed(String),
    #[error("Container not found: {0}")]
    ContainerNotFound(String),
    #[error("Permission denied")]
    PermissionDenied,
    #[error("Stream closed")]
    StreamClosed,
    #[error("Unsupported log driver: {0}")]
    UnsupportedLogDriver(String),
    #[error("This node is not a swarm manager. Swarm management operations require a manager node.")]
    NotSwarmManager,
    #[error("Bollard error: {0}")]
    BollardError(#[from] bollard::errors::Error),
}

/// Result of inspecting swarm state — distinguishes manager, worker, and not-in-swarm.
#[derive(Debug)]
pub enum SwarmInspectResult {
    /// This node is a swarm manager; full swarm info available.
    Manager(bollard::models::Swarm),
    /// This node is in a swarm but is a worker (503 from inspect_swarm).
    Worker,
    /// This node is not part of any swarm (406 from inspect_swarm).
    NotInSwarm,
}

#[derive(Debug, Clone)]
pub struct DockerClient {
    /// The bollard Docker client.  `pub(super)` so that domain modules
    /// in sibling files can call bollard APIs directly.
    pub(super) client: Docker,
    /// The Docker socket path this client is connected to.
    pub(super) socket_path: String,
}

impl DockerClient {
    pub fn new(socket_path: &str) -> Result<Self, DockerError> {
        let connection = if socket_path.is_empty() {
            Docker::connect_with_defaults()
                .map_err(|e| DockerError::ConnectionFailed(e.to_string()))?
        } else {
            let clean_path = socket_path.trim_start_matches("unix://");
            Docker::connect_with_socket(clean_path, 120, &bollard::API_DEFAULT_VERSION)
                .map_err(|e| DockerError::ConnectionFailed(e.to_string()))?
        };

        Ok(DockerClient {
            client: connection,
            socket_path: socket_path.to_string(),
        })
    }

    /// Build a `tokio::process::Command` for the Docker CLI that targets
    /// the same daemon this client is connected to.
    pub(super) fn docker_cli_command(&self) -> tokio::process::Command {
        let mut cmd = tokio::process::Command::new("docker");
        if !self.socket_path.is_empty() {
            let host = if self.socket_path.starts_with("unix://")
                || self.socket_path.starts_with("tcp://")
            {
                self.socket_path.clone()
            } else {
                format!("unix://{}", self.socket_path)
            };
            cmd.env("DOCKER_HOST", host);
        }
        cmd
    }

    /// Get Docker system information (includes swarm node_id, node_addr, etc.)
    pub async fn system_info(&self) -> Result<bollard::models::SystemInfo, DockerError> {
        self.client.info().await.map_err(DockerError::from)
    }
}
