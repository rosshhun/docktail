/**
 * GraphQL Type Definitions for Docktail Cluster API
 * 
 * This file contains all TypeScript interfaces and types that map to the
 * GraphQL schema exposed by the Docktail cluster.
 * 
 * @module api/types
 */

// ============================================================================
// CONTAINER TYPES
// ============================================================================

/**
 * Container state enum matching the GraphQL ContainerState type
 */
export type ContainerState = 
  | 'CREATED' 
  | 'RUNNING' 
  | 'PAUSED' 
  | 'RESTARTING' 
  | 'REMOVING' 
  | 'EXITED' 
  | 'DEAD';

/**
 * Port mapping information
 */
export interface PortMapping {
  /** Container port */
  containerPort: number;
  /** Protocol (tcp, udp, sctp) */
  protocol: string;
  /** Host IP (if mapped to host) */
  hostIp?: string;
  /** Host port (if mapped to host) */
  hostPort?: number;
}

/**
 * Core container information returned from GraphQL queries
 */
export interface Container {
  /** Unique container ID (64-char Docker hash) */
  id: string;
  /** Human-readable container name (without leading /) */
  name: string;
  /** Docker image name with tag */
  image: string;
  /** Current container state */
  state: ContainerState;
  /** Agent ID where this container is running */
  agentId: string;
  /** Timestamp when container was created (ISO-8601) */
  createdAt: string;
  /** Human-readable status string */
  status: string;
  /** Container labels (Docker metadata) */
  labels: Array<{ key: string; value: string }>;
  /** Log driver used by this container (e.g., "json-file", "journald") */
  logDriver?: string;
  /** Port mappings (now included in basic container info) */
  ports: PortMapping[];
  /** Detailed state information (from inspect, available on single container queries) */
  stateInfo?: ContainerStateInfo;
  /** Optional detailed container information (must be explicitly requested) */
  details?: ContainerDetails;
}

/**
 * Extended container with resolved agent name
 * Used in UI views that need both container and agent info
 */
export interface ContainerWithAgent extends Container {
  /** Resolved human-readable agent name */
  agentName: string;
}

/**
 * Detailed container configuration and runtime information
 * Retrieved via the Container.details nested field
 */
export interface ContainerDetails {
  /** Command executed in the container */
  command: string[];
  /** Working directory inside container */
  workingDir: string;
  /** Environment variables */
  env: string[];
  /** Exposed ports (format: "80/tcp") */
  exposedPorts: string[];
  /** Volume mounts */
  mounts: VolumeMount[];
  /** Network attachments */
  networks: NetworkInfo[];
  /** Resource limits (CPU, memory, PIDs) */
  limits?: ResourceLimits;
  /** Entrypoint command */
  entrypoint: string[];
  /** Container hostname */
  hostname?: string;
  /** User the container process runs as */
  user?: string;
  /** Restart policy */
  restartPolicy?: RestartPolicy;
  /** Network mode (bridge, host, none) */
  networkMode?: string;
  /** Healthcheck configuration */
  healthcheck?: HealthcheckConfig;
  /** Platform (e.g., "linux") */
  platform?: string;
  /** Container runtime (e.g., "runc") */
  runtime?: string;
}

/**
 * Volume mount configuration
 */
export interface VolumeMount {
  /** Host path or volume name */
  source: string;
  /** Path inside container */
  destination: string;
  /** Mount mode (e.g., "rw", "ro") */
  mode: string;
  /** Mount type: "bind", "volume", or "tmpfs" */
  mountType?: string;
  /** Mount propagation mode */
  propagation?: string;
}

/**
 * Network attachment information
 */
export interface NetworkInfo {
  /** Docker network name */
  networkName: string;
  /** Container's IP address on this network */
  ipAddress: string;
  /** Network gateway IP */
  gateway: string;
  /** MAC address on this network */
  macAddress?: string;
}

/**
 * Container resource limits
 */
export interface ResourceLimits {
  /** Memory limit in bytes (null = unlimited) */
  memoryLimitBytes?: number;
  /** CPU limit (number of cores, e.g., 1.5) */
  cpuLimit?: number;
  /** Maximum number of PIDs/processes */
  pidsLimit?: number;
}

/**
 * Detailed container state information from docker inspect
 */
