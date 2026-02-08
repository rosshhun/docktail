use async_graphql::{Context, InputObject, SimpleObject};
use crate::state::AppState;
use crate::error::ApiError;

/// GraphQL Mutation root — container lifecycle + resource management operations
pub struct MutationRoot;

// ============================================================================
// Input Types
// ============================================================================

/// Input for container start/stop/restart/pause/unpause operations
#[derive(InputObject)]
pub struct ContainerActionInput {
    /// Container ID (full or short hash)
    pub container_id: String,
    /// Agent ID where the container lives
    pub agent_id: String,
    /// Optional timeout in seconds for stop/restart operations
    pub timeout: Option<i32>,
}

/// Input for container removal
#[derive(InputObject)]
pub struct ContainerRemoveInput {
    /// Container ID (full or short hash)
    pub container_id: String,
    /// Agent ID where the container lives
    pub agent_id: String,
    /// Force removal even if the container is running
    #[graphql(default = false)]
    pub force: bool,
    /// Remove associated anonymous volumes
    #[graphql(default = false)]
    pub remove_volumes: bool,
}

/// Input for pulling an image
#[derive(InputObject)]
pub struct ImagePullInput {
    /// Image name (e.g., "nginx", "ubuntu", "myregistry/myimage")
    pub image: String,
    /// Tag to pull (default: "latest")
    #[graphql(default_with = "\"latest\".to_string()")]
    pub tag: String,
    /// Agent ID where to pull the image
    pub agent_id: String,
}

/// Input for removing an image
#[derive(InputObject)]
pub struct ImageRemoveInput {
    /// Image ID or tag
    pub image_id: String,
    /// Agent ID where the image lives
    pub agent_id: String,
    /// Force removal
    #[graphql(default = false)]
    pub force: bool,
}

/// Input for creating a volume
#[derive(InputObject)]
pub struct VolumeCreateInput {
    /// Volume name
    pub name: String,
    /// Agent ID where to create the volume
    pub agent_id: String,
    /// Volume driver (default: "local")
    pub driver: Option<String>,
    /// Labels for the volume
    pub labels: Option<Vec<LabelInput>>,
}

/// Input for removing a volume
#[derive(InputObject)]
pub struct VolumeRemoveInput {
    /// Volume name
    pub name: String,
    /// Agent ID where the volume lives
    pub agent_id: String,
    /// Force removal
    #[graphql(default = false)]
    pub force: bool,
}

/// Input for creating a network
#[derive(InputObject)]
pub struct NetworkCreateInput {
    /// Network name
    pub name: String,
    /// Agent ID where to create the network
    pub agent_id: String,
    /// Network driver (default: "bridge")
    pub driver: Option<String>,
    /// Labels for the network
    pub labels: Option<Vec<LabelInput>>,
}

/// Input for removing a network
#[derive(InputObject)]
pub struct NetworkRemoveInput {
    /// Network ID or name
    pub network_id: String,
    /// Agent ID where the network lives
    pub agent_id: String,
}

/// Input for executing a command inside a container
#[derive(InputObject)]
pub struct ExecCommandInput {
    /// Container ID (full or short hash)
    pub container_id: String,
    /// Agent ID where the container lives
    pub agent_id: String,
    /// Command to execute (e.g., ["ls", "-la"])
    pub command: Vec<String>,
    /// Working directory inside the container
    pub working_dir: Option<String>,
    /// Environment variables to set
    pub env: Option<Vec<LabelInput>>,
    /// Timeout in seconds (0 = no timeout)
    pub timeout: Option<u32>,
}

/// Input for creating a swarm service (M6 Compose/Stack)
#[derive(InputObject)]
pub struct ServiceCreateInput {
    /// Service name
    pub name: String,
    /// Container image (e.g., "nginx:latest")
    pub image: String,
    /// Agent ID (must be a swarm manager)
    pub agent_id: String,
    /// Number of replicas (default: 1, ignored if global=true)
    #[graphql(default = 1)]
    pub replicas: u64,
    /// Use global mode (one task per node) instead of replicated
    #[graphql(default = false)]
    pub global: bool,
    /// Port mappings
    pub ports: Option<Vec<PortMappingInput>>,
    /// Environment variables
    pub env: Option<Vec<LabelInput>>,
    /// Labels (use key="com.docker.stack.namespace" for stack grouping)
    pub labels: Option<Vec<LabelInput>>,
    /// Networks to attach to
    pub networks: Option<Vec<String>>,
    /// Command override
    pub command: Option<Vec<String>>,
    /// Placement constraints (e.g., "node.role==manager")
    pub constraints: Option<Vec<String>>,
}

/// Port mapping input for service creation
#[derive(InputObject)]
pub struct PortMappingInput {
    /// Target port inside the container
    pub target_port: u32,
    /// Published port on the host (0 = auto-assign)
    #[graphql(default = 0)]
    pub published_port: u32,
    /// Protocol: "tcp" or "udp"
    #[graphql(default_with = "\"tcp\".to_string()")]
    pub protocol: String,
    /// Publish mode: "ingress" or "host"
    #[graphql(default_with = "\"ingress\".to_string()")]
    pub publish_mode: String,
}

/// Input for deleting a swarm service
#[derive(InputObject)]
pub struct ServiceDeleteInput {
    /// Service ID or name
    pub service_id: String,
    /// Agent ID (must be a swarm manager)
    pub agent_id: String,
}

/// Input for updating a swarm service
#[derive(InputObject)]
pub struct ServiceUpdateInput {
    /// Service ID or name
    pub service_id: String,
    /// Agent ID (must be a swarm manager)
    pub agent_id: String,
    /// New image (optional)
    pub image: Option<String>,
    /// New replica count (optional)
    pub replicas: Option<u64>,
    /// Force re-deployment
    #[graphql(default = false)]
    pub force: bool,
}

/// Input for deploying a stack (multiple services)
#[derive(InputObject)]
pub struct StackDeployInput {
    /// Stack name (used as com.docker.stack.namespace label)
    pub stack_name: String,
    /// Agent ID (must be a swarm manager)
    pub agent_id: String,
    /// Services to create in the stack
    pub services: Vec<StackServiceInput>,
}

