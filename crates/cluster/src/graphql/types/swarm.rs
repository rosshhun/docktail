use async_graphql::{Enum, InputObject, SimpleObject};

/// Swarm cluster information
#[derive(SimpleObject)]
pub struct SwarmInfoView {
    pub swarm_id: String,
    pub node_id: String,
    pub is_manager: bool,
    pub managers: i32,
    pub workers: i32,
    pub is_swarm_mode: bool,
}

/// A node in the Docker Swarm
#[derive(SimpleObject)]
pub struct NodeView {
    pub id: String,
    pub hostname: String,
    pub role: NodeRoleGql,
    pub availability: NodeAvailabilityGql,
    pub status: NodeStatusGql,
    pub addr: String,
    pub engine_version: String,
    pub os: String,
    pub architecture: String,
    pub labels: Vec<super::agent::Label>,
    pub manager_status: Option<ManagerStatusView>,
    pub nano_cpus: String, // i64 as string for GraphQL
    pub memory_bytes: String,
    /// The Docktail agent ID associated with this node (if discovered)
    pub agent_id: Option<String>,
}

/// Manager-specific status
#[derive(SimpleObject)]
pub struct ManagerStatusView {
    pub leader: bool,
    pub reachability: String,
    pub addr: String,
}

/// Swarm service
#[derive(SimpleObject)]
pub struct ServiceView {
    pub id: String,
    pub name: String,
    pub image: String,
    pub mode: ServiceModeGql,
    pub replicas_desired: i32,
    pub replicas_running: i32,
    pub labels: Vec<super::agent::Label>,
    pub stack_namespace: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub ports: Vec<ServicePortView>,
    pub update_status: Option<UpdateStatusView>,
    pub placement_constraints: Vec<String>,
    pub networks: Vec<String>,
    pub agent_id: String,
    // S6: Rolling update / rollback configuration
    pub update_config: Option<UpdateConfigView>,
    pub rollback_config: Option<UpdateConfigView>,
    pub placement: Option<ServicePlacementView>,
    // S8: Secret and config references
    pub secret_references: Vec<SecretReferenceView>,
    pub config_references: Vec<ConfigReferenceView>,
    // S11: Restart policy
    pub restart_policy: Option<RestartPolicyView>,
}

/// Service port mapping
#[derive(SimpleObject, Clone, Debug)]
pub struct ServicePortView {
    pub protocol: String,
    pub target_port: i32,
    pub published_port: i32,
    pub publish_mode: String,
}

/// Service update status
#[derive(SimpleObject)]
pub struct UpdateStatusView {
    pub state: String,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub message: String,
}

/// A swarm task (instance of a service)
#[derive(SimpleObject, Clone, Debug)]
pub struct TaskView {
    pub id: String,
    pub service_id: String,
    pub service_name: String,
    pub node_id: String,
    pub slot: Option<i32>,
    pub container_id: Option<String>,
    pub state: String,
    pub desired_state: String,
    pub status_message: String,
    pub status_err: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub exit_code: Option<i32>,
    pub agent_id: String,
}

/// A stack (group of services from docker stack deploy)
#[derive(SimpleObject)]
pub struct StackView {
    pub namespace: String,
    pub service_count: i32,
    pub replicas_desired: i32,
    pub replicas_running: i32,
    pub services: Vec<ServiceView>,
    pub agent_id: String,
}

// ============================================================================
// S5: Swarm Networking Types
// ============================================================================

/// A Docker swarm network with overlay/service details
#[derive(SimpleObject)]
pub struct SwarmNetworkView {
    pub id: String,
    pub name: String,
    pub driver: String,
    pub scope: String,
    pub is_internal: bool,
    pub is_attachable: bool,
    pub is_ingress: bool,
    pub enable_ipv6: bool,
    pub created_at: String,
    pub labels: Vec<super::agent::Label>,
    pub options: Vec<super::agent::Label>,
    pub ipam_configs: Vec<IpamConfigView>,
    pub peers: Vec<PeerInfoView>,
    pub service_attachments: Vec<NetworkServiceAttachmentView>,
    pub agent_id: String,
}

/// IPAM configuration for a network
#[derive(SimpleObject)]
pub struct IpamConfigView {
    pub subnet: String,
    pub gateway: String,
    pub ip_range: String,
}

/// A peer node in an overlay network
#[derive(SimpleObject)]
pub struct PeerInfoView {
    pub name: String,
    pub ip: String,
}

/// A service attached to a network with its VIP
#[derive(SimpleObject)]
pub struct NetworkServiceAttachmentView {
    pub service_id: String,
    pub service_name: String,
    pub virtual_ip: String,
}