export interface ContainerStateInfo {
  /** Whether the container was killed due to OOM */
  oomKilled: boolean;
  /** Host PID of the container's main process */
  pid: number;
  /** Exit code of the last run */
  exitCode: number;
  /** When the container last started (RFC3339) */
  startedAt: string;
  /** When the container last finished (RFC3339) */
  finishedAt: string;
  /** Number of times the container has restarted */
  restartCount: number;
}

/**
 * Container restart policy
 */
export interface RestartPolicy {
  /** Policy name: "no", "always", "unless-stopped", "on-failure" */
  name: string;
  /** Maximum retry count (for "on-failure" policy) */
  maxRetryCount: number;
}

/**
 * Container healthcheck configuration
 */
export interface HealthcheckConfig {
  /** Test command */
  test: string[];
  /** Interval between checks (nanoseconds) */
  intervalNs: number;
  /** Timeout for each check (nanoseconds) */
  timeoutNs: number;
  /** Retries before marking unhealthy */
  retries: number;
  /** Grace period before checks begin (nanoseconds) */
  startPeriodNs: number;
}

// ============================================================================
// DOCKER COMPOSE HELPERS
// ============================================================================

/**
 * Extracted Docker Compose metadata from container labels
 */
export interface ComposeMetadata {
  /** Docker Compose project name */
  project?: string;
  /** Service name within the project */
  service?: string;
  /** Docker Compose version used */
  version?: string;
  /** Compose file paths */
  configFiles?: string;
  /** Container number (for scaled services) */
  containerNumber?: string;
  /** Whether this is a one-off container (docker-compose run) */
  oneoff?: string;
}

// ============================================================================
// AGENT TYPES
// ============================================================================

/**
 * Agent health status enum
 */
export type AgentStatus = 'HEALTHY' | 'DEGRADED' | 'UNHEALTHY' | 'UNKNOWN';

/**
 * Agent information from GraphQL
 */
export interface Agent {
  /** Unique agent ID */
  id: string;
  /** Human-readable agent name */
  name: string;
  /** Current health status */
  status: AgentStatus;
  /** Agent labels (key-value metadata) */
  labels: Array<{ key: string; value: string }>;
  /** Agent version (if available) */
  version?: string;
  /** Agent address (host:port) */
  address: string;
  /** Last time agent was seen by cluster (ISO-8601) */
  lastSeen: string;
}

/**
 * Aggregated health summary for all agents
 */
export interface AgentHealthSummary {
  /** Total number of agents */
  total: number;
  /** Number of healthy agents */
  healthy: number;
  /** Number of degraded agents */
  degraded: number;
  /** Number of unhealthy agents */
  unhealthy: number;
  /** Number of agents with unknown status */
  unknown: number;
}

/**
 * Real-time agent health event from subscription
 */
export interface AgentHealthEvent {
  /** Agent ID */
  agentId: string;
  /** Current status */
  status: AgentStatus;
  /** Human-readable status message */
  message: string;
  /** Unix timestamp (seconds) */
  timestamp: number;
  /** Additional metadata (e.g., parsing metrics) */
  metadata: Array<{ key: string; value: string }>;
}

// ============================================================================
// REAL-TIME STATS TYPES (for subscriptions)
// ============================================================================

/**
 * Real-time container stats event from subscription
 * 
 * This is an alias for ContainerStats, used in subscriptions.
 * The structure is identical but the semantics differ (streaming vs snapshot).
 */
export type ContainerStatsEvent = ContainerStats;

// ============================================================================
// CONTAINER STATS TYPES
// ============================================================================

/**
 * Real-time container resource statistics
 */
export interface ContainerStats {
  /** Container ID */
  containerId: string;
  /** Unix timestamp when stats were collected */
  timestamp: number;
  /** CPU usage statistics */
  cpuStats: CpuStats;
  /** Memory usage statistics */
  memoryStats: MemoryStats;
  /** Network I/O statistics (one per interface) */
  networkStats: NetworkStats[];
  /** Block I/O (disk) statistics */
  blockIoStats: BlockIoStats;
  /** Number of processes in container */
  pidsCount?: number;
}

/**
 * CPU usage statistics
 */
