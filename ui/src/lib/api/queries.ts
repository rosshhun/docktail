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