/// Virtual IP assigned to a service on a network
#[derive(SimpleObject)]
pub struct VirtualIpView {
    pub network_id: String,
    pub addr: String,
}

// ============================================================================
// S6: Orchestration Observability Types
// ============================================================================

/// Rolling update or rollback configuration
#[derive(SimpleObject)]
pub struct UpdateConfigView {
    pub parallelism: i32,
    pub delay_ns: String,            // i64 as string for GraphQL
    pub failure_action: String,
    pub monitor_ns: String,          // i64 as string for GraphQL
    pub max_failure_ratio: f64,
    pub order: String,
}

/// Full placement configuration for a service
#[derive(SimpleObject)]
pub struct ServicePlacementView {
    pub constraints: Vec<String>,
    pub preferences: Vec<PlacementPreferenceView>,
    pub max_replicas_per_node: Option<i32>,
    pub platforms: Vec<PlatformView>,
}

/// Placement preference (topology-aware scheduling)
#[derive(SimpleObject)]
pub struct PlacementPreferenceView {
    pub spread_descriptor: String,
}

/// Platform constraint (architecture + OS)
#[derive(SimpleObject)]
pub struct PlatformView {
    pub architecture: String,
    pub os: String,
}

/// Real-time rolling update progress event
#[derive(SimpleObject, Clone)]
pub struct ServiceUpdateEventView {
    pub update_state: String,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub message: String,
    pub tasks_total: i32,
    pub tasks_running: i32,
    pub tasks_ready: i32,
    pub tasks_failed: i32,
    pub tasks_shutdown: i32,
    pub snapshot_at: chrono::DateTime<chrono::Utc>,
    pub recent_changes: Vec<TaskStateChangeView>,
}