/// Service definition within a stack
#[derive(InputObject)]
pub struct StackServiceInput {
    /// Service name (will be prefixed with stack name)
    pub name: String,
    /// Container image
    pub image: String,
    /// Replicas
    #[graphql(default = 1)]
    pub replicas: u64,
    /// Port mappings
    pub ports: Option<Vec<PortMappingInput>>,
    /// Environment variables
    pub env: Option<Vec<LabelInput>>,
    /// Networks
    pub networks: Option<Vec<String>>,
    /// Command override
    pub command: Option<Vec<String>>,
}

/// Key-value label input
#[derive(InputObject, Clone, Debug)]
pub struct LabelInput {
    pub key: String,
    pub value: String,
}

// ============================================================================
// Response Types
// ============================================================================

/// Response from container lifecycle operations
#[derive(SimpleObject)]
pub struct ContainerActionResult {
    /// Whether the operation succeeded
    pub success: bool,
    /// Human-readable message
    pub message: String,
    /// Container ID
    pub container_id: String,
    /// New container state after the operation
    pub new_state: String,
}

/// Response from image operations
#[derive(SimpleObject)]
pub struct ImageActionResult {
    /// Whether the operation succeeded
    pub success: bool,
    /// Human-readable message
    pub message: String,
}

/// Response from volume creation
#[derive(SimpleObject)]
pub struct VolumeCreateResult {
    /// Whether the operation succeeded
    pub success: bool,
    /// Human-readable message
    pub message: String,
    /// Name of the created volume
    pub name: Option<String>,
}

/// Response from volume removal
#[derive(SimpleObject)]
pub struct VolumeRemoveResult {
    pub success: bool,
    pub message: String,
}

/// Response from network creation
#[derive(SimpleObject)]
pub struct NetworkCreateResult {
    pub success: bool,
    pub message: String,
    /// ID of the created network
    pub network_id: Option<String>,
}

/// Response from network removal
#[derive(SimpleObject)]
pub struct NetworkRemoveResult {
    pub success: bool,
    pub message: String,
}

/// Response from exec command
#[derive(SimpleObject)]
pub struct ExecCommandResult {
    /// Exit code of the command
    pub exit_code: i32,
    /// Captured stdout (UTF-8)
    pub stdout: String,
    /// Captured stderr (UTF-8)
    pub stderr: String,
    /// Execution time in milliseconds
    pub execution_time_ms: i64,
    /// Whether the command was killed due to timeout
    pub timed_out: bool,
}

/// Response from service creation
#[derive(SimpleObject)]
pub struct ServiceCreateResult {
    pub success: bool,
    pub message: String,
    /// ID of the created service
    pub service_id: Option<String>,
}

/// Response from service deletion
#[derive(SimpleObject)]
pub struct ServiceDeleteResult {
    pub success: bool,
    pub message: String,
}

/// Response from service update
#[derive(SimpleObject)]
pub struct ServiceUpdateResult {
    pub success: bool,
    pub message: String,
}

/// Response from stack deployment
#[derive(SimpleObject)]
pub struct StackDeployResult {
    pub success: bool,
    pub message: String,
    /// IDs of services created
    pub service_ids: Vec<String>,
    /// Services that failed to create
    pub failed_services: Vec<String>,
}

/// Response from stack removal
#[derive(SimpleObject)]
pub struct StackRemoveResult {
    pub success: bool,
    pub message: String,
    /// Number of services removed
    pub services_removed: u32,
}

// ============================================================================
// Helper: Get agent and clone client
// ============================================================================

/// Helper to obtain a cloned gRPC client for a given agent, releasing the lock immediately.
async fn get_client(
    state: &AppState,
    agent_id: &str,
) -> async_graphql::Result<crate::agent::client::AgentGrpcClient> {
    let agent = state
        .agent_pool
        .get_agent(agent_id)
        .ok_or_else(|| ApiError::AgentNotFound(agent_id.to_string()).extend())?;

    // Clone-and-drop pattern: release the Mutex lock immediately
    let client = {
        let guard = agent.client.lock().await;
        guard.clone()
    };

    Ok(client)
}

/// Convert a list of LabelInput to a HashMap
fn labels_to_map(labels: Option<Vec<LabelInput>>) -> std::collections::HashMap<String, String> {
    labels
        .unwrap_or_default()
        .into_iter()
        .map(|l| (l.key, l.value))
        .collect()
}

// ============================================================================
// Mutation Implementations
// ============================================================================

#[async_graphql::Object]
impl MutationRoot {
    // ===================== Container Lifecycle =====================

    /// Start a stopped container
    async fn start_container(
        &self,
        ctx: &Context<'_>,
        input: ContainerActionInput,
    ) -> async_graphql::Result<ContainerActionResult> {
        let state = ctx.data::<AppState>()?;
        let mut client = get_client(state, &input.agent_id).await?;

        let response = client
            .start_container(crate::agent::client::ContainerControlRequest {
                container_id: input.container_id.clone(),
                timeout: None,
            })
            .await
            .map_err(|e| {
                ApiError::Internal(format!("Failed to start container: {}", e)).extend()
            })?;

        Ok(ContainerActionResult {
            success: response.success,
            message: response.message,
            container_id: response.container_id,
            new_state: response.new_state,
        })
    }

    /// Stop a running container
    async fn stop_container(
        &self,
        ctx: &Context<'_>,
        input: ContainerActionInput,
    ) -> async_graphql::Result<ContainerActionResult> {
        let state = ctx.data::<AppState>()?;
        let mut client = get_client(state, &input.agent_id).await?;

        let response = client
            .stop_container(crate::agent::client::ContainerControlRequest {
                container_id: input.container_id.clone(),
                timeout: input.timeout.map(|t| t as u32),
            })
            .await
            .map_err(|e| {
                ApiError::Internal(format!("Failed to stop container: {}", e)).extend()
            })?;

        Ok(ContainerActionResult {
            success: response.success,
            message: response.message,
            container_id: response.container_id,
            new_state: response.new_state,
        })
    }

