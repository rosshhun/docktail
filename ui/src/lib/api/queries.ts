/**
 * GraphQL Query Functions for Docktail Cluster API
 * 
 * This module contains all query functions for fetching data from the cluster.
 * Queries are one-time requests that return data immediately (not streaming).
 * 
 * @module api/queries
 */

import { logger } from '../utils/logger';
import { query } from './client';
import type {
  Agent,
  AgentHealthSummary,
  Container,
  ContainerWithAgent,
  ContainerDetails,
  ContainerStats,
  LogEvent,
  HealthStatus,
  DiscoveryStatus,
  SwarmInfo,
  NodeView,
  ServiceView,
  TaskView,
  StackView,
  SwarmNetworkView,
  SwarmSecretView,
  SwarmConfigView,
  ComparisonSource,
  ServiceCoverageView,
  StackHealthView,
  ContainerActionResult,
  ImageActionResult,
  ExecCommandResult,
  ServiceCreateResult,
  ServiceDeleteResult,
  ServiceUpdateResult,
  NodeUpdateResult,
} from './types';

// ============================================================================
// CLUSTER QUERIES
// ============================================================================

/**
 * Fetch cluster health status
 * 
 * @returns Health status object with status string and timestamp
 * 
 * @example
 * ```typescript
 * const health = await fetchHealth();
 * console.log(health.status); // "healthy"
 * ```
 */
export async function fetchHealth(): Promise<HealthStatus> {
  const result = await query<{ health: HealthStatus }>(`
    query GetHealth {
      health {
        status
        timestamp
      }
    }
  `);
  
  return result.health;
}

/**
 * Fetch cluster version information
 * 
 * @returns Version string (e.g., "0.2.0")
 * 
 * @example
 * ```typescript
 * const version = await fetchVersion();
 * console.log(`Cluster version: ${version}`);
 * ```
 */
export async function fetchVersion(): Promise<string> {
  const result = await query<{ version: string }>(`
    query GetVersion {
      version
    }
  `);
  
  return result.version;
}

// ============================================================================
// AGENT QUERIES
// ============================================================================

/**
 * Fetch all agents or specific agents by ID
 * 
 * @param ids - Optional array of agent IDs to fetch. If omitted, returns all agents.
 * @returns Array of agents with full details
 * 
 * @example
 * ```typescript
 * // Fetch all agents
 * const allAgents = await fetchAgents();
 * 
 * // Fetch specific agents
 * const agents = await fetchAgents(['agent-1', 'agent-2']);
 * ```
 */
export async function fetchAgents(ids?: string[]): Promise<Agent[]> {
  const result = await query<{ agents: Agent[] }>(`
    query GetAgents($ids: [String!]) {
      agents(ids: $ids) {
        id
        name
        status
        address
        lastSeen
        version
        labels {
          key
          value
        }
      }
    }
  `, ids ? { ids } : undefined);
  
  return result.agents;
}

/**
 * Fetch a single agent by ID
 * 
 * @param id - Agent ID
 * @returns Agent object or null if not found
 * 
 * @example
 * ```typescript
 * const agent = await fetchAgent('agent-local');
 * if (agent) {
 *   console.log(`Agent ${agent.name} is ${agent.status}`);
 * }
 * ```
 */
export async function fetchAgent(id: string): Promise<Agent | null> {
  const result = await query<{ agent: Agent | null }>(`
    query GetAgent($id: String!) {
      agent(id: $id) {
        id
        name
        status
        address
        lastSeen
        version
        labels {
          key
          value
        }
      }
    }
  `, { id });
  
  return result.agent;
}

/**
 * Fetch aggregated health summary for all agents
 * 
 * @returns Summary with counts by health status
 * 
 * @example
 * ```typescript
 * const health = await fetchAgentHealth();
 * console.log(`${health.healthy}/${health.total} agents healthy`);
 * ```
 */
