//! Route — InventoryService gRPC handler.

use tonic::{Request, Response, Status};

use crate::docker::client::DockerError;
use crate::state::SharedState;
use crate::container::map;

use crate::proto::{
    inventory_service_server::InventoryService,
    ContainerListRequest, ContainerListResponse,
    ContainerInspectRequest, ContainerInspectResponse,
    ContainerStateFilter,
};

/// Implementation of the InventoryService gRPC service
/// Handles container listing and inspection with caching and filtering
pub struct InventoryServiceImpl {
    state: SharedState,
}

impl InventoryServiceImpl {
    pub fn new(state: SharedState) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl InventoryService for InventoryServiceImpl {
    async fn list_containers(
        &self,
        request: Request<ContainerListRequest>,
    ) -> Result<Response<ContainerListResponse>, Status> {
        let req = request.into_inner();

        let mut containers: Vec<_> = self.state.inventory
            .iter()
            .map(|entry| entry.value().clone())
            .collect();

        if let Some(state_filter) = req.state_filter {
            containers = map::apply_state_filter(containers, state_filter);
        }

        let has_explicit_state_filter = req.state_filter.map_or(false, |sf| {
             let enum_val = ContainerStateFilter::try_from(sf).unwrap_or(ContainerStateFilter::Unspecified);
             !matches!(enum_val, ContainerStateFilter::Unspecified | ContainerStateFilter::All)
        });

        if !req.include_stopped && !has_explicit_state_filter {
            containers.retain(|c| c.state.eq_ignore_ascii_case("running"));
        }

        let total_count = containers.len() as u32;

        if let Some(limit) = req.limit {
            let limit = limit as usize;
            if limit < containers.len() {
                containers.truncate(limit);
            }
        }

        let proto_containers = containers
            .into_iter()
            .map(map::convert_container_info)
            .collect();

        Ok(Response::new(ContainerListResponse {
            containers: proto_containers,
            total_count,
        }))
    }

    async fn inspect_container(
        &self,
        request: Request<ContainerInspectRequest>,
    ) -> Result<Response<ContainerInspectResponse>, Status> {
        let req = request.into_inner();

        let raw_inspect = self.state.docker
            .inspect_container_raw(&req.container_id)
            .await
            .map_err(|e| match e {
                DockerError::ContainerNotFound(msg) => Status::not_found(msg),
                _ => Status::internal(format!("Docker inspect raw failed: {}", e)),
            })?;

        let info = crate::docker::inventory::ContainerInfo::from(raw_inspect.clone());

        let details = map::extract_container_details(&raw_inspect);

        // Update cache with the fresh truth
        self.state.inventory.insert(info.id.clone(), info.clone());

        Ok(Response::new(ContainerInspectResponse {
            info: Some(map::convert_container_info(info)),
            details, 
        }))
    }
}