export interface CpuStats {
  /** CPU usage percentage (0-100 per core, can exceed 100% on multi-core) */
  cpuPercentage: number;
  /** Total CPU time consumed (nanoseconds) */
  totalUsage: number;
  /** CPU time in kernel mode (nanoseconds) */
  systemUsage: number;
  /** Number of CPU cores available to container */
  onlineCpus: number;
  /** Per-CPU usage breakdown (nanoseconds) */
  perCpuUsage: number[];
  /** CPU throttling statistics (if limits are set) */
  throttling?: CpuThrottlingStats;
}

/**
 * CPU throttling statistics
 */
export interface CpuThrottlingStats {
  /** Number of periods with throttling active */
  throttledPeriods: number;
  /** Total number of periods */
  totalPeriods: number;
  /** Total time throttled (nanoseconds) */
  throttledTime: number;
}

/**
 * Memory usage statistics
 */
export interface MemoryStats {
  /** Current memory usage (bytes) */
  usage: number;
  /** Maximum memory usage recorded (bytes) */
  maxUsage: number;
  /** Memory limit (bytes, 0 = unlimited) */
  limit: number;
  /** Memory usage percentage (0-100) */
  percentage: number;
  /** Cache memory (bytes) */
  cache: number;
  /** RSS memory (bytes) - actual physical memory used */
  rss: number;
  /** Swap usage (bytes) */
  swap?: number;
}

/**
 * Network interface statistics
 */
export interface NetworkStats {
  /** Network interface name */
  interfaceName: string;
  /** Bytes received */
  rxBytes: number;
  /** Packets received */
  rxPackets: number;
  /** Receive errors */
  rxErrors: number;
  /** Receive dropped packets */
  rxDropped: number;
  /** Bytes transmitted */
  txBytes: number;
  /** Packets transmitted */
  txPackets: number;
  /** Transmit errors */
  txErrors: number;
  /** Transmit dropped packets */
  txDropped: number;
}

/**
 * Block I/O statistics
 */
export interface BlockIoStats {
  /** Total bytes read from disk */
  readBytes: number;
  /** Total bytes written to disk */
  writeBytes: number;
  /** Total read operations */
  readOps: number;
  /** Total write operations */
  writeOps: number;
  /** Per-device statistics */
  devices: BlockIoDeviceStats[];
}

/**
 * Per-device block I/O statistics
 */
export interface BlockIoDeviceStats {
  /** Device major number */
  major: number;
  /** Device minor number */
  minor: number;
  /** Bytes read from this device */
  readBytes: number;
  /** Bytes written to this device */
  writeBytes: number;
}

// ============================================================================
// LOG TYPES
// ============================================================================

/**
 * Log level enum
 */
export type LogLevel = 'STDOUT' | 'STDERR';

/**
 * Log entry from a container
 */
export interface LogEvent {
  /** Container ID this log belongs to */
  containerId: string;
  /** Agent ID where container is running */
  agentId: string;
  /** Timestamp when log was generated (ISO-8601) */
  timestamp: string;
  /** Raw log content */
  content: string;
  /** Log level (stdout or stderr) */
  level: LogLevel;
  /** Sequence number for ordering and gap detection */
  sequence: number;
  /** Parsed structured log data (if parsing succeeded) */
  parsed?: ParsedLogData;
  /** Detected log format (JSON, Logfmt, PlainText, etc.) */
  format: string;
  /** Whether parsing succeeded */
  parseSuccess: boolean;
  /** Multiline grouping: continuation lines (empty if not grouped) */
  groupedLines: LogLine[];
  /** Total lines (1 = single line) */
  lineCount: number;
  /** Quick check for grouped logs */
  isGrouped: boolean;
}

/**
 * Individual log line within a multiline group
 */
export interface LogLine {
  /** Log line content */
  content: string;
  /** Timestamp (ISO-8601) */
  timestamp: string;
  /** Sequence number */
  sequence: number;
}

/**
 * Parsed structured log data
 */
export interface ParsedLogData {
  /** Extracted log level (info, warn, error, debug) */
  level?: string;
  /** Main log message */
  message?: string;
  /** Logger name (e.g., "app.users") */
  logger?: string;
  /** Application timestamp (ISO-8601, if different from Docker timestamp) */
  timestamp?: string;
  /** HTTP request context */
  request?: RequestContext;
  /** Error context */
  error?: ErrorContext;
  /** Additional key-value fields */
  fields: KeyValueField[];
}