    /// Restart a container
    async fn restart_container(
        &self,
        ctx: &Context<'_>,
        input: ContainerActionInput,
    ) -> async_graphql::Result<ContainerActionResult> {
        let state = ctx.data::<AppState>()?;
        let mut client = get_client(state, &input.agent_id).await?;

        let response = client
            .restart_container(crate::agent::client::ContainerControlRequest {
                container_id: input.container_id.clone(),
                timeout: input.timeout.map(|t| t as u32),
            })
            .await
            .map_err(|e| {
                ApiError::Internal(format!("Failed to restart container: {}", e)).extend()
            })?;

        Ok(ContainerActionResult {
            success: response.success,
            message: response.message,
            container_id: response.container_id,
            new_state: response.new_state,
        })
    }

    /// Pause a running container (freeze all processes)
    async fn pause_container(
        &self,
        ctx: &Context<'_>,
        input: ContainerActionInput,
    ) -> async_graphql::Result<ContainerActionResult> {
        let state = ctx.data::<AppState>()?;
        let mut client = get_client(state, &input.agent_id).await?;

        let response = client
            .pause_container(crate::agent::client::ContainerControlRequest {
                container_id: input.container_id.clone(),
                timeout: None,
            })
            .await
            .map_err(|e| {
                ApiError::Internal(format!("Failed to pause container: {}", e)).extend()
            })?;

        Ok(ContainerActionResult {
            success: response.success,
            message: response.message,
            container_id: response.container_id,
            new_state: response.new_state,
        })
    }

    /// Unpause a paused container
    async fn unpause_container(
        &self,
        ctx: &Context<'_>,
        input: ContainerActionInput,
    ) -> async_graphql::Result<ContainerActionResult> {
        let state = ctx.data::<AppState>()?;
        let mut client = get_client(state, &input.agent_id).await?;

        let response = client
            .unpause_container(crate::agent::client::ContainerControlRequest {
                container_id: input.container_id.clone(),
                timeout: None,
            })
            .await
            .map_err(|e| {
                ApiError::Internal(format!("Failed to unpause container: {}", e)).extend()
            })?;

        Ok(ContainerActionResult {
            success: response.success,
            message: response.message,
            container_id: response.container_id,
            new_state: response.new_state,
        })
    }

    /// Remove a container
    async fn remove_container(
        &self,
        ctx: &Context<'_>,
        input: ContainerRemoveInput,
    ) -> async_graphql::Result<ContainerActionResult> {
        let state = ctx.data::<AppState>()?;
        let mut client = get_client(state, &input.agent_id).await?;

        let response = client
            .remove_container(crate::agent::client::ContainerRemoveRequest {
                container_id: input.container_id.clone(),
                force: input.force,
                remove_volumes: input.remove_volumes,
            })
            .await
            .map_err(|e| {
                ApiError::Internal(format!("Failed to remove container: {}", e)).extend()
            })?;

        Ok(ContainerActionResult {
            success: response.success,
            message: response.message,
            container_id: response.container_id,
            new_state: response.new_state,
        })
    }

    // ===================== Image Management =====================

    /// Pull an image from a registry
    async fn pull_image(
        &self,
        ctx: &Context<'_>,
        input: ImagePullInput,
    ) -> async_graphql::Result<ImageActionResult> {
        let state = ctx.data::<AppState>()?;
        let mut client = get_client(state, &input.agent_id).await?;

        let response = client
            .pull_image(crate::agent::client::PullImageRequest {
                image: input.image.clone(),
                tag: input.tag.clone(),
                registry_auth: String::new(),
            })
            .await
            .map_err(|e| {
                ApiError::Internal(format!("Failed to pull image: {}", e)).extend()
            })?;

        Ok(ImageActionResult {
            success: response.success,
            message: response.message,
        })
    }

    /// Remove an image
    async fn remove_image(
        &self,
        ctx: &Context<'_>,
        input: ImageRemoveInput,
    ) -> async_graphql::Result<ImageActionResult> {
        let state = ctx.data::<AppState>()?;
        let mut client = get_client(state, &input.agent_id).await?;

        let response = client
            .remove_image(crate::agent::client::RemoveImageRequest {
                image_id: input.image_id.clone(),
                force: input.force,
                no_prune: false,
            })
            .await
            .map_err(|e| {
                ApiError::Internal(format!("Failed to remove image: {}", e)).extend()
            })?;

        Ok(ImageActionResult {
            success: response.success,
            message: response.message,
        })
    }

    // ===================== Volume Management =====================

    /// Create a new volume
    async fn create_volume(
        &self,
        ctx: &Context<'_>,
        input: VolumeCreateInput,
    ) -> async_graphql::Result<VolumeCreateResult> {
        let state = ctx.data::<AppState>()?;
        let mut client = get_client(state, &input.agent_id).await?;

        let labels: std::collections::HashMap<String, String> = input.labels
            .unwrap_or_default()
            .into_iter()
            .map(|l| (l.key, l.value))
            .collect();

        let response = client
            .create_volume(crate::agent::client::CreateVolumeRequest {
                name: input.name.clone(),
                driver: input.driver.unwrap_or_default(),
                driver_opts: std::collections::HashMap::new(),
                labels,
            })
            .await
            .map_err(|e| {
                ApiError::Internal(format!("Failed to create volume: {}", e)).extend()
            })?;

        Ok(VolumeCreateResult {
            success: response.success,
            message: response.message,
            name: Some(response.name),
        })
    }

    /// Remove a volume
    async fn remove_volume(
        &self,
        ctx: &Context<'_>,
        input: VolumeRemoveInput,
    ) -> async_graphql::Result<VolumeRemoveResult> {
        let state = ctx.data::<AppState>()?;
        let mut client = get_client(state, &input.agent_id).await?;

        let response = client
            .remove_volume(crate::agent::client::RemoveVolumeRequest {
                name: input.name.clone(),
                force: input.force,
            })
            .await
            .map_err(|e| {
                ApiError::Internal(format!("Failed to remove volume: {}", e)).extend()
            })?;

        Ok(VolumeRemoveResult {
            success: response.success,
            message: response.message,
        })
    }