export async function fetchAgentHealth(): Promise<AgentHealthSummary> {
  const result = await query<{ agentHealth: AgentHealthSummary }>(`
    query GetAgentHealth {
      agentHealth {
        total
        healthy
        degraded
        unhealthy
        unknown
      }
    }
  `);
  
  return result.agentHealth;
}

// ============================================================================
// CONTAINER QUERIES
// ============================================================================

/**
 * Fetch all containers across all agents with agent names resolved
 * 
 * This function fetches containers and agents in parallel, then merges
 * the data to include human-readable agent names.
 * 
 * @returns Array of containers with agent names
 * 
 * @example
 * ```typescript
 * const containers = await fetchContainers();
 * containers.forEach(c => {
 *   console.log(`${c.name} on ${c.agentName}: ${c.state}`);
 * });
 * ```
 */
export async function fetchContainers(): Promise<ContainerWithAgent[]> {
  const [containersResult, agentsResult] = await Promise.all([
    query<{ containers: Container[] }>(`
      query GetContainers {
        containers(filter: { includeStopped: true }) {
          id
          name
          image
          state
          agentId
          status
          createdAt
          logDriver
          labels {
            key
            value
          }
          ports {
            containerPort
            protocol
            hostIp
            hostPort
          }
        }
      }
    `),
    query<{ agents: Agent[] }>(`
      query GetAgents {
        agents {
          id
          name
        }
      }
    `)
  ]);
  
  const agentMap = new Map(agentsResult.agents.map(a => [a.id, a.name]));
  
  logger.debug('[fetchContainers] Agent map:', Array.from(agentMap.entries()));
  
  // Deduplicate containers by ID (in case backend returns duplicates)
  const uniqueContainers = new Map<string, ContainerWithAgent>();
  
  for (const container of containersResult.containers) {
    if (!uniqueContainers.has(container.id)) {
      const agentName = agentMap.get(container.agentId) || container.agentId;
      logger.debug(`[fetchContainers] Container ${container.name}: agentId=${container.agentId}, mapped to agentName=${agentName}`);
      uniqueContainers.set(container.id, {
        ...container,
        agentName
      });
    }
  }
  
  return Array.from(uniqueContainers.values());
}

/**
 * Fetch a single container by ID
 * 
 * @param id - Container ID
 * @param agentId - Optional agent ID to narrow the search
 * @returns Container object or null if not found
 * 
 * @example
 * ```typescript
 * const container = await fetchContainer('abc123', 'agent-local');
 * ```
 */
export async function fetchContainer(id: string, agentId?: string): Promise<Container | null> {
  const result = await query<{ container: Container | null }>(`
    query GetContainer($id: String!, $agentId: String) {
      container(id: $id, agentId: $agentId) {
        id
        name
        image
        state
        agentId
        status
        createdAt
        logDriver
        labels {
          key
          value
        }
        ports {
          containerPort
          protocol
          hostIp
          hostPort
        }
        stateInfo {
          oomKilled
          pid
          exitCode
          startedAt
          finishedAt
          restartCount
        }
      }
    }
  `, { id, agentId });
  
  return result.container;
}

/**
 * Fetch detailed container configuration
 * 
 * Retrieves extended information including environment variables,
 * mounts, networks, and resource limits.
 * 
 * @param id - Container ID
 * @param agentId - Optional agent ID to narrow the search
 * @returns Container details or null if not found
 * 
 * @example
 * ```typescript
 * const details = await fetchContainerDetails('abc123', 'agent-local');
 * if (details) {
 *   console.log('Exposed ports:', details.exposedPorts);
 *   console.log('Environment:', details.env);
 * }
 * ```
 */
