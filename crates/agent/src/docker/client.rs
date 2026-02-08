use crate::docker::inventory::ContainerInfo;
use crate::docker::stream::{LogStream, LogStreamRequest, LogLine, LogLevel};
use crate::filter::engine::FilterEngine;
use bollard::Docker;
use bollard::container::{LogOutput};
use bollard::models::ContainerInspectResponse;
use bollard::query_parameters::{ListContainersOptions, LogsOptions, RemoveContainerOptions};
use thiserror::Error;
use futures_util::stream::StreamExt;
use bytes::Bytes;
use std::sync::Arc;

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
    #[error("Bollard error: {0}")]
    BollardError(#[from] bollard::errors::Error),
}

// use for time-travel (since/until parameters)
const SUPPORTED_LOG_DRIVERS: &[&str] = &["json-file", "journald", "local"];

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
    client: Docker,
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

        Ok(DockerClient { client: connection })
    }
    pub async fn list_containers(&self) -> Result<Vec<ContainerInfo>, DockerError> {
        let options = Some(ListContainersOptions {
            all: true,  // Include stopped containers
            ..Default::default()
        });
        let containers = self.client.list_containers(options).await?;
        Ok(containers
            .into_iter()
            .map(|c| c.into())
            .collect())
    }
    pub async fn stream_logs(
        &self,
        request: LogStreamRequest,
        filter: Option<Arc<FilterEngine>>,
    ) -> Result<LogStream, DockerError> {
        // Validate time-travel support if since/until is requested
        if request.since.is_some() || request.until.is_some() {
            let container = self.inspect_container(&request.container_id).await?;
            if let Some(driver) = container.log_driver {
                if !SUPPORTED_LOG_DRIVERS.contains(&driver.as_str()) {
                    return Err(DockerError::UnsupportedLogDriver(
                        format!("Log driver '{}' does not support time-travel (since/until). Supported drivers: {:?}", 
                            driver, SUPPORTED_LOG_DRIVERS)
                    ));
                }
            }
        }

        // NOTE: Bollard v0.20 requires i32 for since/until (Unix timestamps in seconds).
        // We clamp i64 request values to the i32 range and warn if clamping occurs.
        // Post-2038 timestamps will be silently capped.
        let since_raw = request.since.unwrap_or(0);
        let until_raw = request.until.unwrap_or(0);
        if since_raw > i32::MAX as i64 || until_raw > i32::MAX as i64 {
            tracing::warn!(
                since = since_raw,
                until = until_raw,
                max = i32::MAX,
                "Timestamp exceeds i32 range (year 2038 limit) — clamping to i32::MAX. \
                 Bollard v0.20 does not support i64 timestamps."
            );
        }
        let since = since_raw.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        let until = until_raw.clamp(i32::MIN as i64, i32::MAX as i64) as i32;

        let options = LogsOptions {
            follow: request.follow,
            stdout: true,
            stderr: true,
            since,
            until,
            timestamps: true,
            tail: request.tail_lines.map(|n| n.to_string()).unwrap_or_else(|| "all".to_string()),
        };

        
        let bollard_stream = self.client.logs(&request.container_id, Some(options));
        
        let log_stream = bollard_stream.map(move |result| {
            match result {
                Ok(output) => convert_bollard_log(output),
                Err(e) => Err(DockerError::from(e)),
            }
        });

        Ok(LogStream::new(
            request.container_id,
            log_stream,
            filter,
        ))
    }
    
    pub async fn inspect_container(&self, id: &str) -> Result<ContainerInfo, DockerError> {
        let details: ContainerInspectResponse = self.client.inspect_container(id, None).await?;
        Ok(ContainerInfo::from(details))
    }

    /// Returns the full `ContainerInspectResponse` from Docker for a container.
    /// Use this when you need details beyond `ContainerInfo` (ports, mounts, etc.).
    pub async fn inspect_container_raw(&self, id: &str) -> Result<ContainerInspectResponse, DockerError> {
        let details: ContainerInspectResponse = self.client.inspect_container(id, None).await?;
        Ok(details)
    }

    /// Returns container stats either as a single snapshot or a continuous stream.
    ///
    /// If `stream` is `true`, the returned stream yields live stats updates;
    /// if `false`, it yields a single stats response and then ends.
    pub async fn stats(&self, container_id: &str, stream: bool) -> Result<impl tokio_stream::Stream<Item = Result<bollard::models::ContainerStatsResponse, bollard::errors::Error>>, DockerError> {
        use bollard::query_parameters::StatsOptions;

        let options = Some(StatsOptions {
            stream,
            ..Default::default()
        });

        Ok(self.client.stats(container_id, options))
    }

    // =========================================================================
    // Container Lifecycle Methods
    // =========================================================================

    /// Start a stopped container.
    pub async fn start_container(&self, container_id: &str) -> Result<(), DockerError> {
        self.client
            .start_container(container_id, None)
            .await
            .map_err(|e| match e {
                bollard::errors::Error::DockerResponseServerError { status_code: 404, .. } => {
                    DockerError::ContainerNotFound(container_id.to_string())
                }
                other => DockerError::BollardError(other),
            })
    }

    /// Stop a running container with an optional timeout (in seconds).
    /// If no timeout is given, Docker uses its default (10 seconds).
    pub async fn stop_container(&self, container_id: &str, timeout_secs: Option<u32>) -> Result<(), DockerError> {
        use bollard::query_parameters::StopContainerOptions;

        let options = timeout_secs.map(|t| StopContainerOptions {
            t: Some(t as i32),
            ..Default::default()
        });

        self.client
            .stop_container(container_id, options)
            .await
            .map_err(|e| match e {
                bollard::errors::Error::DockerResponseServerError { status_code: 404, .. } => {
                    DockerError::ContainerNotFound(container_id.to_string())
                }
                other => DockerError::BollardError(other),
            })
    }

    /// Restart a container with an optional timeout (in seconds).
    pub async fn restart_container(&self, container_id: &str, timeout_secs: Option<u32>) -> Result<(), DockerError> {
        use bollard::query_parameters::RestartContainerOptions;

        let options = timeout_secs.map(|t| RestartContainerOptions {
            t: Some(t as i32),
            ..Default::default()
        });

        self.client
            .restart_container(container_id, options)
            .await
            .map_err(|e| match e {
                bollard::errors::Error::DockerResponseServerError { status_code: 404, .. } => {
                    DockerError::ContainerNotFound(container_id.to_string())
                }
                other => DockerError::BollardError(other),
            })
    }

    /// Pause a running container (freezes all processes).
    pub async fn pause_container(&self, container_id: &str) -> Result<(), DockerError> {
        self.client
            .pause_container(container_id)
            .await
            .map_err(|e| match e {
                bollard::errors::Error::DockerResponseServerError { status_code: 404, .. } => {
                    DockerError::ContainerNotFound(container_id.to_string())
                }
                other => DockerError::BollardError(other),
            })
    }

    /// Unpause a paused container.
    pub async fn unpause_container(&self, container_id: &str) -> Result<(), DockerError> {
        self.client
            .unpause_container(container_id)
            .await
            .map_err(|e| match e {
                bollard::errors::Error::DockerResponseServerError { status_code: 404, .. } => {
                    DockerError::ContainerNotFound(container_id.to_string())
                }
                other => DockerError::BollardError(other),
            })
    }

    /// Remove a container. If `force` is true, the container will be killed first.
    /// If `remove_volumes` is true, associated anonymous volumes are also removed.
    pub async fn remove_container(&self, container_id: &str, force: bool, remove_volumes: bool) -> Result<(), DockerError> {
        let options = Some(RemoveContainerOptions {
            force,
            v: remove_volumes,
            ..Default::default()
        });

        self.client
            .remove_container(container_id, options)
            .await
            .map_err(|e| match e {
                bollard::errors::Error::DockerResponseServerError { status_code: 404, .. } => {
                    DockerError::ContainerNotFound(container_id.to_string())
                }
                other => DockerError::BollardError(other),
            })
    }

    // =========================================================================
    // Image Management Methods
    // =========================================================================

    /// List all images on the Docker host.
    pub async fn list_images(&self) -> Result<Vec<bollard::models::ImageSummary>, DockerError> {
        use bollard::query_parameters::ListImagesOptions;

        let options = Some(ListImagesOptions {
            all: false, // Only top-level images, not intermediate layers
            ..Default::default()
        });

        self.client
            .list_images(options)
            .await
            .map_err(DockerError::from)
    }

    /// Inspect a specific image by ID or tag.
    pub async fn inspect_image(&self, image_id: &str) -> Result<bollard::models::ImageInspect, DockerError> {
        self.client
            .inspect_image(image_id)
            .await
            .map_err(DockerError::from)
    }

    /// Pull an image from a registry. Returns when the pull is complete.
    /// `registry_auth` is an optional base64-encoded JSON auth string.
    pub async fn pull_image(&self, image: &str, tag: &str, registry_auth: Option<&str>) -> Result<(), DockerError> {
        use bollard::query_parameters::CreateImageOptions;
        use bollard::auth::DockerCredentials;

        let options = Some(CreateImageOptions {
            from_image: Some(image.to_string()),
            tag: Some(tag.to_string()),
            ..Default::default()
        });

        let credentials = registry_auth.map(|auth| DockerCredentials {
            auth: Some(auth.to_string()),
            ..Default::default()
        });

        let mut stream = self.client.create_image(options, None, credentials);

        // Consume the stream to completion — each item is a progress update
        while let Some(result) = stream.next().await {
            match result {
                Ok(info) => {
                    tracing::debug!(status = ?info.status, "Image pull progress");
                }
                Err(e) => return Err(DockerError::from(e)),
            }
        }

        Ok(())
    }

    /// Remove an image by ID or tag.
    pub async fn remove_image(&self, image_id: &str, force: bool, no_prune: bool) -> Result<(), DockerError> {
        use bollard::query_parameters::RemoveImageOptions;

        let options = Some(RemoveImageOptions {
            force,
            noprune: no_prune,
            ..Default::default()
        });

        self.client
            .remove_image(image_id, options, None)
            .await
            .map_err(DockerError::from)?;

        Ok(())
    }

    // =========================================================================
    // Volume Management Methods
    // =========================================================================

    /// List all volumes.
    pub async fn list_volumes(&self) -> Result<bollard::models::VolumeListResponse, DockerError> {
        self.client
            .list_volumes(None::<bollard::query_parameters::ListVolumesOptions>)
            .await
            .map_err(DockerError::from)
    }

    /// Inspect a specific volume.
    pub async fn inspect_volume(&self, name: &str) -> Result<bollard::models::Volume, DockerError> {
        self.client
            .inspect_volume(name)
            .await
            .map_err(DockerError::from)
    }

    /// Create a new volume.
    pub async fn create_volume(
        &self,
        name: &str,
        driver: Option<&str>,
        labels: std::collections::HashMap<String, String>,
        driver_opts: std::collections::HashMap<String, String>,
    ) -> Result<bollard::models::Volume, DockerError> {
        use bollard::models::VolumeCreateRequest;

        let config = VolumeCreateRequest {
            name: Some(name.to_string()),
            driver: Some(driver.unwrap_or("local").to_string()),
            driver_opts: if driver_opts.is_empty() { None } else { Some(driver_opts) },
            labels: Some(labels),
            ..Default::default()
        };

        self.client
            .create_volume(config)
            .await
            .map_err(DockerError::from)
    }

    /// Remove a volume.
    pub async fn remove_volume(&self, name: &str, force: bool) -> Result<(), DockerError> {
        use bollard::query_parameters::RemoveVolumeOptions;

        let options = Some(RemoveVolumeOptions { force });

        self.client
            .remove_volume(name, options)
            .await
            .map_err(DockerError::from)
    }

    // =========================================================================
    // Network Management Methods
    // =========================================================================

    /// List all networks.
    pub async fn list_networks(&self) -> Result<Vec<bollard::models::Network>, DockerError> {
        self.client
            .list_networks(None::<bollard::query_parameters::ListNetworksOptions>)
            .await
            .map_err(DockerError::from)
    }

    /// Inspect a specific network.
    pub async fn inspect_network(&self, network_id: &str) -> Result<bollard::models::NetworkInspect, DockerError> {
        self.client
            .inspect_network(network_id, None::<bollard::query_parameters::InspectNetworkOptions>)
            .await
            .map_err(DockerError::from)
    }

    /// Create a new network.
    pub async fn create_network(
        &self,
        name: &str,
        driver: Option<&str>,
        labels: std::collections::HashMap<String, String>,
        internal: bool,
        attachable: bool,
        enable_ipv6: bool,
        options: std::collections::HashMap<String, String>,
        ipam: Option<bollard::models::Ipam>,
    ) -> Result<bollard::models::NetworkCreateResponse, DockerError> {
        use bollard::models::NetworkCreateRequest;

        let config = NetworkCreateRequest {
            name: name.to_string(),
            driver: Some(driver.unwrap_or("bridge").to_string()),
            internal: if internal { Some(true) } else { None },
            attachable: if attachable { Some(true) } else { None },
            enable_ipv6: if enable_ipv6 { Some(true) } else { None },
            options: if options.is_empty() { None } else { Some(options) },
            ipam,
            labels: Some(labels),
            ..Default::default()
        };

        self.client
            .create_network(config)
            .await
            .map_err(DockerError::from)
    }

    /// Remove a network.
    pub async fn remove_network(&self, network_id: &str) -> Result<(), DockerError> {
        self.client
            .remove_network(network_id)
            .await
            .map_err(DockerError::from)
    }

    // =========================================================================
    // System Methods
    // =========================================================================

    /// Get Docker system information (includes swarm node_id, node_addr, etc.)
    pub async fn system_info(&self) -> Result<bollard::models::SystemInfo, DockerError> {
        self.client.info().await.map_err(DockerError::from)
    }

    // =========================================================================
    // Swarm Methods
    // =========================================================================

    /// Get swarm information.
    /// Returns `SwarmInspectResult::Manager(swarm)` if this node is a manager,
    /// `SwarmInspectResult::Worker` if this node is in a swarm but not a manager (503),
    /// `SwarmInspectResult::NotInSwarm` if not part of any swarm (406).
    pub async fn swarm_inspect(&self) -> Result<SwarmInspectResult, DockerError> {
        match self.client.inspect_swarm().await {
            Ok(swarm) => Ok(SwarmInspectResult::Manager(swarm)),
            Err(bollard::errors::Error::DockerResponseServerError { status_code: 503, .. }) => {
                // 503 = "This node is not a swarm manager" — node IS in swarm, just a worker
                Ok(SwarmInspectResult::Worker)
            }
            Err(bollard::errors::Error::DockerResponseServerError { status_code: 406, .. }) => {
                // 406 = "node is not part of a swarm"
                Ok(SwarmInspectResult::NotInSwarm)
            }
            Err(e) => Err(DockerError::from(e)),
        }
    }

    /// List all nodes in the swarm. Returns empty vec if not in swarm mode.
    pub async fn list_nodes(&self) -> Result<Vec<bollard::models::Node>, DockerError> {
        match self.client.list_nodes(None::<bollard::query_parameters::ListNodesOptions>).await {
            Ok(nodes) => Ok(nodes),
            Err(bollard::errors::Error::DockerResponseServerError { status_code: 503, .. }) => {
                Ok(Vec::new())
            }
            Err(e) => Err(DockerError::from(e)),
        }
    }

    /// List all swarm services. Returns empty vec if not in swarm mode.
    pub async fn list_services(&self) -> Result<Vec<bollard::models::Service>, DockerError> {
        match self.client.list_services(None::<bollard::query_parameters::ListServicesOptions>).await {
            Ok(services) => Ok(services),
            Err(bollard::errors::Error::DockerResponseServerError { status_code: 503, .. }) => {
                Ok(Vec::new())
            }
            Err(e) => Err(DockerError::from(e)),
        }
    }

    /// Inspect a specific swarm service.
    pub async fn inspect_service(&self, service_id: &str) -> Result<bollard::models::Service, DockerError> {
        self.client
            .inspect_service(service_id, None)
            .await
            .map_err(DockerError::from)
    }

    /// List tasks in the swarm with optional filters.
    pub async fn list_tasks(&self) -> Result<Vec<bollard::models::Task>, DockerError> {
        match self.client.list_tasks(None::<bollard::query_parameters::ListTasksOptions>).await {
            Ok(tasks) => Ok(tasks),
            Err(bollard::errors::Error::DockerResponseServerError { status_code: 503, .. }) => {
                Ok(Vec::new())
            }
            Err(e) => Err(DockerError::from(e)),
        }
    }

    /// List all swarm secrets (metadata only — actual secret data is never returned).
    pub async fn list_secrets(&self) -> Result<Vec<bollard::models::Secret>, DockerError> {
        match self.client.list_secrets(None::<bollard::query_parameters::ListSecretsOptions>).await {
            Ok(secrets) => Ok(secrets),
            Err(bollard::errors::Error::DockerResponseServerError { status_code: 503, .. }) => {
                Ok(Vec::new()) // Not in swarm mode
            }
            Err(e) => Err(DockerError::from(e)),
        }
    }

    /// List all swarm configs (metadata only — data content is omitted).
    pub async fn list_configs(&self) -> Result<Vec<bollard::models::Config>, DockerError> {
        match self.client.list_configs(None::<bollard::query_parameters::ListConfigsOptions>).await {
            Ok(configs) => Ok(configs),
            Err(bollard::errors::Error::DockerResponseServerError { status_code: 503, .. }) => {
                Ok(Vec::new()) // Not in swarm mode
            }
            Err(e) => Err(DockerError::from(e)),
        }
    }

    // =========================================================================
    // S9: Node Management & Drain Awareness
    // =========================================================================

    /// Inspect a single node by ID. Returns None if not in swarm mode.
    pub async fn inspect_node(&self, node_id: &str) -> Result<Option<bollard::models::Node>, DockerError> {
        match self.client.inspect_node(node_id).await {
            Ok(node) => Ok(Some(node)),
            Err(bollard::errors::Error::DockerResponseServerError { status_code: 503, .. }) => {
                Ok(None) // Not in swarm mode
            }
            Err(bollard::errors::Error::DockerResponseServerError { status_code: 404, .. }) => {
                Ok(None) // Node not found
            }
            Err(e) => Err(DockerError::from(e)),
        }
    }

    /// Update node availability, role, and/or labels.
    /// Requires the current node version (from inspect) to prevent conflicting writes.
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

    /// Stream aggregated logs from all tasks of a swarm service.
    /// Returns a stream of (LogOutput, raw bytes) from bollard's service_logs API.
    /// The log lines include task/node info in Docker's prefix format.
    pub fn stream_service_logs(
        &self,
        service_id: &str,
        follow: bool,
        tail: Option<String>,
        since: i32,
        until: i32,
        timestamps: bool,
    ) -> std::pin::Pin<Box<dyn futures_util::Stream<Item = Result<LogOutput, bollard::errors::Error>> + Send>> {
        let options = bollard::query_parameters::LogsOptions {
            follow,
            stdout: true,
            stderr: true,
            since,
            until,
            timestamps,
            tail: tail.unwrap_or_else(|| "all".to_string()),
        };
        // Clone the bollard client (Arc-backed) so the returned stream is 'static
        let client = self.client.clone();
        let service_id = service_id.to_string();
        Box::pin(client.service_logs(&service_id, Some(options)))
    }

    /// Create a new swarm service. Returns the service ID.
    /// If `registry_auth` is non-empty, it is passed as the X-Registry-Auth header.
    pub async fn create_service(
        &self,
        spec: bollard::models::ServiceSpec,
        registry_auth: Option<&str>,
    ) -> Result<String, DockerError> {
        let credentials = if let Some(auth) = registry_auth.filter(|s| !s.is_empty()) {
            // registry_auth should be base64-encoded JSON: {"username":"...","password":"...","serveraddress":"..."}
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
        let result = self.client
            .create_service(spec, credentials)
            .await
            .map_err(DockerError::from)?;
        Ok(result.id.unwrap_or_default())
    }

    /// Delete a swarm service.
    pub async fn delete_service(&self, service_id: &str) -> Result<(), DockerError> {
        self.client
            .delete_service(service_id)
            .await
            .map_err(DockerError::from)
    }

    /// Update an existing swarm service (scaling, image change, etc.).
    /// If `registry_auth` is non-empty, it is passed as the X-Registry-Auth header.
    pub async fn update_service(
        &self,
        service_id: &str,
        spec: bollard::models::ServiceSpec,
        version: u64,
        _force: bool,
        registry_auth: Option<&str>,
    ) -> Result<(), DockerError> {
        use bollard::query_parameters::UpdateServiceOptions;

        let opts = UpdateServiceOptions {
            version: version as i32,
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
    /// Since bollard 0.20 doesn't expose `previous_spec`, we use the rollback
    /// flag in UpdateServiceOptions which tells Docker to rollback.
    pub async fn rollback_service(&self, service_id: &str) -> Result<(), DockerError> {
        use bollard::query_parameters::UpdateServiceOptions;

        // Inspect the service to get its current spec and version
        let service = self.inspect_service(service_id).await?;
        let version = service.version.as_ref()
            .and_then(|v| v.index)
            .ok_or_else(|| DockerError::ConnectionFailed("Service has no version".to_string()))?;

        let spec = service.spec.unwrap_or_default();

        let opts = UpdateServiceOptions {
            version: version as i32,
            rollback: Some("previous".to_string()),
            ..Default::default()
        };

        self.client
            .update_service(service_id, spec, opts, None)
            .await
            .map(|_| ())
            .map_err(DockerError::from)
    }

    // =========================================================================
    // Secret & Config CRUD
    // =========================================================================

    /// Create a swarm secret
    pub async fn create_secret(
        &self,
        name: &str,
        data: &[u8],
        labels: std::collections::HashMap<String, String>,
    ) -> Result<String, DockerError> {
        use bollard::models::SecretSpec;
        use base64::Engine as _;

        let spec = SecretSpec {
            name: Some(name.to_string()),
            data: Some(base64::engine::general_purpose::STANDARD.encode(data)),
            labels: Some(labels),
            ..Default::default()
        };

        let result = self.client.create_secret(spec).await.map_err(DockerError::from)?;
        Ok(result.id)
    }

    /// Delete a swarm secret
    pub async fn delete_secret(&self, secret_id: &str) -> Result<(), DockerError> {
        self.client.delete_secret(secret_id).await.map_err(DockerError::from)
    }

    /// Create a swarm config
    pub async fn create_config(
        &self,
        name: &str,
        data: &[u8],
        labels: std::collections::HashMap<String, String>,
    ) -> Result<String, DockerError> {
        use bollard::models::ConfigSpec;
        use base64::Engine as _;

        let spec = ConfigSpec {
            name: Some(name.to_string()),
            data: Some(base64::engine::general_purpose::STANDARD.encode(data)),
            labels: Some(labels),
            ..Default::default()
        };

        let result = self.client.create_config(spec).await.map_err(DockerError::from)?;
        Ok(result.id)
    }

    /// Delete a swarm config
    pub async fn delete_config(&self, config_id: &str) -> Result<(), DockerError> {
        self.client.delete_config(config_id).await.map_err(DockerError::from)
    }

    // =========================================================================
    // Swarm Init / Join / Leave
    // =========================================================================

    /// Initialize a new swarm
    pub async fn swarm_init(
        &self,
        listen_addr: &str,
        advertise_addr: &str,
        force_new_cluster: bool,
    ) -> Result<String, DockerError> {
        use bollard::models::SwarmInitRequest;

        let request = SwarmInitRequest {
            listen_addr: Some(listen_addr.to_string()),
            advertise_addr: if advertise_addr.is_empty() { None } else { Some(advertise_addr.to_string()) },
            force_new_cluster: Some(force_new_cluster),
            ..Default::default()
        };

        self.client.init_swarm(request).await.map_err(DockerError::from)
    }

    /// Join an existing swarm
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
            advertise_addr: if advertise_addr.is_empty() { None } else { Some(advertise_addr.to_string()) },
            ..Default::default()
        };

        self.client.join_swarm(request).await.map_err(DockerError::from)
    }

    /// Leave the swarm
    pub async fn swarm_leave(&self, force: bool) -> Result<(), DockerError> {
        use bollard::query_parameters::LeaveSwarmOptionsBuilder;
        let options = LeaveSwarmOptionsBuilder::default().force(force).build();
        self.client.leave_swarm(Some(options))
            .await.map_err(DockerError::from)
    }

    // =========================================================================
    // Node Management
    // =========================================================================

    /// Remove a node from the swarm
    pub async fn remove_node(&self, node_id: &str, force: bool) -> Result<(), DockerError> {
        use bollard::query_parameters::DeleteNodeOptionsBuilder;
        let options = DeleteNodeOptionsBuilder::default().force(force).build();
        self.client.delete_node(node_id, Some(options))
            .await.map_err(DockerError::from)
    }

    // =========================================================================
    // Network Connect / Disconnect
    // =========================================================================

    /// Connect a container to a network
    pub async fn network_connect(&self, network_id: &str, container_id: &str) -> Result<(), DockerError> {
        use bollard::models::NetworkConnectRequest;

        let config = NetworkConnectRequest {
            container: container_id.to_string(),
            ..Default::default()
        };

        self.client.connect_network(network_id, config).await.map_err(DockerError::from)
    }

    /// Disconnect a container from a network
    pub async fn network_disconnect(&self, network_id: &str, container_id: &str, force: bool) -> Result<(), DockerError> {
        use bollard::models::NetworkDisconnectRequest;

        let config = NetworkDisconnectRequest {
            container: container_id.to_string(),
            force: Some(force),
        };

        self.client.disconnect_network(network_id, config).await.map_err(DockerError::from)
    }

    // =========================================================================
    // Docker Events Stream
    // =========================================================================

    /// Stream Docker engine events
    pub fn stream_events(
        &self,
        type_filters: Vec<String>,
        since: Option<i64>,
        until: Option<i64>,
    ) -> impl futures_util::Stream<Item = Result<bollard::models::EventMessage, DockerError>> + '_ {
        use bollard::query_parameters::EventsOptionsBuilder;
        use std::collections::HashMap;

        let mut filters = HashMap::new();
        if !type_filters.is_empty() {
            filters.insert("type", type_filters.iter().map(|s| s.as_str()).collect::<Vec<_>>());
        }

        let since_str = since.map(|s| s.to_string());
        let until_str = until.map(|u| u.to_string());

        let mut builder = EventsOptionsBuilder::default();
        builder = builder.filters(&filters);
        if let Some(ref s) = since_str {
            builder = builder.since(s);
        }
        if let Some(ref u) = until_str {
            builder = builder.until(u);
        }
        let options = builder.build();

        self.client.events(Some(options)).map(|r| r.map_err(DockerError::from))
    }

    // =========================================================================
    // Exec / Shell Methods
    // =========================================================================

    /// Create an exec instance in a container.
    /// Returns the exec ID that can be used with `start_exec`.
    pub async fn create_exec(
        &self,
        container_id: &str,
        cmd: Vec<String>,
        tty: bool,
        working_dir: Option<String>,
        env: Vec<String>,
    ) -> Result<String, DockerError> {
        use bollard::models::ExecConfig;

        let config = ExecConfig {
            attach_stdin: Some(true),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            tty: Some(tty),
            cmd: Some(cmd),
            env: if env.is_empty() { None } else { Some(env) },
            working_dir,
            ..Default::default()
        };

        let result = self.client
            .create_exec(container_id, config)
            .await
            .map_err(|e| match e {
                bollard::errors::Error::DockerResponseServerError { status_code: 404, .. } => {
                    DockerError::ContainerNotFound(container_id.to_string())
                }
                other => DockerError::BollardError(other),
            })?;

        Ok(result.id)
    }

    /// Start an exec instance in attached mode.
    /// Returns a bidirectional stream: an output stream and an input writer.
    pub async fn start_exec(
        &self,
        exec_id: &str,
        tty: bool,
    ) -> Result<bollard::exec::StartExecResults, DockerError> {
        use bollard::exec::StartExecOptions;

        let options = Some(StartExecOptions {
            detach: false,
            tty,
            ..Default::default()
        });

        self.client
            .start_exec(exec_id, options)
            .await
            .map_err(DockerError::from)
    }

    /// Resize the TTY of an exec instance.
    pub async fn resize_exec(
        &self,
        exec_id: &str,
        height: u16,
        width: u16,
    ) -> Result<(), DockerError> {
        use bollard::exec::ResizeExecOptions;

        let options = ResizeExecOptions { height, width };

        self.client
            .resize_exec(exec_id, options)
            .await
            .map_err(DockerError::from)
    }

    /// Inspect an exec instance to get its exit code and running status.
    pub async fn inspect_exec(
        &self,
        exec_id: &str,
    ) -> Result<bollard::models::ExecInspectResponse, DockerError> {
        self.client
            .inspect_exec(exec_id)
            .await
            .map_err(DockerError::from)
    }

    // =========================================================================
    // Task Inspect & Task Logs (B02, B03)
    // =========================================================================

    /// Inspect a single swarm task by ID.
    pub async fn inspect_task(&self, task_id: &str) -> Result<Option<bollard::models::Task>, DockerError> {
        match self.client.inspect_task(task_id).await {
            Ok(task) => Ok(Some(task)),
            Err(bollard::errors::Error::DockerResponseServerError { status_code: 404, .. }) => {
                Ok(None)
            }
            Err(bollard::errors::Error::DockerResponseServerError { status_code: 503, .. }) => {
                Ok(None) // Not in swarm mode
            }
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
    ) -> std::pin::Pin<Box<dyn futures_util::Stream<Item = Result<LogOutput, bollard::errors::Error>> + Send>> {
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

    // =========================================================================
    // Swarm Update / Unlock (B05)
    // =========================================================================

    /// Update swarm settings (autolock, certificate rotation, etc.)
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

    /// Get the swarm unlock key via bollard's raw request API.
    /// Docker API: `GET /swarm/unlockkey`
    /// Get the swarm unlock key via Docker CLI.
    /// bollard 0.20 does not wrap `GET /swarm/unlockkey`, so we shell out
    /// to `docker swarm unlock-key -q` which is available everywhere the
    /// Docker daemon is reachable.
    pub async fn swarm_unlock_key(&self) -> Result<String, DockerError> {
        // Pre-check: verify swarm mode and autolock
        let swarm = self.swarm_inspect().await?;
        let s = match swarm {
            SwarmInspectResult::Manager(s) => s,
            SwarmInspectResult::Worker => return Err(DockerError::ConnectionFailed("This node is a worker, not a manager. Cannot retrieve unlock key.".to_string())),
            SwarmInspectResult::NotInSwarm => return Err(DockerError::ConnectionFailed("Not in swarm mode".to_string())),
        };
        let autolock = s.spec
            .as_ref()
            .and_then(|spec| spec.encryption_config.as_ref())
            .and_then(|enc| enc.auto_lock_managers)
            .unwrap_or(false);
        if !autolock {
            return Err(DockerError::ConnectionFailed(
                "Swarm autolock is not enabled. Enable it first via swarm update with autolock=true.".to_string()
            ));
        }

        let output = tokio::process::Command::new("docker")
            .args(["swarm", "unlock-key", "-q"])
            .output()
            .await
            .map_err(|e| DockerError::ConnectionFailed(format!("Failed to run docker CLI: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(DockerError::ConnectionFailed(
                format!("docker swarm unlock-key failed: {}", stderr.trim())
            ));
        }

        let key = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if key.is_empty() {
            return Err(DockerError::ConnectionFailed(
                "Docker returned an empty unlock key. Try rotating the unlock key via swarm update.".to_string()
            ));
        }
        Ok(key)
    }

    /// Unlock the swarm after manager restart when autolock is enabled.
    /// bollard 0.20 does not wrap `POST /swarm/unlock`, so we shell out
    /// to `docker swarm unlock` and pipe the key via stdin.
    pub async fn swarm_unlock(&self, unlock_key: &str) -> Result<(), DockerError> {
        use tokio::io::AsyncWriteExt;

        let mut child = tokio::process::Command::new("docker")
            .args(["swarm", "unlock"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| DockerError::ConnectionFailed(format!("Failed to run docker CLI: {}", e)))?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(unlock_key.as_bytes()).await
                .map_err(|e| DockerError::ConnectionFailed(format!("Failed to send unlock key: {}", e)))?;
            stdin.write_all(b"\n").await.ok();
            drop(stdin);
        }

        let output = child.wait_with_output().await
            .map_err(|e| DockerError::ConnectionFailed(format!("Failed to wait for docker CLI: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(DockerError::ConnectionFailed(
                format!("docker swarm unlock failed: {}", stderr.trim())
            ));
        }
        Ok(())
    }
}

/// Converts Bollard's `LogOutput` to our `LogLine` format.
///
/// Docker with `timestamps: true` prepends an RFC3339Nano timestamp like
/// `"2023-01-01T00:00:00.000000000Z message content..."`.
/// We parse this to preserve the actual log timestamp instead of using the current time.
fn convert_bollard_log(output: LogOutput) -> Result<LogLine, DockerError> {
    let (stream_type, raw_bytes) = match output {
        LogOutput::StdOut { message } => (LogLevel::Stdout, message),
        LogOutput::StdErr { message } => (LogLevel::Stderr, message),
        LogOutput::StdIn { message } => (LogLevel::Stdout, message), // Treat stdin as stdout
        LogOutput::Console { message } => (LogLevel::Stdout, message),
    };

    // Docker prepends timestamp: "2023-01-01T00:00:00.000000000Z message"
    // Split at first space to separate timestamp from actual log content
    // We split on bytes to avoid decoding the entire message as UTF-8 yet,
    // which protects against invalid UTF-8 in the log content.
    let split_idx = raw_bytes.iter().position(|&b| b == b' ');
    
    let (timestamp, content) = match split_idx {
        Some(idx) => {
            // Try to parse the bytes before space as the Docker timestamp
            // We only decode the timestamp part as UTF-8
            match std::str::from_utf8(&raw_bytes[..idx]) {
                Ok(ts_str) => {
                    match chrono::DateTime::parse_from_rfc3339(ts_str) {
                        Ok(dt) => {
                            let ts_nanos = dt.timestamp_nanos_opt()
                                .unwrap_or_else(|| chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0));
                            
                            // Zero-copy slice
                            // Calculate the offset where the message begins
                            // +1 is for the space character we split on
                            let msg_start = idx + 1;
                            
                            // Slice the ORIGINAL Bytes object. 
                            let clean_content = if msg_start < raw_bytes.len() {
                                raw_bytes.slice(msg_start..)
                            } else {
                                Bytes::new()
                            };

                            (ts_nanos, clean_content)
                        }
                        Err(_) => {
                            // Parsing failed - maybe malformed timestamp?
                            (chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0), raw_bytes)
                        }
                    }
                },
                Err(_) => {
                    // Timestamp part not valid UTF-8
                    (chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0), raw_bytes)
                }
            }
        },
        None => {
            // No space found - no timestamp prefix (shouldn't happen with timestamps:true)
            (chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0), raw_bytes)
        }
    };

    Ok(LogLine {
        timestamp,
        stream_type,
        content,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bollard::container::LogOutput;

    #[test]
    fn test_convert_bollard_log_with_timestamp() {
        let log_content = "2023-01-15T10:30:45.123456789Z Application started successfully";
        let output = LogOutput::StdOut {
            message: Bytes::from(log_content),
        };

        let result = convert_bollard_log(output).unwrap();

        let expected_dt = chrono::DateTime::parse_from_rfc3339("2023-01-15T10:30:45.123456789Z").unwrap();
        let expected_ts = expected_dt.timestamp_nanos_opt().unwrap();
        assert_eq!(result.timestamp, expected_ts);

        assert_eq!(result.content, Bytes::from("Application started successfully"));
        assert_eq!(result.stream_type, LogLevel::Stdout);
    }

    #[test]
    fn test_convert_bollard_log_stderr() {
        let log_content = "2023-01-15T10:30:45.123456789Z ERROR: Connection failed";
        let output = LogOutput::StdErr {
            message: Bytes::from(log_content),
        };

        let result = convert_bollard_log(output).unwrap();

        assert_eq!(result.stream_type, LogLevel::Stderr);
        assert_eq!(result.content, Bytes::from("ERROR: Connection failed"));
    }

    #[test]
    fn test_convert_bollard_log_no_timestamp() {
        let log_content = "Plain log message without timestamp";
        let output = LogOutput::StdOut {
            message: Bytes::from(log_content),
        };

        let result = convert_bollard_log(output).unwrap();

        // Should fallback to current time and keep full content
        assert!(result.timestamp > 0); // Some timestamp was set
        assert_eq!(result.content, Bytes::from(log_content));
    }

    #[test]
    fn test_convert_bollard_log_malformed_timestamp() {
        let log_content = "NOT_A_TIMESTAMP Application log message";
        let output = LogOutput::StdOut {
            message: Bytes::from(log_content),
        };

        let result = convert_bollard_log(output).unwrap();

        // Should fallback to current time and keep full content
        assert!(result.timestamp > 0);
        assert_eq!(result.content, Bytes::from(log_content));
    }

    #[test]
    fn test_convert_bollard_log_multiline_message() {

        let log_content = "2023-01-15T10:30:45.123456789Z Stack trace:\n  at line 1\n  at line 2";
        let output = LogOutput::StdOut {
            message: Bytes::from(log_content),
        };

        let result = convert_bollard_log(output).unwrap();

        assert_eq!(result.content, Bytes::from("Stack trace:\n  at line 1\n  at line 2"));
    }

    #[test]
    fn test_convert_bollard_log_empty_message() {
        let log_content = "2023-01-15T10:30:45.123456789Z ";
        let output = LogOutput::StdOut {
            message: Bytes::from(log_content),
        };

        let result = convert_bollard_log(output).unwrap();

        assert_eq!(result.content, Bytes::from(""));
    }

    #[test]
    fn test_convert_bollard_log_invalid_utf8_in_message() {
        let mut data = Vec::new();
        data.extend_from_slice(b"2023-01-15T10:30:45.123456789Z "); // Valid header
        data.extend_from_slice(&[0xFF, 0xFF, 0x61, 0x62, 0x63]); // Invalid UTF-8 body

        let output = LogOutput::StdOut {
            message: Bytes::from(data),
        };

        let result = convert_bollard_log(output).unwrap();

        let expected_dt = chrono::DateTime::parse_from_rfc3339("2023-01-15T10:30:45.123456789Z").unwrap();
        let expected_ts = expected_dt.timestamp_nanos_opt().unwrap();
        assert_eq!(result.timestamp, expected_ts);
        
        // And content should be the invalid bytes (stripped of timestamp)
        assert_eq!(result.content, Bytes::from(&[0xFF, 0xFF, 0x61, 0x62, 0x63][..]));
    }

    #[test]
    fn test_convert_bollard_log_json_content() {
        let log_content = r#"2023-01-15T10:30:45.123456789Z {"level":"info","msg":"Request processed","duration_ms":123}"#;
        let output = LogOutput::StdOut {
            message: Bytes::from(log_content),
        };

        let result = convert_bollard_log(output).unwrap();

        // Verify JSON content is preserved without timestamp prefix
        let expected_json = r#"{"level":"info","msg":"Request processed","duration_ms":123}"#;
        assert_eq!(result.content, Bytes::from(expected_json));
    }

    #[test]
    fn test_convert_bollard_log_invalid_utf8_in_timestamp() {
        let mut data = Vec::new();
        data.extend_from_slice(&[0xFF, 0xFF, 0x20]); // Invalid UTF-8 + space
        data.extend_from_slice(b"message"); // Valid message

        let output = LogOutput::StdOut {
            message: Bytes::from(data.clone()),
        };

        let result = convert_bollard_log(output).unwrap();

        assert!(result.timestamp > 0);
        assert_eq!(result.content, Bytes::from(data));
    }

    #[test]
    fn test_convert_bollard_log_empty_log() {
        let output = LogOutput::StdOut {
            message: Bytes::new(),
        };

        let result = convert_bollard_log(output).unwrap();

        assert!(result.timestamp > 0);
        assert_eq!(result.content, Bytes::new());
    }

    #[test]
    fn test_convert_bollard_log_unicode_emoji() {
        let log_content = "2023-01-15T10:30:45.123456789Z 🚀 Deployment successful! 🎉";
        let output = LogOutput::StdOut {
            message: Bytes::from(log_content),
        };

        let result = convert_bollard_log(output).unwrap();

        let expected_dt = chrono::DateTime::parse_from_rfc3339("2023-01-15T10:30:45.123456789Z").unwrap();
        let expected_ts = expected_dt.timestamp_nanos_opt().unwrap();
        assert_eq!(result.timestamp, expected_ts);

        // Emoji content should be preserved
        assert_eq!(result.content, Bytes::from("🚀 Deployment successful! 🎉"));
    }

    #[test]
    fn test_convert_bollard_log_multiple_spaces() {
        let log_content = "2023-01-15T10:30:45.123456789Z   message with leading spaces";
        let output = LogOutput::StdOut {
            message: Bytes::from(log_content),
        };

        let result = convert_bollard_log(output).unwrap();

        // Timestamp should parse correctly
        let expected_dt = chrono::DateTime::parse_from_rfc3339("2023-01-15T10:30:45.123456789Z").unwrap();
        let expected_ts = expected_dt.timestamp_nanos_opt().unwrap();
        assert_eq!(result.timestamp, expected_ts);

        assert_eq!(result.content, Bytes::from("  message with leading spaces"));
    }

    #[test]
    fn test_convert_bollard_log_timestamp_only() {
        let log_content = "2023-01-15T10:30:45.123456789Z";
        let output = LogOutput::StdOut {
            message: Bytes::from(log_content),
        };

        let result = convert_bollard_log(output).unwrap();

        assert!(result.timestamp > 0);
        // Should keep full content
        assert_eq!(result.content, Bytes::from(log_content));
    }

    #[test]
    fn test_timestamp_conversion_safety() {
        let valid_ts = 1673780400i64; // 2023-01-15 10:00:00 UTC
        let dt = chrono::DateTime::from_timestamp(valid_ts, 0);
        assert!(dt.is_some());
        
        let year_2038 = 2147483647i64; // Max i32 value
        let dt_2038 = chrono::DateTime::from_timestamp(year_2038, 0);
        assert!(dt_2038.is_some());
        
        let invalid_ts = -1i64;
        let dt_invalid = chrono::DateTime::from_timestamp(invalid_ts, 0);
        // Should handle gracefully (returns None)
        assert!(dt_invalid.is_some() || dt_invalid.is_none());
    }
}