    // ===================== Network Management =====================

    /// Create a new network
    async fn create_network(
        &self,
        ctx: &Context<'_>,
        input: NetworkCreateInput,
    ) -> async_graphql::Result<NetworkCreateResult> {
        let state = ctx.data::<AppState>()?;
        let mut client = get_client(state, &input.agent_id).await?;

        let labels: std::collections::HashMap<String, String> = input.labels
            .unwrap_or_default()
            .into_iter()
            .map(|l| (l.key, l.value))
            .collect();

        let response = client
            .create_network_rpc(crate::agent::client::CreateNetworkRequest {
                name: input.name.clone(),
                driver: input.driver.unwrap_or_default(),
                internal: false,
                attachable: false,
                enable_ipv6: false,
                options: std::collections::HashMap::new(),
                labels,
                ipam_driver: String::new(),
                ipam_configs: Vec::new(),
            })
            .await
            .map_err(|e| {
                ApiError::Internal(format!("Failed to create network: {}", e)).extend()
            })?;

        Ok(NetworkCreateResult {
            success: response.success,
            message: response.message,
            network_id: if response.network_id.is_empty() { None } else { Some(response.network_id) },
        })
    }

    /// Remove a network
    async fn remove_network(
        &self,
        ctx: &Context<'_>,
        input: NetworkRemoveInput,
    ) -> async_graphql::Result<NetworkRemoveResult> {
        let state = ctx.data::<AppState>()?;
        let mut client = get_client(state, &input.agent_id).await?;

        let response = client
            .remove_network_rpc(crate::agent::client::RemoveNetworkRequest {
                network_id: input.network_id.clone(),
            })
            .await
            .map_err(|e| {
                ApiError::Internal(format!("Failed to remove network: {}", e)).extend()
            })?;

        Ok(NetworkRemoveResult {
            success: response.success,
            message: response.message,
        })
    }

    // ===================== Shell / Exec =====================

    /// Execute a one-shot command inside a container
    async fn exec_command(
        &self,
        ctx: &Context<'_>,
        input: ExecCommandInput,
    ) -> async_graphql::Result<ExecCommandResult> {
        let state = ctx.data::<AppState>()?;
        let mut client = get_client(state, &input.agent_id).await?;

        // Convert env labels to a HashMap (proto uses map<string,string>)
        let env: std::collections::HashMap<String, String> = input
            .env
            .unwrap_or_default()
            .into_iter()
            .map(|l| (l.key, l.value))
            .collect();

        let response = client
            .exec_command(crate::agent::client::ExecCommandRequest {
                container_id: input.container_id.clone(),
                command: input.command,
                working_dir: input.working_dir,
                env,
                capture_stdout: true,
                capture_stderr: true,
                timeout: input.timeout,
            })
            .await
            .map_err(|e| {
                ApiError::Internal(format!("Failed to exec command: {}", e)).extend()
            })?;

        Ok(ExecCommandResult {
            exit_code: response.exit_code,
            stdout: String::from_utf8_lossy(&response.stdout).to_string(),
            stderr: String::from_utf8_lossy(&response.stderr).to_string(),
            execution_time_ms: response.execution_time_ms,
            timed_out: response.timed_out,
        })
    }

    // ===================== Service Management (M6 Compose/Stack) =====================

    /// Create a new swarm service
    async fn create_service(
        &self,
        ctx: &Context<'_>,
        input: ServiceCreateInput,
    ) -> async_graphql::Result<ServiceCreateResult> {
        let state = ctx.data::<AppState>()?;
        let mut client = get_client(state, &input.agent_id).await?;

        let ports: Vec<crate::agent::client::ServicePortConfig> = input.ports
            .unwrap_or_default()
            .into_iter()
            .map(|p| crate::agent::client::ServicePortConfig {
                target_port: p.target_port,
                published_port: p.published_port,
                protocol: p.protocol,
                publish_mode: p.publish_mode,
            })
            .collect();

        let env: std::collections::HashMap<String, String> = input.env
            .unwrap_or_default()
            .into_iter()
            .map(|l| (l.key, l.value))
            .collect();

        let labels: std::collections::HashMap<String, String> = input.labels
            .unwrap_or_default()
            .into_iter()
            .map(|l| (l.key, l.value))
            .collect();

        let response = client
            .create_service(crate::agent::client::CreateServiceRequest {
                name: input.name,
                image: input.image,
                replicas: input.replicas,
                global: input.global,
                ports,
                env,
                labels,
                networks: input.networks.unwrap_or_default(),
                command: input.command.unwrap_or_default(),
                constraints: input.constraints.unwrap_or_default(),
                resource_limits: None,
                resource_reservations: None,
                mounts: Vec::new(),
                restart_policy: None,
                update_config: None,
                rollback_config: None,
                secrets: Vec::new(),
                configs: Vec::new(),
                health_check: None,
                registry_auth: String::new(),
                log_driver: String::new(),
                log_driver_opts: std::collections::HashMap::new(),
            })
            .await
            .map_err(|e| {
                ApiError::Internal(format!("Failed to create service: {}", e)).extend()
            })?;

        Ok(ServiceCreateResult {
            success: response.success,
            message: response.message,
            service_id: if response.service_id.is_empty() { None } else { Some(response.service_id) },
        })
    }

    /// Delete a swarm service
    async fn delete_service(
        &self,
        ctx: &Context<'_>,
        input: ServiceDeleteInput,
    ) -> async_graphql::Result<ServiceDeleteResult> {
        let state = ctx.data::<AppState>()?;
        let mut client = get_client(state, &input.agent_id).await?;

        let response = client
            .delete_service(crate::agent::client::DeleteServiceRequest {
                service_id: input.service_id,
            })
            .await
            .map_err(|e| {
                ApiError::Internal(format!("Failed to delete service: {}", e)).extend()
            })?;

        Ok(ServiceDeleteResult {
            success: response.success,
            message: response.message,
        })
    }