export async function fetchContainerDetails(id: string, agentId?: string): Promise<ContainerDetails | null> {
  const result = await query<{ container: { details?: ContainerDetails } | null }>(`
    query GetContainerDetails($id: String!, $agentId: String) {
      container(id: $id, agentId: $agentId) {
        details {
          command
          workingDir
          env
          exposedPorts
          entrypoint
          hostname
          user
          networkMode
          platform
          runtime
          mounts {
            source
            destination
            mode
            mountType
            propagation
          }
          networks {
            networkName
            ipAddress
            gateway
            macAddress
          }
          limits {
            memoryLimitBytes
            cpuLimit
            pidsLimit
          }
          restartPolicy {
            name
            maxRetryCount
          }
          healthcheck {
            test
            intervalNs
            timeoutNs
            retries
            startPeriodNs
          }
        }
      }
    }
  `, { id, agentId });
  
  return result.container?.details || null;
}

/**
 * Fetch a single snapshot of container resource statistics
 * 
 * For real-time streaming stats, use `subscribeToContainerStats` instead.
 * 
 * @param containerId - Container ID
 * @param agentId - Agent ID where the container is running
 * @returns Container stats or null if not available
 * 
 * @example
 * ```typescript
 * const stats = await fetchContainerStats('abc123', 'agent-local');
 * if (stats) {
 *   console.log(`CPU: ${stats.cpuStats.cpuPercentage.toFixed(2)}%`);
 *   console.log(`Memory: ${stats.memoryStats.percentage.toFixed(2)}%`);
 * }
 * ```
 */
export async function fetchContainerStats(containerId: string, agentId: string): Promise<ContainerStats | null> {
  const result = await query<{ containerStats: ContainerStats | null }>(`
    query GetContainerStats($id: String!, $agentId: String!) {
      containerStats(id: $id, agentId: $agentId) {
        containerId
        timestamp
        cpuStats {
          cpuPercentage
          totalUsage
          systemUsage
          onlineCpus
          perCpuUsage
          throttling {
            throttledPeriods
            totalPeriods
            throttledTime
          }
        }
        memoryStats {
          usage
          maxUsage
          limit
          percentage
          cache
          rss
          swap
        }
        networkStats {
          interfaceName
          rxBytes
          rxPackets
          rxErrors
          rxDropped
          txBytes
          txPackets
          txErrors
          txDropped
        }
        blockIoStats {
          readBytes
          writeBytes
          readOps
          writeOps
          devices {
            major
            minor
            readBytes
            writeBytes
          }
        }
        pidsCount
      }
    }
  `, { id: containerId, agentId });
  
  return result.containerStats;
}

// ============================================================================
// LOG QUERIES
// ============================================================================

/**
 * Fetch historical logs from a container (non-streaming, paginated)
 * 
 * For real-time streaming logs, use `subscribeToLogs` instead.
 * 
 * @param containerId - Container ID
 * @param agentId - Agent ID where the container is running
 * @param options - Optional query options
 * @param options.tail - Number of lines from the end (default: 100)
 * @param options.since - Start time (ISO-8601 timestamp)
 * @param options.until - End time (ISO-8601 timestamp)
 * @param options.filter - Filter pattern (regex or substring)
 * @param options.filterMode - Filter mode ('NONE', 'INCLUDE', 'EXCLUDE')
 * @returns Array of log entries
 * 
 * @example
 * ```typescript
 * // Get last 100 lines
 * const logs = await fetchHistoricalLogs('abc123', 'agent-local');
 * 
 * // Get logs with filter
 * const errorLogs = await fetchHistoricalLogs('abc123', 'agent-local', {
 *   tail: 500,
 *   filter: 'ERROR',
 *   filterMode: 'INCLUDE'
 * });
 * ```
 */