/// A single task state change in a rolling update
#[derive(SimpleObject, Clone)]
pub struct TaskStateChangeView {
    pub task_id: String,
    pub service_id: String,
    pub node_id: String,
    pub slot: Option<i32>,
    pub state: String,
    pub desired_state: String,
    pub message: String,
    pub error: Option<String>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

// ============================================================================
// S8: Swarm Secrets & Configs Types
// ============================================================================

/// Metadata about a swarm secret (never includes actual secret data)
#[derive(SimpleObject, Clone)]
pub struct SwarmSecretView {
    pub id: String,
    pub name: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub labels: Vec<super::agent::Label>,
    /// Driver name for external secret stores (empty if Docker-managed)
    pub driver: String,
    pub agent_id: String,
}

/// Metadata about a swarm config (data content omitted)
#[derive(SimpleObject, Clone)]
pub struct SwarmConfigView {
    pub id: String,
    pub name: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub labels: Vec<super::agent::Label>,
    pub agent_id: String,
}

/// A secret reference attached to a service
#[derive(SimpleObject, Clone)]
pub struct SecretReferenceView {
    pub secret_id: String,
    pub secret_name: String,
    /// Mount path inside the container (e.g., "/run/secrets/my_secret")
    pub file_name: String,
    pub file_uid: String,
    pub file_gid: String,
    pub file_mode: i32,
}

/// A config reference attached to a service
#[derive(SimpleObject, Clone)]
pub struct ConfigReferenceView {
    pub config_id: String,
    pub config_name: String,
    /// Mount path inside the container
    pub file_name: String,
    pub file_uid: String,
    pub file_gid: String,
    pub file_mode: i32,
}

// ============================================================================
// S7: Side-by-Side Replica Log Comparison Types
// ============================================================================

/// Input specifying a log source for comparison
#[derive(Debug, Clone, InputObject)]
pub struct ComparisonSourceInput {
    /// Container ID (for standalone container comparison)
    pub container_id: Option<String>,
    /// Service ID (for service-level log comparison)
    pub service_id: Option<String>,
    /// Task ID (for specific task/replica comparison)
    pub task_id: Option<String>,
    /// Agent ID where the source is running
    pub agent_id: String,
    /// Display label for this lane (e.g., "web-1", "replica-2")
    pub label: Option<String>,
}

/// A comparison source returned by serviceReplicas query
#[derive(SimpleObject, Clone)]
pub struct ComparisonSource {
    /// Container ID (if running)
    pub container_id: Option<String>,
    /// Service ID
    pub service_id: String,
    /// Task ID
    pub task_id: String,
    /// Agent ID
    pub agent_id: String,
    /// Display label (e.g., "my-service.1", "my-service.2")
    pub label: String,
    /// Task slot (replica index)
    pub slot: Option<i32>,
    /// Node ID where the task is running
    pub node_id: String,
    /// Task state (e.g., "running")
    pub state: String,
}

/// Synchronization mode for comparison log alignment
#[derive(Enum, Copy, Clone, Eq, PartialEq, Default)]
pub enum SyncMode {
    /// Align entries by timestamp (default)
    #[default]
    Timestamp,
    /// Align by sequence number
    Sequence,
    /// No synchronization — independent streams
    None,
}

/// A log entry tagged with lane information for side-by-side comparison
#[derive(SimpleObject, Clone)]
pub struct ComparisonLogEntry {
    /// Lane index (0, 1, 2…) — which column this entry belongs to
    pub lane_index: i32,
    /// Display label for this lane
    pub lane_label: String,
    /// The actual log entry
    pub entry: super::log::LogEntry,
    /// Normalized timestamp for alignment across lanes
    pub sync_timestamp: chrono::DateTime<chrono::Utc>,
}

// ============================================================================
// Enums
// ============================================================================

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum NodeRoleGql {
    Unknown,
    Manager,
    Worker,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum NodeAvailabilityGql {
    Unknown,
    Active,
    Pause,
    Drain,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum NodeStatusGql {
    Unknown,
    Ready,
    Down,
    Disconnected,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum ServiceModeGql {
    Unknown,
    Replicated,
    Global,
    ReplicatedJob,
    GlobalJob,
}

// ============================================================================
// Conversion helpers (proto -> GraphQL)
// ============================================================================

impl NodeRoleGql {
    pub fn from_proto(value: i32) -> Self {
        match value {
            1 => Self::Manager,
            2 => Self::Worker,
            _ => Self::Unknown,
        }
    }
}

impl NodeAvailabilityGql {
    pub fn from_proto(value: i32) -> Self {
        match value {
            1 => Self::Active,
            2 => Self::Pause,
            3 => Self::Drain,
            _ => Self::Unknown,
        }
    }
}

impl NodeStatusGql {
    pub fn from_proto(value: i32) -> Self {
        match value {
            1 => Self::Ready,
            2 => Self::Down,
            3 => Self::Disconnected,
            _ => Self::Unknown,
        }
    }
}

impl ServiceModeGql {
    pub fn from_proto(value: i32) -> Self {
        match value {
            1 => Self::Replicated,
            2 => Self::Global,
            3 => Self::ReplicatedJob,
            4 => Self::GlobalJob,
            _ => Self::Unknown,
        }
    }
}

// =============================================================================
// S9: Node Management & Drain Awareness
// =============================================================================

/// Node event — emitted when a node's state, availability, or role changes
#[derive(SimpleObject, Clone, Debug)]
pub struct NodeEventView {
    pub node_id: String,
    pub hostname: String,
    pub event_type: NodeEventTypeGql,
    pub previous_value: String,
    pub current_value: String,
    /// Tasks affected by this event (e.g., tasks being migrated during drain)
    pub affected_tasks: Vec<TaskView>,
    pub timestamp: i64,
}

/// Node event type enumeration
#[derive(Enum, Copy, Clone, Debug, Eq, PartialEq)]
pub enum NodeEventTypeGql {
    Unknown,
    StateChange,
    AvailabilityChange,
    RoleChange,
    DrainStarted,
    DrainCompleted,
    NodeDown,
    NodeReady,
}

impl NodeEventTypeGql {
    pub fn from_proto(value: i32) -> Self {
        match value {
            1 => Self::StateChange,
            2 => Self::AvailabilityChange,
            3 => Self::RoleChange,
            4 => Self::DrainStarted,
            5 => Self::DrainCompleted,
            6 => Self::NodeDown,
            7 => Self::NodeReady,
            _ => Self::Unknown,
        }
    }
}

/// Input for updating a node's availability, role, or labels
#[derive(InputObject, Clone, Debug)]
pub struct NodeUpdateInput {
    pub node_id: String,
    /// Agent ID (must be a swarm manager). If omitted, auto-selects a manager.
    pub agent_id: Option<String>,
    /// New availability: "active", "pause", or "drain"
    pub availability: Option<String>,
    /// New role: "worker" or "manager"
    pub role: Option<String>,
    /// Labels to set (replaces all existing labels)
    pub labels: Option<Vec<super::super::mutations::LabelInput>>,
}

// =============================================================================
// S10: Service Scaling Insights & Coverage
// =============================================================================

/// A service scaling or lifecycle event
#[derive(SimpleObject, Clone, Debug)]
pub struct ServiceEventView {
    pub service_id: String,
    pub event_type: ServiceEventTypeGql,
    pub previous_replicas: Option<i32>,
    pub current_replicas: Option<i32>,
    pub timestamp: i64,
    pub message: String,
    pub affected_tasks: Vec<TaskView>,
}

/// Service event type enumeration
#[derive(Enum, Copy, Clone, Debug, Eq, PartialEq)]
pub enum ServiceEventTypeGql {
    Unknown,
    ScaledUp,
    ScaledDown,
    UpdateStarted,
    UpdateCompleted,
    UpdateRolledBack,
    TaskFailed,
    TaskRecovered,
}

impl ServiceEventTypeGql {
    pub fn from_proto(value: i32) -> Self {
        match value {
            1 => Self::ScaledUp,
            2 => Self::ScaledDown,
            3 => Self::UpdateStarted,
            4 => Self::UpdateCompleted,
            5 => Self::UpdateRolledBack,
            6 => Self::TaskFailed,
            7 => Self::TaskRecovered,
            _ => Self::Unknown,
        }
    }
}

/// Coverage information for a global service — which nodes have tasks and which don't
#[derive(SimpleObject, Clone, Debug)]
pub struct ServiceCoverageView {
    /// Node IDs that have a running task for this service
    pub covered_nodes: Vec<String>,
    /// Node IDs that should have a task but don't (or task is not running)
    pub uncovered_nodes: Vec<String>,
    /// Total eligible nodes in the swarm
    pub total_nodes: i32,
    /// Percentage of nodes covered (0.0 – 100.0)
    pub coverage_percentage: f64,
    /// Service ID
    pub service_id: String,
    /// Whether this is a global service
    pub is_global: bool,
    /// Agent ID that answered
    pub agent_id: String,
}

// =============================================================================
// S11: Stack-Level Health & Restart Policies
// =============================================================================

/// Swarm service restart policy (from TaskSpec)
#[derive(SimpleObject, Clone, Debug)]
pub struct RestartPolicyView {
    /// Condition: "none", "on-failure", "any"
    pub condition: String,
    /// Delay between restart attempts (human-readable, e.g. "5s")
    pub delay_ns: String,
    /// Maximum number of restart attempts (0 = unlimited)
    pub max_attempts: i32,
    /// Evaluation window in nanoseconds (0 = unbounded)
    pub window_ns: String,
}

/// Aggregated health for an entire stack (all services in a namespace)
#[derive(SimpleObject, Clone, Debug)]
pub struct StackHealthView {
    pub namespace: String,
    pub overall_status: StackHealthStatusGql,
    pub service_healths: Vec<ServiceHealthView>,
    pub total_services: i32,
    pub healthy_services: i32,
    pub degraded_services: i32,
    pub unhealthy_services: i32,
    pub total_desired: i32,
    pub total_running: i32,
    pub total_failed: i32,
    pub agent_id: String,
}

/// Health status for a single service within a stack
#[derive(SimpleObject, Clone, Debug)]
pub struct ServiceHealthView {
    pub service_id: String,
    pub service_name: String,
    pub health_status: ServiceHealthStatusGql,
    pub replicas_desired: i32,
    pub replicas_running: i32,
    pub replicas_failed: i32,
    /// Recent task failure messages (last 5)
    pub recent_errors: Vec<String>,
    /// Whether a rolling update is in progress
    pub update_in_progress: bool,
    /// Restart policy for this service
    pub restart_policy: Option<RestartPolicyView>,
}

/// Stack-level health status
#[derive(Enum, Copy, Clone, Debug, Eq, PartialEq)]
pub enum StackHealthStatusGql {
    Unknown,
    Healthy,
    Degraded,
    Unhealthy,
}

impl StackHealthStatusGql {
    pub fn from_proto(value: i32) -> Self {
        match value {
            1 => Self::Healthy,
            2 => Self::Degraded,
            3 => Self::Unhealthy,
            _ => Self::Unknown,
        }
    }
}

/// Service-level health status
#[derive(Enum, Copy, Clone, Debug, Eq, PartialEq)]
pub enum ServiceHealthStatusGql {
    Unknown,
    Healthy,
    Degraded,
    Unhealthy,
}

impl ServiceHealthStatusGql {
    pub fn from_proto(value: i32) -> Self {
        match value {
            1 => Self::Healthy,
            2 => Self::Degraded,
            3 => Self::Unhealthy,
            _ => Self::Unknown,
        }
    }
}

/// A service restart event (task replaced, crash loop, OOM)
#[derive(SimpleObject, Clone, Debug)]
pub struct ServiceRestartEventView {
    pub service_id: String,
    pub service_name: String,
    pub event_type: RestartEventTypeGql,
    pub new_task: Option<TaskView>,
    pub old_task: Option<TaskView>,
    pub slot: Option<i32>,
    pub restart_count: i32,
    pub timestamp: i64,
    pub message: String,
}

/// Restart event type
#[derive(Enum, Copy, Clone, Debug, Eq, PartialEq)]
pub enum RestartEventTypeGql {
    Unknown,
    TaskRestarted,
    CrashLoop,
    OomKilled,
}

impl RestartEventTypeGql {
    pub fn from_proto(value: i32) -> Self {
        match value {
            1 => Self::TaskRestarted,
            2 => Self::CrashLoop,
            3 => Self::OomKilled,
            _ => Self::Unknown,
        }
    }
}

/// A Docker engine event (container, image, network, volume, etc.)
#[derive(SimpleObject, Clone, Debug)]
pub struct DockerEventView {
    pub agent_id: String,
    pub event_type: String,
    pub action: String,
    pub actor_id: String,
    pub actor_name: String,
    pub attributes: Vec<super::agent::Label>,
    pub timestamp: i64,
}

// =========================================================================
// B03: Task Inspect View
// =========================================================================

/// Detailed task inspection result
#[derive(SimpleObject, Clone, Debug)]
pub struct TaskInspectView {
    pub id: String,
    pub service_id: String,
    pub service_name: String,
    pub node_id: String,
    pub slot: Option<String>,
    pub container_id: Option<String>,
    pub state: String,
    pub desired_state: String,
    pub status_message: String,
    pub status_err: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub exit_code: Option<i32>,
    pub image: String,
    pub command: Vec<String>,
    pub args: Vec<String>,
    pub env: Vec<super::agent::Label>,
    pub labels: Vec<super::agent::Label>,
    pub network_attachments: Vec<TaskNetworkAttachmentView>,
    pub resource_limits: Option<ResourceView>,
    pub resource_reservations: Option<ResourceView>,
    pub restart_policy: Option<RestartPolicyView>,
    pub started_at: String,
    pub finished_at: String,
    pub ports: Vec<ServicePortView>,
}

/// Network attachment for a task
#[derive(SimpleObject, Clone, Debug)]
pub struct TaskNetworkAttachmentView {
    pub network_id: String,
    pub network_name: String,
    pub addresses: Vec<String>,
}

/// Resource limits/reservations view
#[derive(SimpleObject, Clone, Debug)]
pub struct ResourceView {
    pub nano_cpus: String,
    pub memory_bytes: String,
}

// =========================================================================
// B05: Swarm Update / Unlock
// =========================================================================

/// Input for updating swarm settings
#[derive(InputObject)]
pub struct SwarmUpdateInput {
    pub autolock: Option<bool>,
    pub task_history_limit: Option<i64>,
    pub snapshot_interval: Option<String>,
    pub heartbeat_tick: Option<String>,
    pub election_tick: Option<String>,
    pub cert_expiry_ns: Option<i64>,
    pub rotate_worker_token: Option<bool>,
    pub rotate_manager_token: Option<bool>,
    pub rotate_manager_unlock_key: Option<bool>,
}

/// Result of swarm update
#[derive(SimpleObject)]
pub struct SwarmUpdateResult {
    pub success: bool,
    pub message: String,
}

/// Result of swarm unlock key retrieval
#[derive(SimpleObject)]
pub struct SwarmUnlockKeyResult {
    pub success: bool,
    pub unlock_key: String,
    pub message: String,
}

/// Result of swarm unlock
#[derive(SimpleObject)]
pub struct SwarmUnlockResult {
    pub success: bool,
    pub message: String,
}

// =========================================================================
// B11: Compose Stack Deployment
// =========================================================================

/// Input for deploying a compose stack
#[derive(InputObject)]
pub struct DeployComposeStackInput {
    pub stack_name: String,
    pub compose_yaml: String,
}

/// Result of compose stack deployment
#[derive(SimpleObject)]
pub struct DeployComposeStackResult {
    pub success: bool,
    pub message: String,
    pub service_ids: Vec<String>,
    pub network_names: Vec<String>,
    pub volume_names: Vec<String>,
    pub failed_services: Vec<String>,
}

// =========================================================================
// B12: Stack File Viewer
// =========================================================================

/// Result of stack file lookup
#[derive(SimpleObject)]
pub struct StackFileResult {
    pub found: bool,
    pub stack_name: String,
    pub compose_yaml: String,
}