/**
 * HTTP request context from parsed logs
 */
export interface RequestContext {
  /** HTTP method (GET, POST, etc.) */
  method?: string;
  /** Request path */
  path?: string;
  /** Client IP address */
  remoteAddr?: string;
  /** HTTP status code */
  statusCode?: number;
  /** Request duration in milliseconds */
  durationMs?: number;
  /** Request/correlation ID */
  requestId?: string;
}

/**
 * Error context from parsed logs
 */
export interface ErrorContext {
  /** Exception/error type */
  errorType?: string;
  /** Error message */
  errorMessage?: string;
  /** Stack trace lines */
  stackTrace: string[];
  /** Source file */
  file?: string;
  /** Line number */
  line?: number;
}

/**
 * Key-value field from parsed logs
 */
export interface KeyValueField {
  /** Field name */
  key: string;
  /** Field value */
  value: string;
}

// ============================================================================
// CLUSTER TYPES
// ============================================================================

/**
 * Cluster health status
 */
export interface HealthStatus {
  /** Overall health status */
  status: string;
  /** Timestamp of health check (ISO-8601) */
  timestamp: string;
}

// ============================================================================
// DISCOVERY TYPES
// ============================================================================

/**
 * Agent discovery status view
 */
export interface DiscoveryStatus {
  swarmDiscoveryEnabled: boolean;
  registrationEnabled: boolean;
  discoveryLabel: string;
  discoveryIntervalSecs: number;
  agentPort: number;
  totalAgents: number;
  staticAgents: number;
  discoveredAgents: number;
  registeredAgents: number;
}

/**
 * Agent source enum
 */
export type AgentSource = 'STATIC' | 'DISCOVERED' | 'REGISTERED';

// ============================================================================
// SWARM TYPES
// ============================================================================

/**
 * Swarm cluster info
 */
export interface SwarmInfo {
  swarmId: string;
  nodeId: string;
  isManager: boolean;
  managers: number;
  workers: number;
  isSwarmMode: boolean;
}

/**
 * Node role enum
 */
export type NodeRole = 'UNKNOWN' | 'MANAGER' | 'WORKER';

/**
 * Node availability enum
 */
export type NodeAvailability = 'UNKNOWN' | 'ACTIVE' | 'PAUSE' | 'DRAIN';

/**
 * Node state enum
 */
export type NodeState = 'UNKNOWN' | 'READY' | 'DOWN' | 'DISCONNECTED';

/**
 * Manager status info
 */
export interface ManagerStatus {
  leader: boolean;
  reachability: string;
  addr: string;
}

/**
 * Swarm node view
 */
export interface NodeView {
  id: string;
  hostname: string;
  role: NodeRole;
  availability: NodeAvailability;
  status: NodeState;
  addr: string;
  engineVersion: string;
  os: string;
  architecture: string;
  labels: Array<{ key: string; value: string }>;
  managerStatus?: ManagerStatus;
  nanoCpus: string;
  memoryBytes: string;
  agentId: string;
}

/**
 * Service mode enum
 */
export type ServiceMode = 'UNKNOWN' | 'REPLICATED' | 'GLOBAL' | 'REPLICATED_JOB' | 'GLOBAL_JOB';

/**
 * Service port view
 */
export interface ServicePort {
  protocol: string;
  targetPort: number;
  publishedPort: number;
  publishMode: string;
}

/**
 * Update status view
 */
export interface UpdateStatus {
  state: string;
  startedAt: string;
  completedAt: string;
  message: string;
}

/**
 * Update config view
 */
export interface UpdateConfig {
  parallelism: number;
  delayNs: number;
  failureAction: string;
  monitorNs: number;
  maxFailureRatio: number;
  order: string;
}

/**
 * Service placement view
 */
export interface ServicePlacement {
  constraints: string[];
  preferences: Array<{ spreadDescriptor: string }>;
  maxReplicasPerNode: number;
  platforms: Array<{ architecture: string; os: string }>;
}

/**
 * Secret reference view
 */