export async function fetchHistoricalLogs(
  containerId: string, 
  agentId: string,
  options?: {
    tail?: number;
    since?: string;
    until?: string;
    filter?: string;
    filterMode?: 'NONE' | 'INCLUDE' | 'EXCLUDE';
  }
): Promise<LogEvent[]> {
  const result = await query<{ logs: LogEvent[] }>(`
    query GetLogs($containerId: String!, $agentId: String!, $options: LogStreamOptions) {
      logs(containerId: $containerId, agentId: $agentId, options: $options) {
        containerId
        agentId
        timestamp
        content
        level
        sequence
        format
        parseSuccess
        groupedLines {
          content
          timestamp
          sequence
        }
        lineCount
        isGrouped
        parsed {
          level
          message
          logger
          timestamp
          request {
            method
            path
            remoteAddr
            statusCode
            durationMs
            requestId
          }
          error {
            errorType
            errorMessage
            stackTrace
            file
            line
          }
          fields {
            key
            value
          }
        }
      }
    }
  `, { 
    containerId, 
    agentId, 
    options: options || { tail: 100 }
  });
  
  return result.logs;
}

// ============================================================================
// DISCOVERY QUERIES
// ============================================================================

export async function fetchDiscoveryStatus(): Promise<DiscoveryStatus> {
  const result = await query<{ discoveryStatus: DiscoveryStatus }>(`
    query GetDiscoveryStatus {
      discoveryStatus {
        swarmDiscoveryEnabled
        registrationEnabled
        discoveryLabel
        discoveryIntervalSecs
        agentPort
        totalAgents
        staticAgents
        discoveredAgents
        registeredAgents
      }
    }
  `);
  return result.discoveryStatus;
}

// ============================================================================
// SWARM QUERIES
// ============================================================================

export async function fetchSwarmInfo(agentId: string): Promise<SwarmInfo | null> {
  const result = await query<{ swarmInfo: SwarmInfo | null }>(`
    query GetSwarmInfo($agentId: String!) {
      swarmInfo(agentId: $agentId) {
        swarmId
        nodeId
        isManager
        managers
        workers
        isSwarmMode
      }
    }
  `, { agentId });
  return result.swarmInfo;
}

export async function fetchNodes(agentId: string): Promise<NodeView[]> {
  const result = await query<{ nodes: NodeView[] }>(`
    query GetNodes($agentId: String!) {
      nodes(agentId: $agentId) {
        id
        hostname
        role
        availability
        status
        addr
        engineVersion
        os
        architecture
        labels { key value }
        managerStatus { leader reachability addr }
        nanoCpus
        memoryBytes
        agentId
      }
    }
  `, { agentId });
  return result.nodes;
}

export async function fetchNode(nodeId: string, agentId: string): Promise<NodeView | null> {
  const result = await query<{ node: NodeView | null }>(`
    query GetNode($nodeId: String!, $agentId: String!) {
      node(nodeId: $nodeId, agentId: $agentId) {
        id
        hostname
        role
        availability
        status
        addr
        engineVersion
        os
        architecture
        labels { key value }
        managerStatus { leader reachability addr }
        nanoCpus
        memoryBytes
        agentId
      }
    }
  `, { nodeId, agentId });
  return result.node;
}

export async function fetchServices(agentId: string): Promise<ServiceView[]> {
  const result = await query<{ services: ServiceView[] }>(`
    query GetServices($agentId: String!) {
      services(agentIds: [$agentId]) {
        id
        name
        image
        mode
        replicasDesired
        replicasRunning
        ports { protocol targetPort publishedPort publishMode }
        stackNamespace
        createdAt
        updatedAt
        labels { key value }
        updateStatus { state startedAt completedAt message }
        placementConstraints
        networks
        agentId
        updateConfig { parallelism delayNs failureAction monitorNs maxFailureRatio order }
        rollbackConfig { parallelism delayNs failureAction monitorNs maxFailureRatio order }
        placement { constraints preferences { spreadDescriptor } maxReplicasPerNode platforms { architecture os } }
        secretReferences { secretId secretName fileName fileUid fileGid fileMode }
        configReferences { configId configName fileName fileUid fileGid fileMode }
        restartPolicy { condition delayNs maxAttempts windowNs }
      }
    }
  `, { agentId });
  return result.services;
}