    /// Update a swarm service (scale, change image, force re-deploy)
    async fn update_service(
        &self,
        ctx: &Context<'_>,
        input: ServiceUpdateInput,
    ) -> async_graphql::Result<ServiceUpdateResult> {
        let state = ctx.data::<AppState>()?;
        let mut client = get_client(state, &input.agent_id).await?;

        let response = client
            .update_service(crate::agent::client::UpdateServiceRequest {
                service_id: input.service_id,
                image: input.image,
                replicas: input.replicas,
                force: input.force,
                env: std::collections::HashMap::new(),
                labels: std::collections::HashMap::new(),
                networks: Vec::new(),
                ports: Vec::new(),
                resource_limits: None,
                resource_reservations: None,
                mounts: Vec::new(),
                restart_policy: None,
                update_config: None,
                rollback_config: None,
                constraints: Vec::new(),
                command: Vec::new(),
                registry_auth: String::new(),
                clear_env: false,
                clear_labels: false,
                clear_networks: false,
                clear_ports: false,
                clear_mounts: false,
                clear_constraints: false,
                clear_command: false,
            })
            .await
            .map_err(|e| {
                ApiError::Internal(format!("Failed to update service: {}", e)).extend()
            })?;

        Ok(ServiceUpdateResult {
            success: response.success,
            message: response.message,
        })
    }

    /// Deploy a stack — creates multiple services grouped by stack namespace label
    async fn deploy_stack(
        &self,
        ctx: &Context<'_>,
        input: StackDeployInput,
    ) -> async_graphql::Result<StackDeployResult> {
        let state = ctx.data::<AppState>()?;
        let mut client = get_client(state, &input.agent_id).await?;

        let mut service_ids = Vec::new();
        let mut failed_services = Vec::new();

        for svc in input.services {
            let full_name = format!("{}_{}", input.stack_name, svc.name);

            let ports: Vec<crate::agent::client::ServicePortConfig> = svc.ports
                .unwrap_or_default()
                .into_iter()
                .map(|p| crate::agent::client::ServicePortConfig {
                    target_port: p.target_port,
                    published_port: p.published_port,
                    protocol: p.protocol,
                    publish_mode: p.publish_mode,
                })
                .collect();

            let env: std::collections::HashMap<String, String> = svc.env
                .unwrap_or_default()
                .into_iter()
                .map(|l| (l.key, l.value))
                .collect();

            // Stack label for grouping
            let mut labels = std::collections::HashMap::new();
            labels.insert("com.docker.stack.namespace".to_string(), input.stack_name.clone());
            labels.insert("com.docker.stack.image".to_string(), svc.image.clone());

            let result = client
                .create_service(crate::agent::client::CreateServiceRequest {
                    name: full_name.clone(),
                    image: svc.image,
                    replicas: svc.replicas,
                    global: false,
                    ports,
                    env,
                    labels,
                    networks: svc.networks.unwrap_or_default(),
                    command: svc.command.unwrap_or_default(),
                    constraints: Vec::new(),
                    resource_limits: None,
                    resource_reservations: None,
                    mounts: Vec::new(),
                    restart_policy: None,
                    update_config: None,
                    rollback_config: None,
                    secrets: Vec::new(),
                    configs: Vec::new(),
                    health_check: None,
                    registry_auth: String::new(),
                    log_driver: String::new(),
                    log_driver_opts: std::collections::HashMap::new(),
                })
                .await;

            match result {
                Ok(resp) if resp.success => {
                    service_ids.push(resp.service_id);
                }
                Ok(resp) => {
                    failed_services.push(format!("{}: {}", full_name, resp.message));
                }
                Err(e) => {
                    failed_services.push(format!("{}: {}", full_name, e));
                }
            }
        }

        let all_succeeded = failed_services.is_empty();
        let message = if all_succeeded {
            format!("Stack '{}' deployed with {} services", input.stack_name, service_ids.len())
        } else {
            format!(
                "Stack '{}' partially deployed: {} succeeded, {} failed",
                input.stack_name,
                service_ids.len(),
                failed_services.len()
            )
        };

        Ok(StackDeployResult {
            success: all_succeeded,
            message,
            service_ids,
            failed_services,
        })
    }

    /// Remove a stack — deletes all services with the matching stack namespace label
    async fn remove_stack(
        &self,
        ctx: &Context<'_>,
        stack_name: String,
        agent_id: String,
    ) -> async_graphql::Result<StackRemoveResult> {
        let state = ctx.data::<AppState>()?;
        let mut client = get_client(state, &agent_id).await?;

        // List all services, then filter by stack namespace label
        let services = client
            .list_services(crate::agent::client::ServiceListRequest {})
            .await
            .map_err(|e| {
                ApiError::Internal(format!("Failed to list services: {}", e)).extend()
            })?;

        let stack_services: Vec<_> = services.services.iter()
            .filter(|s| s.stack_namespace.as_deref() == Some(&stack_name))
            .collect();

        if stack_services.is_empty() {
            return Ok(StackRemoveResult {
                success: false,
                message: format!("No services found for stack '{}'", stack_name),
                services_removed: 0,
            });
        }

        let mut removed = 0u32;
        for svc in &stack_services {
            match client.delete_service(crate::agent::client::DeleteServiceRequest {
                service_id: svc.id.clone(),
            }).await {
                Ok(_) => removed += 1,
                Err(e) => {
                    tracing::warn!(service = %svc.name, stack = %stack_name, "Failed to remove service: {}", e);
                }
            }
        }

        Ok(StackRemoveResult {
            success: removed == stack_services.len() as u32,
            message: format!("Removed {}/{} services from stack '{}'", removed, stack_services.len(), stack_name),
            services_removed: removed,
        })
    }

    // ===================== S9: Node Management =====================

    /// Update a node's availability, role, or labels
    async fn update_node(
        &self,
        ctx: &Context<'_>,
        input: super::types::swarm::NodeUpdateInput,
    ) -> async_graphql::Result<NodeUpdateResult> {
        let state = ctx.data::<AppState>()?;
        let mut client = get_client(state, &input.agent_id).await?;

        let labels: std::collections::HashMap<String, String> = input.labels
            .unwrap_or_default()
            .into_iter()
            .map(|l| (l.key, l.value))
            .collect();

        let request = crate::agent::client::NodeUpdateRequest {
            node_id: input.node_id.clone(),
            availability: input.availability,
            role: input.role,
            labels,
        };

        let response = client
            .update_node(request)
            .await
            .map_err(|e| ApiError::Internal(format!("Failed to update node: {}", e)).extend())?;

        Ok(NodeUpdateResult {
            success: response.success,
            message: response.message,
        })
    }