export interface SecretReference {
  secretId: string;
  secretName: string;
  fileName: string;
  fileUid: string;
  fileGid: string;
  fileMode: number;
}

/**
 * Config reference view
 */
export interface ConfigReference {
  configId: string;
  configName: string;
  fileName: string;
  fileUid: string;
  fileGid: string;
  fileMode: number;
}

/**
 * Service restart policy view
 */
export interface ServiceRestartPolicy {
  condition: string;
  delayNs: number;
  maxAttempts: number;
  windowNs: number;
}

/**
 * Swarm service view
 */
export interface ServiceView {
  id: string;
  name: string;
  image: string;
  mode: ServiceMode;
  replicasDesired: number;
  replicasRunning: number;
  ports: ServicePort[];
  stackNamespace?: string;
  createdAt: string;
  updatedAt: string;
  labels: Array<{ key: string; value: string }>;
  updateStatus?: UpdateStatus;
  placementConstraints: string[];
  networks: string[];
  agentId: string;
  updateConfig?: UpdateConfig;
  rollbackConfig?: UpdateConfig;
  placement?: ServicePlacement;
  secretReferences: SecretReference[];
  configReferences: ConfigReference[];
  restartPolicy?: ServiceRestartPolicy;
}

/**
 * Task state enum
 */
export type TaskState = string;

/**
 * Swarm task view
 */
export interface TaskView {
  id: string;
  serviceId: string;
  serviceName: string;
  nodeId: string;
  slot: number;
  containerId?: string;
  state: TaskState;
  desiredState: string;
  statusMessage: string;
  statusErr?: string;
  createdAt: string;
  updatedAt: string;
  exitCode?: number;
  agentId: string;
}

/**
 * Stack view
 */
export interface StackView {
  namespace: string;
  serviceCount: number;
  replicasDesired: number;
  replicasRunning: number;
  services: ServiceView[];
  agentId: string;
}

/**
 * IPAM config view
 */
export interface IpamConfig {
  subnet: string;
  gateway: string;
  ipRange: string;
}

/**
 * Network service attachment view
 */
export interface NetworkServiceAttachment {
  serviceId: string;
  serviceName: string;
  virtualIp: string;
}

/**
 * Swarm network view
 */
export interface SwarmNetworkView {
  id: string;
  name: string;
  driver: string;
  scope: string;
  isInternal: boolean;
  isAttachable: boolean;
  isIngress: boolean;
  enableIpv6: boolean;
  createdAt: string;
  labels: Array<{ key: string; value: string }>;
  options: Array<{ key: string; value: string }>;
  ipamConfigs: IpamConfig[];
  peers: Array<{ name: string; ip: string }>;
  serviceAttachments: NetworkServiceAttachment[];
  agentId: string;
}

/**
 * Swarm secret view
 */
export interface SwarmSecretView {
  id: string;
  name: string;
  createdAt: string;
  updatedAt: string;
  labels: Array<{ key: string; value: string }>;
  driver: string;
  agentId: string;
}

/**
 * Swarm config view
 */
export interface SwarmConfigView {
  id: string;
  name: string;
  createdAt: string;
  updatedAt: string;
  labels: Array<{ key: string; value: string }>;
  agentId: string;
}

/**
 * Comparison source for replica comparison
 */
export interface ComparisonSource {
  containerId: string;
  serviceId: string;
  taskId: string;
  agentId: string;
  slot: number;
  state: string;
  nodeId: string;
  hostname: string;
}

/**
 * Comparison log event (multi-lane)
 */
export interface ComparisonLogEvent {
  laneIndex: number;
  laneLabel: string;
  content: string;
  syncTimestamp: string;
}

/**
 * Service update event view
 */
export interface ServiceUpdateEvent {
  updateState: string;
  startedAt: string;
  completedAt: string;
  message: string;
  tasksTotal: number;
  tasksRunning: number;
  tasksReady: number;
  tasksFailed: number;
  tasksShutdown: number;
  snapshotAt: string;
  recentChanges: TaskStateChange[];
}

/**
 * Task state change view
 */
export interface TaskStateChange {
  taskId: string;
  serviceId: string;
  nodeId: string;
  slot: number;
  state: string;
  desiredState: string;
  message: string;
  timestamp: string;
  updatedAt: string;
}