export async function fetchService(serviceId: string, agentId: string): Promise<ServiceView | null> {
  const result = await query<{ service: ServiceView | null }>(`
    query GetService($serviceId: String!, $agentId: String!) {
      service(serviceId: $serviceId, agentId: $agentId) {
        id
        name
        image
        mode
        replicasDesired
        replicasRunning
        ports { protocol targetPort publishedPort publishMode }
        stackNamespace
        createdAt
        updatedAt
        labels { key value }
        updateStatus { state startedAt completedAt message }
        placementConstraints
        networks
        agentId
        updateConfig { parallelism delayNs failureAction monitorNs maxFailureRatio order }
        rollbackConfig { parallelism delayNs failureAction monitorNs maxFailureRatio order }
        placement { constraints preferences { spreadDescriptor } maxReplicasPerNode platforms { architecture os } }
        secretReferences { secretId secretName fileName fileUid fileGid fileMode }
        configReferences { configId configName fileName fileUid fileGid fileMode }
        restartPolicy { condition delayNs maxAttempts windowNs }
      }
    }
  `, { serviceId, agentId });
  return result.service;
}

export async function fetchTasks(serviceId: string, agentId: string): Promise<TaskView[]> {
  const result = await query<{ tasks: TaskView[] }>(`
    query GetTasks($serviceId: String!, $agentId: String!) {
      tasks(serviceId: $serviceId, agentId: $agentId) {
        id
        serviceId
        serviceName
        nodeId
        slot
        containerId
        state
        desiredState
        statusMessage
        statusErr
        createdAt
        updatedAt
        exitCode
        agentId
      }
    }
  `, { serviceId, agentId });
  return result.tasks;
}

export async function fetchStacks(agentId: string): Promise<StackView[]> {
  const result = await query<{ stacks: StackView[] }>(`
    query GetStacks($agentId: String!) {
      stacks(agentIds: [$agentId]) {
        namespace
        serviceCount
        replicasDesired
        replicasRunning
        services {
          id
          name
          image
          mode
          replicasDesired
          replicasRunning
          ports { protocol targetPort publishedPort publishMode }
          stackNamespace
          createdAt
          updatedAt
          labels { key value }
          agentId
        }
        agentId
      }
    }
  `, { agentId });
  return result.stacks;
}

export async function fetchStack(stackName: string, agentId: string): Promise<StackView | null> {
  const result = await query<{ stack: StackView | null }>(`
    query GetStack($stackName: String!, $agentId: String!) {
      stack(stackName: $stackName, agentId: $agentId) {
        namespace
        serviceCount
        replicasDesired
        replicasRunning
        services {
          id
          name
          image
          mode
          replicasDesired
          replicasRunning
          ports { protocol targetPort publishedPort publishMode }
          stackNamespace
          createdAt
          updatedAt
          labels { key value }
          updateStatus { state startedAt completedAt message }
          agentId
          restartPolicy { condition delayNs maxAttempts windowNs }
        }
        agentId
      }
    }
  `, { stackName, agentId });
  return result.stack;
}

export async function fetchSwarmNetworks(agentId: string, swarmOnly = true): Promise<SwarmNetworkView[]> {
  const result = await query<{ swarmNetworks: SwarmNetworkView[] }>(`
    query GetSwarmNetworks($agentId: String!, $swarmOnly: Boolean) {
      swarmNetworks(agentIds: [$agentId], swarmOnly: $swarmOnly) {
        id
        name
        driver
        scope
        isInternal
        isAttachable
        isIngress
        enableIpv6
        createdAt
        labels { key value }
        options { key value }
        ipamConfigs { subnet gateway ipRange }
        peers { name ip }
        serviceAttachments { serviceId serviceName virtualIp }
        agentId
      }
    }
  `, { agentId, swarmOnly });
  return result.swarmNetworks;
}