    // ===================== Node Remove =====================

    /// Remove a node from the swarm
    async fn remove_node(
        &self,
        ctx: &Context<'_>,
        input: RemoveNodeInput,
    ) -> async_graphql::Result<RemoveNodeResult> {
        let state = ctx.data::<AppState>()?;
        let mut client = get_client(state, &input.agent_id).await?;

        let response = client
            .remove_node(crate::agent::client::RemoveNodeRequest {
                node_id: input.node_id.clone(),
                force: input.force.unwrap_or(false),
            })
            .await
            .map_err(|e| {
                ApiError::Internal(format!("Failed to remove node: {}", e)).extend()
            })?;

        Ok(RemoveNodeResult {
            success: response.success,
            message: response.message,
        })
    }

    // ===================== Service Rollback =====================

    /// Rollback a service to its previous specification
    async fn rollback_service(
        &self,
        ctx: &Context<'_>,
        input: RollbackServiceInput,
    ) -> async_graphql::Result<RollbackServiceResult> {
        let state = ctx.data::<AppState>()?;
        let mut client = get_client(state, &input.agent_id).await?;

        let response = client
            .rollback_service(crate::agent::client::RollbackServiceRequest {
                service_id: input.service_id.clone(),
            })
            .await
            .map_err(|e| {
                ApiError::Internal(format!("Failed to rollback service: {}", e)).extend()
            })?;

        Ok(RollbackServiceResult {
            success: response.success,
            message: response.message,
        })
    }

    // ===================== Secret Management =====================

    /// Create a new Docker secret
    async fn create_secret(
        &self,
        ctx: &Context<'_>,
        input: CreateSecretInput,
    ) -> async_graphql::Result<CreateSecretResult> {
        let state = ctx.data::<AppState>()?;
        let mut client = get_client(state, &input.agent_id).await?;

        let labels: std::collections::HashMap<String, String> = input.labels
            .unwrap_or_default()
            .into_iter()
            .map(|l| (l.key, l.value))
            .collect();

        let response = client
            .create_secret(crate::agent::client::CreateSecretRequest {
                name: input.name.clone(),
                data: input.data.clone().into_bytes(),
                labels,
                driver: String::new(),
            })
            .await
            .map_err(|e| {
                ApiError::Internal(format!("Failed to create secret: {}", e)).extend()
            })?;

        Ok(CreateSecretResult {
            success: response.success,
            message: response.message,
            secret_id: if response.secret_id.is_empty() { None } else { Some(response.secret_id) },
        })
    }

    /// Delete a Docker secret
    async fn delete_secret(
        &self,
        ctx: &Context<'_>,
        input: DeleteSecretInput,
    ) -> async_graphql::Result<DeleteSecretResult> {
        let state = ctx.data::<AppState>()?;
        let mut client = get_client(state, &input.agent_id).await?;

        let response = client
            .delete_secret(crate::agent::client::DeleteSecretRequest {
                secret_id: input.secret_id.clone(),
            })
            .await
            .map_err(|e| {
                ApiError::Internal(format!("Failed to delete secret: {}", e)).extend()
            })?;

        Ok(DeleteSecretResult {
            success: response.success,
            message: response.message,
        })
    }

    // ===================== Config Management =====================

    /// Create a new Docker config
    async fn create_config(
        &self,
        ctx: &Context<'_>,
        input: CreateConfigInput,
    ) -> async_graphql::Result<CreateConfigResult> {
        let state = ctx.data::<AppState>()?;
        let mut client = get_client(state, &input.agent_id).await?;

        let labels: std::collections::HashMap<String, String> = input.labels
            .unwrap_or_default()
            .into_iter()
            .map(|l| (l.key, l.value))
            .collect();

        let response = client
            .create_config(crate::agent::client::CreateConfigRequest {
                name: input.name.clone(),
                data: input.data.clone().into_bytes(),
                labels,
            })
            .await
            .map_err(|e| {
                ApiError::Internal(format!("Failed to create config: {}", e)).extend()
            })?;

        Ok(CreateConfigResult {
            success: response.success,
            message: response.message,
            config_id: if response.config_id.is_empty() { None } else { Some(response.config_id) },
        })
    }

    /// Delete a Docker config
    async fn delete_config(
        &self,
        ctx: &Context<'_>,
        input: DeleteConfigInput,
    ) -> async_graphql::Result<DeleteConfigResult> {
        let state = ctx.data::<AppState>()?;
        let mut client = get_client(state, &input.agent_id).await?;

        let response = client
            .delete_config(crate::agent::client::DeleteConfigRequest {
                config_id: input.config_id.clone(),
            })
            .await
            .map_err(|e| {
                ApiError::Internal(format!("Failed to delete config: {}", e)).extend()
            })?;

        Ok(DeleteConfigResult {
            success: response.success,
            message: response.message,
        })
    }

    // ===================== Swarm Lifecycle =====================

    /// Initialize a new swarm
    async fn swarm_init(
        &self,
        ctx: &Context<'_>,
        input: SwarmInitInput,
    ) -> async_graphql::Result<SwarmInitResult> {
        let state = ctx.data::<AppState>()?;
        let mut client = get_client(state, &input.agent_id).await?;

        let response = client
            .swarm_init(crate::agent::client::SwarmInitRequest {
                listen_addr: input.listen_addr.clone(),
                advertise_addr: input.advertise_addr.unwrap_or_default(),
                force_new_cluster: input.force_new_cluster.unwrap_or(false),
            })
            .await
            .map_err(|e| {
                ApiError::Internal(format!("Failed to init swarm: {}", e)).extend()
            })?;

        Ok(SwarmInitResult {
            success: response.success,
            message: response.message,
            node_id: if response.node_id.is_empty() { None } else { Some(response.node_id) },
        })
    }