/**
 * Node event types
 */
export type NodeEventType = 'UNKNOWN' | 'STATE_CHANGE' | 'AVAILABILITY_CHANGE' | 'ROLE_CHANGE' | 'DRAIN_STARTED' | 'DRAIN_COMPLETED' | 'NODE_DOWN' | 'NODE_READY';

/**
 * Node event view
 */
export interface NodeEvent {
  nodeId: string;
  hostname: string;
  eventType: NodeEventType;
  previousValue: string;
  currentValue: string;
  affectedTasks: TaskView[];
  timestamp: string;
}

/**
 * Service scaling event types
 */
export type ScalingEventType = 'UNKNOWN' | 'SCALED_UP' | 'SCALED_DOWN' | 'UPDATE_STARTED' | 'UPDATE_COMPLETED' | 'UPDATE_ROLLED_BACK' | 'TASK_FAILED' | 'TASK_RECOVERED';

/**
 * Service scaling event
 */
export interface ServiceScalingEvent {
  serviceId: string;
  serviceName: string;
  previousReplicas: number;
  currentReplicas: number;
  eventType: ScalingEventType;
  timestamp: string;
  affectedTasks: TaskView[];
}

/**
 * Service coverage view
 */
export interface ServiceCoverageView {
  coveredNodes: string[];
  uncoveredNodes: string[];
  totalNodes: number;
  coveragePercentage: number;
  serviceId: string;
  isGlobal: boolean;
  agentId: string;
}

/**
 * Service health status enum
 */
export type ServiceHealthStatus = 'UNKNOWN' | 'HEALTHY' | 'DEGRADED' | 'UNHEALTHY';

/**
 * Stack health status enum
 */
export type StackHealthStatus = 'UNKNOWN' | 'HEALTHY' | 'DEGRADED' | 'UNHEALTHY';

/**
 * Service health view
 */
export interface ServiceHealthView {
  serviceId: string;
  serviceName: string;
  status: ServiceHealthStatus;
  replicasDesired: number;
  replicasRunning: number;
  replicasFailed: number;
  recentErrors: string[];
  updateInProgress: boolean;
  restartPolicy?: ServiceRestartPolicy;
}

/**
 * Stack health view
 */
export interface StackHealthView {
  name: string;
  status: StackHealthStatus;
  serviceHealths: ServiceHealthView[];
  totalServices: number;
  healthyServices: number;
  degradedServices: number;
  unhealthyServices: number;
  totalDesired: number;
  totalRunning: number;
  totalFailed: number;
  agentId: string;
}

/**
 * Restart event type enum
 */
export type RestartEventType = 'UNKNOWN' | 'TASK_RESTARTED' | 'CRASH_LOOP' | 'OOM_KILLED';

/**
 * Service restart event
 */
export interface ServiceRestartEvent {
  serviceId: string;
  serviceName: string;
  eventType: RestartEventType;
  newTask?: TaskView;
  oldTask?: TaskView;
  message: string;
  restartCount: number;
  timestamp: string;
  agentId: string;
}

// ============================================================================
// MUTATION RESULT TYPES
// ============================================================================

/**
 * Container action result
 */
export interface ContainerActionResult {
  success: boolean;
  message: string;
  containerId: string;
  newState?: string;
}

/**
 * Image action result
 */
export interface ImageActionResult {
  success: boolean;
  message: string;
}

/**
 * Exec command result
 */
export interface ExecCommandResult {
  exitCode: number;
  stdout: string;
  stderr: string;
  executionTimeMs: number;
  timedOut: boolean;
}

/**
 * Service create result
 */
export interface ServiceCreateResult {
  success: boolean;
  message: string;
  serviceId?: string;
}

/**
 * Service delete result
 */
export interface ServiceDeleteResult {
  success: boolean;
  message: string;
}

/**
 * Service update result
 */
export interface ServiceUpdateResult {
  success: boolean;
  message: string;
}

/**
 * Node update result
 */
export interface NodeUpdateResult {
  success: boolean;
  message: string;
}

/**
 * Swarm log context
 */
export interface SwarmLogContext {
  serviceId: string;
  serviceName: string;
  taskId: string;
  taskSlot: number;
  nodeId: string;
}