export async function fetchSwarmSecrets(agentId: string): Promise<SwarmSecretView[]> {
  const result = await query<{ swarmSecrets: SwarmSecretView[] }>(`
    query GetSwarmSecrets($agentId: String!) {
      swarmSecrets(agentIds: [$agentId]) {
        id
        name
        createdAt
        updatedAt
        labels { key value }
        driver
        agentId
      }
    }
  `, { agentId });
  return result.swarmSecrets;
}

export async function fetchSwarmConfigs(agentId: string): Promise<SwarmConfigView[]> {
  const result = await query<{ swarmConfigs: SwarmConfigView[] }>(`
    query GetSwarmConfigs($agentId: String!) {
      swarmConfigs(agentIds: [$agentId]) {
        id
        name
        createdAt
        updatedAt
        labels { key value }
        agentId
      }
    }
  `, { agentId });
  return result.swarmConfigs;
}

export async function fetchServiceReplicas(serviceId: string, agentId: string): Promise<ComparisonSource[]> {
  const result = await query<{ serviceReplicas: ComparisonSource[] }>(`
    query GetServiceReplicas($serviceId: String!, $agentId: String!) {
      serviceReplicas(serviceId: $serviceId, agentId: $agentId) {
        containerId
        serviceId
        taskId
        agentId
        slot
        state
        nodeId
        hostname
      }
    }
  `, { serviceId, agentId });
  return result.serviceReplicas;
}

export async function fetchServiceCoverage(serviceId: string, agentId: string): Promise<ServiceCoverageView> {
  const result = await query<{ serviceCoverage: ServiceCoverageView }>(`
    query GetServiceCoverage($serviceId: String!, $agentId: String!) {
      serviceCoverage(serviceId: $serviceId, agentId: $agentId) {
        coveredNodes
        uncoveredNodes
        totalNodes
        coveragePercentage
        serviceId
        isGlobal
        agentId
      }
    }
  `, { serviceId, agentId });
  return result.serviceCoverage;
}

export async function fetchStackHealth(stackName: string, agentId: string): Promise<StackHealthView> {
  const result = await query<{ stackHealth: StackHealthView }>(`
    query GetStackHealth($stackName: String!, $agentId: String!) {
      stackHealth(stackName: $stackName, agentId: $agentId) {
        name
        status
        serviceHealths {
          serviceId
          serviceName
          status
          replicasDesired
          replicasRunning
          replicasFailed
          recentErrors
          updateInProgress
          restartPolicy { condition delayNs maxAttempts windowNs }
        }
        totalServices
        healthyServices
        degradedServices
        unhealthyServices
        totalDesired
        totalRunning
        totalFailed
        agentId
      }
    }
  `, { stackName, agentId });
  return result.stackHealth;
}

// ============================================================================
// CONTAINER MUTATIONS
// ============================================================================

export async function startContainer(containerId: string, agentId: string): Promise<ContainerActionResult> {
  const result = await query<{ startContainer: ContainerActionResult }>(`
    mutation StartContainer($input: ContainerActionInput!) {
      startContainer(input: $input) {
        success message containerId newState
      }
    }
  `, { input: { containerId, agentId } });
  return result.startContainer;
}

export async function stopContainer(containerId: string, agentId: string): Promise<ContainerActionResult> {
  const result = await query<{ stopContainer: ContainerActionResult }>(`
    mutation StopContainer($input: ContainerActionInput!) {
      stopContainer(input: $input) {
        success message containerId newState
      }
    }
  `, { input: { containerId, agentId } });
  return result.stopContainer;
}

export async function restartContainer(containerId: string, agentId: string): Promise<ContainerActionResult> {
  const result = await query<{ restartContainer: ContainerActionResult }>(`
    mutation RestartContainer($input: ContainerActionInput!) {
      restartContainer(input: $input) {
        success message containerId newState
      }
    }
  `, { input: { containerId, agentId } });
  return result.restartContainer;
}