    /// Join an existing swarm
    async fn swarm_join(
        &self,
        ctx: &Context<'_>,
        input: SwarmJoinInput,
    ) -> async_graphql::Result<SwarmJoinResult> {
        let state = ctx.data::<AppState>()?;
        let mut client = get_client(state, &input.agent_id).await?;

        let response = client
            .swarm_join(crate::agent::client::SwarmJoinRequest {
                listen_addr: input.listen_addr.clone(),
                advertise_addr: input.advertise_addr.unwrap_or_default(),
                remote_addrs: input.remote_addrs.clone(),
                join_token: input.join_token.clone(),
            })
            .await
            .map_err(|e| {
                ApiError::Internal(format!("Failed to join swarm: {}", e)).extend()
            })?;

        Ok(SwarmJoinResult {
            success: response.success,
            message: response.message,
        })
    }

    /// Leave the swarm
    async fn swarm_leave(
        &self,
        ctx: &Context<'_>,
        input: SwarmLeaveInput,
    ) -> async_graphql::Result<SwarmLeaveResult> {
        let state = ctx.data::<AppState>()?;
        let mut client = get_client(state, &input.agent_id).await?;

        let response = client
            .swarm_leave(crate::agent::client::SwarmLeaveRequest {
                force: input.force.unwrap_or(false),
            })
            .await
            .map_err(|e| {
                ApiError::Internal(format!("Failed to leave swarm: {}", e)).extend()
            })?;

        Ok(SwarmLeaveResult {
            success: response.success,
            message: response.message,
        })
    }

    // ===================== Network Connect/Disconnect =====================

    /// Connect a container to a network
    async fn network_connect(
        &self,
        ctx: &Context<'_>,
        input: NetworkConnectInput,
    ) -> async_graphql::Result<NetworkConnectResult> {
        let state = ctx.data::<AppState>()?;
        let mut client = get_client(state, &input.agent_id).await?;

        let response = client
            .network_connect(crate::agent::client::NetworkConnectRequest {
                network_id: input.network_id.clone(),
                container_id: input.container_id.clone(),
            })
            .await
            .map_err(|e| {
                ApiError::Internal(format!("Failed to connect network: {}", e)).extend()
            })?;

        Ok(NetworkConnectResult {
            success: response.success,
            message: response.message,
        })
    }

    /// Disconnect a container from a network
    async fn network_disconnect(
        &self,
        ctx: &Context<'_>,
        input: NetworkDisconnectInput,
    ) -> async_graphql::Result<NetworkDisconnectResult> {
        let state = ctx.data::<AppState>()?;
        let mut client = get_client(state, &input.agent_id).await?;

        let response = client
            .network_disconnect(crate::agent::client::NetworkDisconnectRequest {
                network_id: input.network_id.clone(),
                container_id: input.container_id.clone(),
                force: input.force.unwrap_or(false),
            })
            .await
            .map_err(|e| {
                ApiError::Internal(format!("Failed to disconnect network: {}", e)).extend()
            })?;

        Ok(NetworkDisconnectResult {
            success: response.success,
            message: response.message,
        })
    }

    // =========================================================================
    // B05: Swarm Update / Unlock
    // =========================================================================

    /// Update swarm configuration (raft settings, autolock, token rotation, etc.)
    async fn swarm_update(
        &self,
        ctx: &Context<'_>,
        input: super::types::swarm::SwarmUpdateInput,
        agent_id: String,
    ) -> async_graphql::Result<super::types::swarm::SwarmUpdateResult> {
        let state = ctx.data::<AppState>()?;
        let mut client = get_client(state, &agent_id).await?;

        // Parse numeric strings, returning a clear error on invalid input
        // instead of silently discarding the value.
        let snapshot_interval = input.snapshot_interval
            .map(|s| s.parse::<u64>().map_err(|_| ApiError::Internal(
                format!("Invalid snapshot_interval '{}': expected unsigned integer", s)
            ).extend()))
            .transpose()?;
        let heartbeat_tick = input.heartbeat_tick
            .map(|s| s.parse::<u64>().map_err(|_| ApiError::Internal(
                format!("Invalid heartbeat_tick '{}': expected unsigned integer", s)
            ).extend()))
            .transpose()?;
        let election_tick = input.election_tick
            .map(|s| s.parse::<u64>().map_err(|_| ApiError::Internal(
                format!("Invalid election_tick '{}': expected unsigned integer", s)
            ).extend()))
            .transpose()?;

        let response = client
            .swarm_update(crate::agent::client::SwarmUpdateRequest {
                autolock: input.autolock,
                task_history_limit: input.task_history_limit,
                snapshot_interval,
                heartbeat_tick,
                election_tick,
                cert_expiry_ns: input.cert_expiry_ns,
                rotate_worker_token: input.rotate_worker_token.unwrap_or(false),
                rotate_manager_token: input.rotate_manager_token.unwrap_or(false),
                rotate_manager_unlock_key: input.rotate_manager_unlock_key.unwrap_or(false),
            })
            .await
            .map_err(|e| ApiError::Internal(format!("Failed to update swarm: {}", e)).extend())?;

        Ok(super::types::swarm::SwarmUpdateResult {
            success: response.success,
            message: response.message,
        })
    }

    /// Retrieve the swarm unlock key.
    async fn swarm_unlock_key(
        &self,
        ctx: &Context<'_>,
        agent_id: String,
    ) -> async_graphql::Result<super::types::swarm::SwarmUnlockKeyResult> {
        let state = ctx.data::<AppState>()?;
        let mut client = get_client(state, &agent_id).await?;

        let response = client
            .swarm_unlock_key(crate::agent::client::SwarmUnlockKeyRequest {})
            .await
            .map_err(|e| ApiError::Internal(format!("Failed to get unlock key: {}", e)).extend())?;

        Ok(super::types::swarm::SwarmUnlockKeyResult {
            success: response.success,
            unlock_key: response.unlock_key,
            message: response.message,
        })
    }

