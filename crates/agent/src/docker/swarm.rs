//! Swarm domain — swarm inspect, nodes, services, tasks, secrets, configs,
//! init/join/leave, update/unlock, and service/task log streaming.

use super::client::{DockerClient, DockerError, SwarmInspectResult};
use bollard::container::LogOutput;

impl DockerClient {
    /// Get swarm information.
    /// Returns Manager/Worker/NotInSwarm variants.
    pub async fn swarm_inspect(&self) -> Result<SwarmInspectResult, DockerError> {
        match self.client.inspect_swarm().await {
            Ok(swarm) => Ok(SwarmInspectResult::Manager(swarm)),
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 503, ..
            }) => Ok(SwarmInspectResult::Worker),
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 406, ..
            }) => Ok(SwarmInspectResult::NotInSwarm),
            Err(e) => Err(DockerError::from(e)),
        }
    }

    /// List all nodes in the swarm.
    pub async fn list_nodes(&self) -> Result<Vec<bollard::models::Node>, DockerError> {
        match self
            .client
            .list_nodes(None::<bollard::query_parameters::ListNodesOptions>)
            .await
        {
            Ok(nodes) => Ok(nodes),
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 503, ..
            }) => Err(DockerError::NotSwarmManager),
            Err(e) => Err(DockerError::from(e)),
        }
    }

    /// List all swarm services.
    pub async fn list_services(&self) -> Result<Vec<bollard::models::Service>, DockerError> {
        match self
            .client
            .list_services(None::<bollard::query_parameters::ListServicesOptions>)
            .await
        {
            Ok(services) => Ok(services),
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 503, ..
            }) => Err(DockerError::NotSwarmManager),
            Err(e) => Err(DockerError::from(e)),
        }
    }

    /// Inspect a specific swarm service.
    pub async fn inspect_service(
        &self,
        service_id: &str,
    ) -> Result<bollard::models::Service, DockerError> {
        self.client
            .inspect_service(service_id, None)
            .await
            .map_err(DockerError::from)
    }

    /// List tasks in the swarm.
    pub async fn list_tasks(&self) -> Result<Vec<bollard::models::Task>, DockerError> {
        match self
            .client
            .list_tasks(None::<bollard::query_parameters::ListTasksOptions>)
            .await
        {
            Ok(tasks) => Ok(tasks),
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 503, ..
            }) => Err(DockerError::NotSwarmManager),
            Err(e) => Err(DockerError::from(e)),
        }
    }

    /// List all swarm secrets (metadata only).
    pub async fn list_secrets(&self) -> Result<Vec<bollard::models::Secret>, DockerError> {
        match self
            .client
            .list_secrets(None::<bollard::query_parameters::ListSecretsOptions>)
            .await
        {
            Ok(secrets) => Ok(secrets),
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 503, ..
            }) => Err(DockerError::NotSwarmManager),
            Err(e) => Err(DockerError::from(e)),
        }
    }

    /// List all swarm configs (metadata only).
    pub async fn list_configs(&self) -> Result<Vec<bollard::models::Config>, DockerError> {
        match self
            .client
            .list_configs(None::<bollard::query_parameters::ListConfigsOptions>)
            .await
        {
            Ok(configs) => Ok(configs),
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 503, ..
            }) => Err(DockerError::NotSwarmManager),
            Err(e) => Err(DockerError::from(e)),
        }
    }

    // ── Node Management ───────────────────────────────────────────

    /// Inspect a single node by ID.
    pub async fn inspect_node(
        &self,
        node_id: &str,
    ) -> Result<Option<bollard::models::Node>, DockerError> {
        match self.client.inspect_node(node_id).await {
            Ok(node) => Ok(Some(node)),
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 503, ..
            }) => Ok(None),
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => Ok(None),
            Err(e) => Err(DockerError::from(e)),
        }
    }

    /// Update node availability, role, and/or labels.
    pub async fn update_node(
        &self,
        node_id: &str,
        spec: bollard::models::NodeSpec,
        version: i64,
    ) -> Result<(), DockerError> {
        let options = bollard::query_parameters::UpdateNodeOptionsBuilder::new()
            .version(version)
            .build();
        self.client
            .update_node(node_id, spec, options)
            .await
            .map_err(DockerError::from)
    }

    /// Remove a node from the swarm.
    pub async fn remove_node(&self, node_id: &str, force: bool) -> Result<(), DockerError> {
        use bollard::query_parameters::DeleteNodeOptionsBuilder;
        let options = DeleteNodeOptionsBuilder::default().force(force).build();
        self.client
            .delete_node(node_id, Some(options))
            .await
            .map_err(DockerError::from)
    }

    // ── Service CRUD ──────────────────────────────────────────────

    /// Stream aggregated logs from all tasks of a swarm service.
    pub fn stream_service_logs(
        &self,
        service_id: &str,
        follow: bool,
        tail: Option<String>,
        since: i32,
        until: i32,
        timestamps: bool,
    ) -> std::pin::Pin<
        Box<dyn futures_util::Stream<Item = Result<LogOutput, bollard::errors::Error>> + Send>,
    > {
        let options = bollard::query_parameters::LogsOptions {
            follow,
            stdout: true,
            stderr: true,
            since,
            until,
            timestamps,
            tail: tail.unwrap_or_else(|| "all".to_string()),
        };
        let client = self.client.clone();
        let service_id = service_id.to_string();
        Box::pin(client.service_logs(&service_id, Some(options)))
    }

    /// Create a new swarm service. Returns the service ID.
    pub async fn create_service(
        &self,
        spec: bollard::models::ServiceSpec,
        registry_auth: Option<&str>,
    ) -> Result<String, DockerError> {
        let credentials = if let Some(auth) = registry_auth.filter(|s| !s.is_empty()) {
            Some(bollard::auth::DockerCredentials {
                username: None,
                password: None,
                auth: Some(auth.to_string()),
                email: None,
                serveraddress: None,
                identitytoken: None,
                registrytoken: None,
            })
        } else {
            None
        };
        let result = self
            .client
            .create_service(spec, credentials)
            .await
            .map_err(DockerError::from)?;
        result
            .id
            .filter(|id| !id.is_empty())
            .ok_or_else(|| {
                DockerError::ConnectionFailed(
                    "Docker returned success but did not provide a service ID".to_string(),
                )
            })
    }

    /// Delete a swarm service.
    pub async fn delete_service(&self, service_id: &str) -> Result<(), DockerError> {
        self.client
            .delete_service(service_id)
            .await
            .map_err(DockerError::from)
    }

    /// Update an existing swarm service.
    pub async fn update_service(
        &self,
        service_id: &str,
        spec: bollard::models::ServiceSpec,
        version: u64,
        _force: bool,
        registry_auth: Option<&str>,
    ) -> Result<(), DockerError> {
        use bollard::query_parameters::UpdateServiceOptions;

        let version_i32 = i32::try_from(version).map_err(|_| {
            DockerError::ConnectionFailed(format!(
                "Service version index {} exceeds i32::MAX; cannot update via bollard",
                version
            ))
        })?;
        let opts = UpdateServiceOptions {
            version: version_i32,
            ..Default::default()
        };

        let credentials = if let Some(auth) = registry_auth.filter(|s| !s.is_empty()) {
            Some(bollard::auth::DockerCredentials {
                username: None,
                password: None,
                auth: Some(auth.to_string()),
                email: None,
                serveraddress: None,
                identitytoken: None,
                registrytoken: None,
            })
        } else {
            None
        };

        self.client
            .update_service(service_id, spec, opts, credentials)
            .await
            .map(|_| ())
            .map_err(DockerError::from)
    }

    /// Rollback a service to its previous specification.
    pub async fn rollback_service(&self, service_id: &str) -> Result<(), DockerError> {
        use bollard::query_parameters::UpdateServiceOptions;

        let service = self.inspect_service(service_id).await?;
        let version = service
            .version
            .as_ref()
            .and_then(|v| v.index)
            .ok_or_else(|| {
                DockerError::ConnectionFailed("Service has no version".to_string())
            })?;

        let spec = service.spec.unwrap_or_default();

        let version_i32 = i32::try_from(version).map_err(|_| {
            DockerError::ConnectionFailed(format!(
                "Service version index {} exceeds i32::MAX; cannot rollback via bollard",
                version
            ))
        })?;
        let opts = UpdateServiceOptions {
            version: version_i32,
            rollback: Some("previous".to_string()),
            ..Default::default()
        };

        self.client
            .update_service(service_id, spec, opts, None)
            .await
            .map(|_| ())
            .map_err(DockerError::from)
    }

    // ── Secrets & Configs ─────────────────────────────────────────

    /// Create a swarm secret.
    pub async fn create_secret(
        &self,
        name: &str,
        data: &[u8],
        labels: std::collections::HashMap<String, String>,
    ) -> Result<String, DockerError> {
        use base64::Engine as _;
        use bollard::models::SecretSpec;

        let spec = SecretSpec {
            name: Some(name.to_string()),
            data: Some(base64::engine::general_purpose::STANDARD.encode(data)),
            labels: Some(labels),
            ..Default::default()
        };

        let result = self
            .client
            .create_secret(spec)
            .await
            .map_err(DockerError::from)?;
        Ok(result.id)
    }

    /// Delete a swarm secret.
    pub async fn delete_secret(&self, secret_id: &str) -> Result<(), DockerError> {
        self.client
            .delete_secret(secret_id)
            .await
            .map_err(DockerError::from)
    }

    /// Create a swarm config.
    pub async fn create_config(
        &self,
        name: &str,
        data: &[u8],
        labels: std::collections::HashMap<String, String>,
    ) -> Result<String, DockerError> {
        use base64::Engine as _;
        use bollard::models::ConfigSpec;

        let spec = ConfigSpec {
            name: Some(name.to_string()),
            data: Some(base64::engine::general_purpose::STANDARD.encode(data)),
            labels: Some(labels),
            ..Default::default()
        };

        let result = self
            .client
            .create_config(spec)
            .await
            .map_err(DockerError::from)?;
        Ok(result.id)
    }

    /// Delete a swarm config.
    pub async fn delete_config(&self, config_id: &str) -> Result<(), DockerError> {
        self.client
            .delete_config(config_id)
            .await
            .map_err(DockerError::from)
    }

    // ── Swarm Init / Join / Leave ─────────────────────────────────

    /// Initialize a new swarm.
    pub async fn swarm_init(
        &self,
        listen_addr: &str,
        advertise_addr: &str,
        force_new_cluster: bool,
    ) -> Result<String, DockerError> {
        use bollard::models::SwarmInitRequest;

        let request = SwarmInitRequest {
            listen_addr: Some(listen_addr.to_string()),
            advertise_addr: if advertise_addr.is_empty() {
                None
            } else {
                Some(advertise_addr.to_string())
            },
            force_new_cluster: Some(force_new_cluster),
            ..Default::default()
        };

        self.client
            .init_swarm(request)
            .await
            .map_err(DockerError::from)
    }

    /// Join an existing swarm.
    pub async fn swarm_join(
        &self,
        remote_addrs: Vec<String>,
        join_token: &str,
        listen_addr: &str,
        advertise_addr: &str,
    ) -> Result<(), DockerError> {
        use bollard::models::SwarmJoinRequest;

        let request = SwarmJoinRequest {
            remote_addrs: Some(remote_addrs),
            join_token: Some(join_token.to_string()),
            listen_addr: Some(listen_addr.to_string()),
            advertise_addr: if advertise_addr.is_empty() {
                None
            } else {
                Some(advertise_addr.to_string())
            },
            ..Default::default()
        };

        self.client
            .join_swarm(request)
            .await
            .map_err(DockerError::from)
    }

    /// Leave the swarm.
    pub async fn swarm_leave(&self, force: bool) -> Result<(), DockerError> {
        use bollard::query_parameters::LeaveSwarmOptionsBuilder;
        let options = LeaveSwarmOptionsBuilder::default().force(force).build();
        self.client
            .leave_swarm(Some(options))
            .await
            .map_err(DockerError::from)
    }

    // ── Task Inspect & Task Logs ──────────────────────────────────

    /// Inspect a single swarm task by ID.
    pub async fn inspect_task(
        &self,
        task_id: &str,
    ) -> Result<Option<bollard::models::Task>, DockerError> {
        match self.client.inspect_task(task_id).await {
            Ok(task) => Ok(Some(task)),
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => Ok(None),
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 503, ..
            }) => Ok(None),
            Err(e) => Err(DockerError::from(e)),
        }
    }

    /// Stream logs from a single swarm task.
    pub fn stream_task_logs(
        &self,
        task_id: &str,
        follow: bool,
        tail: Option<String>,
        since: i32,
        until: i32,
        timestamps: bool,
    ) -> std::pin::Pin<
        Box<dyn futures_util::Stream<Item = Result<LogOutput, bollard::errors::Error>> + Send>,
    > {
        let options = bollard::query_parameters::LogsOptions {
            follow,
            stdout: true,
            stderr: true,
            since,
            until,
            timestamps,
            tail: tail.unwrap_or_else(|| "all".to_string()),
        };
        let client = self.client.clone();
        let task_id = task_id.to_string();
        Box::pin(client.task_logs(&task_id, Some(options)))
    }

    // ── Swarm Update / Unlock ─────────────────────────────────────

    /// Update swarm settings.
    pub async fn swarm_update(
        &self,
        spec: bollard::models::SwarmSpec,
        version: i64,
        rotate_worker_token: bool,
        rotate_manager_token: bool,
        rotate_manager_unlock_key: bool,
    ) -> Result<(), DockerError> {
        use bollard::query_parameters::UpdateSwarmOptionsBuilder;

        let options = UpdateSwarmOptionsBuilder::default()
            .version(version)
            .rotate_worker_token(rotate_worker_token)
            .rotate_manager_token(rotate_manager_token)
            .rotate_manager_unlock_key(rotate_manager_unlock_key)
            .build();

        self.client
            .update_swarm(spec, options)
            .await
            .map_err(DockerError::from)
    }

    /// Get the swarm unlock key via Docker CLI.
    pub async fn swarm_unlock_key(&self) -> Result<String, DockerError> {
        let swarm = self.swarm_inspect().await?;
        let s = match swarm {
            SwarmInspectResult::Manager(s) => s,
            SwarmInspectResult::Worker => {
                return Err(DockerError::ConnectionFailed(
                    "This node is a worker, not a manager. Cannot retrieve unlock key.".to_string(),
                ))
            }
            SwarmInspectResult::NotInSwarm => {
                return Err(DockerError::ConnectionFailed(
                    "Not in swarm mode".to_string(),
                ))
            }
        };
        let autolock = s
            .spec
            .as_ref()
            .and_then(|spec| spec.encryption_config.as_ref())
            .and_then(|enc| enc.auto_lock_managers)
            .unwrap_or(false);
        if !autolock {
            return Err(DockerError::ConnectionFailed(
                "Swarm autolock is not enabled. Enable it first via swarm update with autolock=true."
                    .to_string(),
            ));
        }

        let output = self
            .docker_cli_command()
            .args(["swarm", "unlock-key", "-q"])
            .output()
            .await
            .map_err(|e| {
                DockerError::ConnectionFailed(format!("Failed to run docker CLI: {}", e))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(DockerError::ConnectionFailed(format!(
                "docker swarm unlock-key failed: {}",
                stderr.trim()
            )));
        }

        let key = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if key.is_empty() {
            return Err(DockerError::ConnectionFailed(
                "Docker returned an empty unlock key. Try rotating the unlock key via swarm update."
                    .to_string(),
            ));
        }
        Ok(key)
    }

    /// Unlock the swarm after manager restart when autolock is enabled.
    pub async fn swarm_unlock(&self, unlock_key: &str) -> Result<(), DockerError> {
        use tokio::io::AsyncWriteExt;

        let mut child = self
            .docker_cli_command()
            .args(["swarm", "unlock"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| {
                DockerError::ConnectionFailed(format!("Failed to run docker CLI: {}", e))
            })?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(unlock_key.as_bytes()).await.map_err(|e| {
                DockerError::ConnectionFailed(format!("Failed to send unlock key: {}", e))
            })?;
            stdin.write_all(b"\n").await.ok();
            drop(stdin);
        }

        let output = child.wait_with_output().await.map_err(|e| {
            DockerError::ConnectionFailed(format!("Failed to wait for docker CLI: {}", e))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(DockerError::ConnectionFailed(format!(
                "docker swarm unlock failed: {}",
                stderr.trim()
            )));
        }
        Ok(())
    }
}