export async function pauseContainer(containerId: string, agentId: string): Promise<ContainerActionResult> {
  const result = await query<{ pauseContainer: ContainerActionResult }>(`
    mutation PauseContainer($input: ContainerActionInput!) {
      pauseContainer(input: $input) {
        success message containerId newState
      }
    }
  `, { input: { containerId, agentId } });
  return result.pauseContainer;
}

export async function unpauseContainer(containerId: string, agentId: string): Promise<ContainerActionResult> {
  const result = await query<{ unpauseContainer: ContainerActionResult }>(`
    mutation UnpauseContainer($input: ContainerActionInput!) {
      unpauseContainer(input: $input) {
        success message containerId newState
      }
    }
  `, { input: { containerId, agentId } });
  return result.unpauseContainer;
}

export async function removeContainer(containerId: string, agentId: string, force = false): Promise<ContainerActionResult> {
  const result = await query<{ removeContainer: ContainerActionResult }>(`
    mutation RemoveContainer($input: ContainerRemoveInput!) {
      removeContainer(input: $input) {
        success message containerId newState
      }
    }
  `, { input: { containerId, agentId, force } });
  return result.removeContainer;
}

// ============================================================================
// IMAGE MUTATIONS
// ============================================================================

export async function pullImage(image: string, tag: string, agentId: string): Promise<ImageActionResult> {
  const result = await query<{ pullImage: ImageActionResult }>(`
    mutation PullImage($input: ImagePullInput!) {
      pullImage(input: $input) {
        success message
      }
    }
  `, { input: { image, tag, agentId } });
  return result.pullImage;
}

export async function removeImage(imageId: string, agentId: string, force = false): Promise<ImageActionResult> {
  const result = await query<{ removeImage: ImageActionResult }>(`
    mutation RemoveImage($input: ImageRemoveInput!) {
      removeImage(input: $input) {
        success message
      }
    }
  `, { input: { imageId, agentId, force } });
  return result.removeImage;
}

// ============================================================================
// EXEC MUTATIONS
// ============================================================================

export async function execCommand(
  containerId: string,
  agentId: string,
  command: string[],
  options?: { workingDir?: string; env?: string[]; timeout?: number }
): Promise<ExecCommandResult> {
  const result = await query<{ execCommand: ExecCommandResult }>(`
    mutation ExecCommand($input: ExecCommandInput!) {
      execCommand(input: $input) {
        exitCode stdout stderr executionTimeMs timedOut
      }
    }
  `, { input: { containerId, agentId, command, ...options } });
  return result.execCommand;
}

// ============================================================================
// SERVICE MUTATIONS
// ============================================================================

export async function updateServiceReplicas(serviceId: string, agentId: string, replicas: number): Promise<ServiceUpdateResult> {
  const result = await query<{ updateService: ServiceUpdateResult }>(`
    mutation UpdateService($input: ServiceUpdateInput!) {
      updateService(input: $input) {
        success message
      }
    }
  `, { input: { serviceId, agentId, replicas } });
  return result.updateService;
}

export async function deleteService(serviceId: string, agentId: string): Promise<ServiceDeleteResult> {
  const result = await query<{ deleteService: ServiceDeleteResult }>(`
    mutation DeleteService($input: ServiceDeleteInput!) {
      deleteService(input: $input) {
        success message
      }
    }
  `, { input: { serviceId, agentId } });
  return result.deleteService;
}

// ============================================================================
// NODE MUTATIONS
// ============================================================================

export async function updateNode(
  nodeId: string,
  agentId: string,
  options: { availability?: string; role?: string; labels?: Array<{ key: string; value: string }> }
): Promise<NodeUpdateResult> {
  const result = await query<{ updateNode: NodeUpdateResult }>(`
    mutation UpdateNode($input: NodeUpdateInput!) {
      updateNode(input: $input) {
        success message
      }
    }
  `, { input: { nodeId, agentId, ...options } });
  return result.updateNode;
}