    /// Unlock a locked swarm.
    async fn swarm_unlock(
        &self,
        ctx: &Context<'_>,
        agent_id: String,
        unlock_key: String,
    ) -> async_graphql::Result<super::types::swarm::SwarmUnlockResult> {
        let state = ctx.data::<AppState>()?;
        let mut client = get_client(state, &agent_id).await?;

        let response = client
            .swarm_unlock(crate::agent::client::SwarmUnlockRequest {
                unlock_key,
            })
            .await
            .map_err(|e| ApiError::Internal(format!("Failed to unlock swarm: {}", e)).extend())?;

        Ok(super::types::swarm::SwarmUnlockResult {
            success: response.success,
            message: response.message,
        })
    }

    // =========================================================================
    // B11: Compose Stack Deployment
    // =========================================================================

    /// Deploy a Docker Compose stack by parsing YAML and creating services/networks/volumes.
    async fn deploy_compose_stack(
        &self,
        ctx: &Context<'_>,
        input: super::types::swarm::DeployComposeStackInput,
    ) -> async_graphql::Result<super::types::swarm::DeployComposeStackResult> {
        let state = ctx.data::<AppState>()?;

        // Pick a healthy agent that is a swarm manager.
        // Strategy: filter to healthy agents, then probe each for swarm
        // manager status. Fall back to the first healthy agent if none
        // report as manager (single-node swarm, or fresh cluster).
        let agents = state.agent_pool.list_agents();
        let healthy: Vec<_> = agents.iter().filter(|a| a.is_healthy()).collect();
        if healthy.is_empty() {
            return Err(ApiError::Internal("No healthy agents available for stack deployment".to_string()).extend());
        }

        // Try to find a manager agent
        let mut chosen = healthy[0].clone();
        for agent in &healthy {
            let mut probe = {
                let guard = agent.client.lock().await;
                guard.clone()
            };
            if let Ok(resp) = probe.get_swarm_info(crate::agent::client::SwarmInfoRequest {}).await {
                if let Some(swarm) = resp.swarm {
                    if swarm.is_manager {
                        chosen = (*agent).clone();
                        break;
                    }
                }
            }
        }

        let mut client = {
            let guard = chosen.client.lock().await;
            guard.clone()
        };

        let response = client
            .deploy_compose_stack(crate::agent::client::DeployComposeStackRequest {
                stack_name: input.stack_name.clone(),
                compose_yaml: input.compose_yaml.clone(),
            })
            .await
            .map_err(|e| ApiError::Internal(format!("Failed to deploy compose stack: {}", e)).extend())?;

        Ok(super::types::swarm::DeployComposeStackResult {
            success: response.success,
            message: response.message,
            service_ids: response.service_ids,
            network_names: response.network_names,
            volume_names: response.volume_names,
            failed_services: response.failed_services,
        })
    }
}

/// Response from node update
#[derive(SimpleObject)]
pub struct NodeUpdateResult {
    pub success: bool,
    pub message: String,
}

// ===================== New Input/Result Types =====================

#[derive(InputObject)]
pub struct RemoveNodeInput {
    pub agent_id: String,
    pub node_id: String,
    pub force: Option<bool>,
}

#[derive(SimpleObject)]
pub struct RemoveNodeResult {
    pub success: bool,
    pub message: String,
}

#[derive(InputObject)]
pub struct RollbackServiceInput {
    pub agent_id: String,
    pub service_id: String,
}

#[derive(SimpleObject)]
pub struct RollbackServiceResult {
    pub success: bool,
    pub message: String,
}

#[derive(InputObject)]
pub struct CreateSecretInput {
    pub agent_id: String,
    pub name: String,
    pub data: String,
    pub labels: Option<Vec<LabelInput>>,
}

#[derive(SimpleObject)]
pub struct CreateSecretResult {
    pub success: bool,
    pub message: String,
    pub secret_id: Option<String>,
}

#[derive(InputObject)]
pub struct DeleteSecretInput {
    pub agent_id: String,
    pub secret_id: String,
}

#[derive(SimpleObject)]
pub struct DeleteSecretResult {
    pub success: bool,
    pub message: String,
}

#[derive(InputObject)]
pub struct CreateConfigInput {
    pub agent_id: String,
    pub name: String,
    pub data: String,
    pub labels: Option<Vec<LabelInput>>,
}

#[derive(SimpleObject)]
pub struct CreateConfigResult {
    pub success: bool,
    pub message: String,
    pub config_id: Option<String>,
}

#[derive(InputObject)]
pub struct DeleteConfigInput {
    pub agent_id: String,
    pub config_id: String,
}

#[derive(SimpleObject)]
pub struct DeleteConfigResult {
    pub success: bool,
    pub message: String,
}

#[derive(InputObject)]
pub struct SwarmInitInput {
    pub agent_id: String,
    pub listen_addr: String,
    pub advertise_addr: Option<String>,
    pub force_new_cluster: Option<bool>,
}

#[derive(SimpleObject)]
pub struct SwarmInitResult {
    pub success: bool,
    pub message: String,
    pub node_id: Option<String>,
}

#[derive(InputObject)]
pub struct SwarmJoinInput {
    pub agent_id: String,
    pub listen_addr: String,
    pub advertise_addr: Option<String>,
    pub remote_addrs: Vec<String>,
    pub join_token: String,
}

#[derive(SimpleObject)]
pub struct SwarmJoinResult {
    pub success: bool,
    pub message: String,
}

#[derive(InputObject)]
pub struct SwarmLeaveInput {
    pub agent_id: String,
    pub force: Option<bool>,
}

#[derive(SimpleObject)]
pub struct SwarmLeaveResult {
    pub success: bool,
    pub message: String,
}

#[derive(InputObject)]
pub struct NetworkConnectInput {
    pub agent_id: String,
    pub network_id: String,
    pub container_id: String,
}

#[derive(SimpleObject)]
pub struct NetworkConnectResult {
    pub success: bool,
    pub message: String,
}

#[derive(InputObject)]
pub struct NetworkDisconnectInput {
    pub agent_id: String,
    pub network_id: String,
    pub container_id: String,
    pub force: Option<bool>,
}

#[derive(SimpleObject)]
pub struct NetworkDisconnectResult {
    pub success: bool,
    pub message: String,
}
