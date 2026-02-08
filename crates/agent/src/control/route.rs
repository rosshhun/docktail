//! Route — ControlService gRPC handler.

use tonic::{Request, Response, Status};
use tracing::info;
use std::pin::Pin;
use tokio_stream::Stream;

use crate::state::SharedState;
use crate::control::map::map_docker_error;

use crate::proto::{
    control_service_server::ControlService,
    ContainerControlRequest, ContainerControlResponse,
    ContainerRemoveRequest,
    PullImageRequest, PullImageResponse,
    RemoveImageRequest, RemoveImageResponse,
    CreateVolumeRequest, CreateVolumeResponse,
    RemoveVolumeRequest, RemoveVolumeResponse,
    CreateNetworkRequest, CreateNetworkResponse,
    RemoveNetworkRequest, RemoveNetworkResponse,
    DockerEventsRequest, DockerEvent,
};

/// Implementation of the ControlService gRPC service.
/// Provides container lifecycle management: start, stop, restart, pause, unpause, remove.
pub struct ControlServiceImpl {
    state: SharedState,
}

impl ControlServiceImpl {
    pub fn new(state: SharedState) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl ControlService for ControlServiceImpl {
    type StreamDockerEventsStream = Pin<Box<dyn Stream<Item = Result<DockerEvent, Status>> + Send>>;

    async fn start_container(
        &self,
        request: Request<ContainerControlRequest>,
    ) -> Result<Response<ContainerControlResponse>, Status> {
        let req = request.into_inner();
        let container_id = &req.container_id;

        info!(container_id = %container_id, "Starting container");

        self.state.docker.start_container(container_id)
            .await
            .map_err(map_docker_error)?;

        info!(container_id = %container_id, "Container started successfully");

        Ok(Response::new(ContainerControlResponse {
            success: true,
            message: "Container started successfully".to_string(),
            container_id: container_id.to_string(),
            new_state: "running".to_string(),
        }))
    }

    async fn stop_container(
        &self,
        request: Request<ContainerControlRequest>,
    ) -> Result<Response<ContainerControlResponse>, Status> {
        let req = request.into_inner();
        let container_id = &req.container_id;
        let timeout_secs = req.timeout;

        info!(container_id = %container_id, timeout = ?timeout_secs, "Stopping container");

        self.state.docker.stop_container(container_id, timeout_secs)
            .await
            .map_err(map_docker_error)?;

        info!(container_id = %container_id, "Container stopped successfully");

        Ok(Response::new(ContainerControlResponse {
            success: true,
            message: "Container stopped successfully".to_string(),
            container_id: container_id.to_string(),
            new_state: "exited".to_string(),
        }))
    }

    async fn restart_container(
        &self,
        request: Request<ContainerControlRequest>,
    ) -> Result<Response<ContainerControlResponse>, Status> {
        let req = request.into_inner();
        let container_id = &req.container_id;
        let timeout_secs = req.timeout;

        info!(container_id = %container_id, timeout = ?timeout_secs, "Restarting container");

        self.state.docker.restart_container(container_id, timeout_secs)
            .await
            .map_err(map_docker_error)?;

        info!(container_id = %container_id, "Container restarted successfully");

        Ok(Response::new(ContainerControlResponse {
            success: true,
            message: "Container restarted successfully".to_string(),
            container_id: container_id.to_string(),
            new_state: "running".to_string(),
        }))
    }

    async fn pause_container(
        &self,
        request: Request<ContainerControlRequest>,
    ) -> Result<Response<ContainerControlResponse>, Status> {
        let req = request.into_inner();
        let container_id = &req.container_id;

        info!(container_id = %container_id, "Pausing container");

        self.state.docker.pause_container(container_id)
            .await
            .map_err(map_docker_error)?;

        info!(container_id = %container_id, "Container paused successfully");

        Ok(Response::new(ContainerControlResponse {
            success: true,
            message: "Container paused successfully".to_string(),
            container_id: container_id.to_string(),
            new_state: "paused".to_string(),
        }))
    }

    async fn unpause_container(
        &self,
        request: Request<ContainerControlRequest>,
    ) -> Result<Response<ContainerControlResponse>, Status> {
        let req = request.into_inner();
        let container_id = &req.container_id;

        info!(container_id = %container_id, "Unpausing container");

        self.state.docker.unpause_container(container_id)
            .await
            .map_err(map_docker_error)?;

        info!(container_id = %container_id, "Container unpaused successfully");

        Ok(Response::new(ContainerControlResponse {
            success: true,
            message: "Container unpaused successfully".to_string(),
            container_id: container_id.to_string(),
            new_state: "running".to_string(),
        }))
    }

    async fn remove_container(
        &self,
        request: Request<ContainerRemoveRequest>,
    ) -> Result<Response<ContainerControlResponse>, Status> {
        let req = request.into_inner();
        let container_id = &req.container_id;
        let force = req.force;
        let remove_volumes = req.remove_volumes;

        info!(
            container_id = %container_id,
            force = force,
            remove_volumes = remove_volumes,
            "Removing container"
        );

        self.state.docker.remove_container(container_id, force, remove_volumes)
            .await
            .map_err(map_docker_error)?;

        info!(container_id = %container_id, "Container removed successfully");

        Ok(Response::new(ContainerControlResponse {
            success: true,
            message: "Container removed successfully".to_string(),
            container_id: container_id.to_string(),
            new_state: "removed".to_string(),
        }))
    }

    // =========================================================================
    // Image Management
    // =========================================================================

    async fn pull_image(
        &self,
        request: Request<PullImageRequest>,
    ) -> Result<Response<PullImageResponse>, Status> {
        let req = request.into_inner();
        let tag = if req.tag.is_empty() { "latest" } else { &req.tag };
        let registry_auth = if req.registry_auth.is_empty() {
            None
        } else {
            Some(req.registry_auth.as_str())
        };

        info!(image = %req.image, tag = %tag, has_auth = registry_auth.is_some(), "Pulling image");

        self.state.docker.pull_image(&req.image, tag, registry_auth)
            .await
            .map_err(map_docker_error)?;

        info!(image = %req.image, tag = %tag, "Image pulled successfully");

        Ok(Response::new(PullImageResponse {
            success: true,
            message: format!("Successfully pulled {}:{}", req.image, tag),
            image_id: format!("{}:{}", req.image, tag),
        }))
    }

    async fn remove_image(
        &self,
        request: Request<RemoveImageRequest>,
    ) -> Result<Response<RemoveImageResponse>, Status> {
        let req = request.into_inner();

        info!(image_id = %req.image_id, force = req.force, no_prune = req.no_prune, "Removing image");

        self.state.docker.remove_image(&req.image_id, req.force, req.no_prune)
            .await
            .map_err(map_docker_error)?;

        info!(image_id = %req.image_id, "Image removed successfully");

        Ok(Response::new(RemoveImageResponse {
            success: true,
            message: format!("Image {} removed successfully", req.image_id),
        }))
    }

    // =========================================================================
    // Volume Management
    // =========================================================================

    async fn create_volume(
        &self,
        request: Request<CreateVolumeRequest>,
    ) -> Result<Response<CreateVolumeResponse>, Status> {
        let req = request.into_inner();
        let driver = if req.driver.is_empty() { None } else { Some(req.driver.as_str()) };

        info!(name = %req.name, "Creating volume");

        let volume = self.state.docker.create_volume(&req.name, driver, req.labels, req.driver_opts)
            .await
            .map_err(map_docker_error)?;

        info!(name = %req.name, "Volume created successfully");

        Ok(Response::new(CreateVolumeResponse {
            success: true,
            message: format!("Volume {} created successfully", req.name),
            name: volume.name,
        }))
    }

    async fn remove_volume(
        &self,
        request: Request<RemoveVolumeRequest>,
    ) -> Result<Response<RemoveVolumeResponse>, Status> {
        let req = request.into_inner();

        info!(name = %req.name, force = req.force, "Removing volume");

        self.state.docker.remove_volume(&req.name, req.force)
            .await
            .map_err(map_docker_error)?;

        info!(name = %req.name, "Volume removed successfully");

        Ok(Response::new(RemoveVolumeResponse {
            success: true,
            message: format!("Volume {} removed successfully", req.name),
        }))
    }

    // =========================================================================
    // Network Management
    // =========================================================================

    async fn create_network(
        &self,
        request: Request<CreateNetworkRequest>,
    ) -> Result<Response<CreateNetworkResponse>, Status> {
        let req = request.into_inner();
        let driver = if req.driver.is_empty() { None } else { Some(req.driver.as_str()) };

        info!(name = %req.name, "Creating network");

        // Build IPAM config if provided
        let ipam = if !req.ipam_configs.is_empty() || !req.ipam_driver.is_empty() {
            Some(bollard::models::Ipam {
                driver: if req.ipam_driver.is_empty() { None } else { Some(req.ipam_driver) },
                config: if req.ipam_configs.is_empty() {
                    None
                } else {
                    Some(req.ipam_configs.into_iter().map(|c| {
                        bollard::models::IpamConfig {
                            subnet: if c.subnet.is_empty() { None } else { Some(c.subnet) },
                            gateway: if c.gateway.is_empty() { None } else { Some(c.gateway) },
                            ip_range: if c.ip_range.is_empty() { None } else { Some(c.ip_range) },
                            ..Default::default()
                        }
                    }).collect())
                },
                ..Default::default()
            })
        } else {
            None
        };

        let result = self.state.docker.create_network(
            &req.name, driver, req.labels, req.internal, req.attachable,
            req.enable_ipv6, req.options, ipam,
        )
            .await
            .map_err(map_docker_error)?;

        let network_id = result.id;
        info!(name = %req.name, network_id = %network_id, "Network created successfully");

        Ok(Response::new(CreateNetworkResponse {
            success: true,
            message: format!("Network {} created successfully", req.name),
            network_id,
        }))
    }

    async fn remove_network(
        &self,
        request: Request<RemoveNetworkRequest>,
    ) -> Result<Response<RemoveNetworkResponse>, Status> {
        let req = request.into_inner();

        info!(network_id = %req.network_id, "Removing network");

        self.state.docker.remove_network(&req.network_id)
            .await
            .map_err(map_docker_error)?;

        info!(network_id = %req.network_id, "Network removed successfully");

        Ok(Response::new(RemoveNetworkResponse {
            success: true,
            message: format!("Network {} removed successfully", req.network_id),
        }))
    }

    // =========================================================================
    // Docker Events Stream (B01)
    // =========================================================================

    async fn stream_docker_events(
        &self,
        request: Request<DockerEventsRequest>,
    ) -> Result<Response<Self::StreamDockerEventsStream>, Status> {
        let req = request.into_inner();

        info!(filters = ?req.type_filters, "Starting Docker events stream");

        let docker = self.state.docker.clone();

        let stream = async_stream::stream! {
            let event_stream = docker.stream_events(
                req.type_filters,
                req.since,
                req.until,
            );
            tokio::pin!(event_stream);

            while let Some(result) = futures_util::StreamExt::next(&mut event_stream).await {
                match result {
                    Ok(event) => {
                        let actor = event.actor.as_ref();
                        yield Ok(DockerEvent {
                            event_type: event.typ.map(|t| format!("{:?}", t).to_lowercase()).unwrap_or_default(),
                            action: event.action.unwrap_or_default(),
                            actor_id: actor.and_then(|a| a.id.clone()).unwrap_or_default(),
                            actor_attributes: actor
                                .and_then(|a| a.attributes.clone())
                                .unwrap_or_default(),
                            timestamp: event.time.unwrap_or(0),
                            scope: event.scope.map(|s| format!("{:?}", s).to_lowercase()).unwrap_or_default(),
                        });
                    }
                    Err(e) => {
                        yield Err(Status::internal(format!("Docker events error: {}", e)));
                        break;
                    }
                }
            }
        };

        Ok(Response::new(Box::pin(stream)))
    }
}
