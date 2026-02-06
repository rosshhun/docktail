/**
 * GraphQL Subscription Functions for Docktail Cluster API
 * 
 * This module contains all subscription functions for real-time data streams.
 * Subscriptions use WebSocket connections to receive live updates.
 * 
 * @module api/subscriptions
 */

import { logger } from '../utils/logger';
import { GraphQLError } from './errors';
import type {
  LogEvent,
  AgentHealthEvent,
  ContainerStats,
} from './types';

/** GraphQL WebSocket endpoint */
// In development, the UI usually runs on 5173/3000 but the API on 8080
const GRAPHQL_WS_ENDPOINT = (() => {
  if (typeof window === 'undefined') return 'ws://localhost:8080/ws';
  
  const { hostname, port, protocol } = window.location;
  const wsProtocol = protocol === 'https:' ? 'wss:' : 'ws:';
  
  // If we're on localhost but not on the standard API port, use 8080
  if (hostname === 'localhost' && port !== '8080') {
    return 'ws://localhost:8080/ws';
  }
  
  // Otherwise use the current host (works for production/containers)
  return `${wsProtocol}//${window.location.host}/ws`;
})();

// ============================================================================
// LOG SUBSCRIPTIONS
// ============================================================================

/**
 * Subscribe to real-time logs from a single container
 * 
 * Opens a WebSocket connection and streams log entries as they are generated.
 * Automatically handles connection initialization and cleanup.
 * 
 * @param containerId - Container ID to stream logs from
 * @param agentId - Agent ID where the container is running
 * @param onMessage - Callback invoked for each log entry
 * @param onError - Optional callback invoked on errors
 * @returns Cleanup function to close the subscription
 * 
 * @example
 * ```typescript
 * const unsubscribe = subscribeToLogs(
 *   'abc123',
 *   'agent-local',
 *   (log) => {
 *     console.log(`[${log.level}] ${log.content}`);
 *   },
 *   (error) => {
 *     console.error('Log stream error:', error);
 *   }
 * );
 * 
 * // Later, to stop streaming:
 * unsubscribe();
 * ```
 */
export function subscribeToLogs(
  containerId: string,
  agentId: string,
  onMessage: (log: LogEvent) => void,
  onError?: (error: Error) => void
): () => void {
  // Use graphql-transport-ws protocol
  const ws = new WebSocket(GRAPHQL_WS_ENDPOINT, 'graphql-transport-ws');
  
  ws.onopen = () => {
    logger.debug('WebSocket opened, sending connection_init');
    // graphql-transport-ws protocol initialization
    ws.send(JSON.stringify({ type: 'connection_init', payload: {} }));
  };

  ws.onmessage = (event) => {
    const message = JSON.parse(event.data);
    logger.debug('WebSocket message:', message);
    
    if (message.type === 'connection_ack') {
      logger.debug('Connection acknowledged, subscribing to logs');
      // Send subscription request using graphql-transport-ws protocol
      ws.send(JSON.stringify({
        id: '1',
        type: 'subscribe',
        payload: {
          query: `
            subscription StreamLogs($containerId: String!, $agentId: String!) {
              logStream(containerId: $containerId, agentId: $agentId, options: { follow: true, tail: 100 }) {
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
          `,
          variables: { containerId, agentId },
        },
      }));
    } else if (message.type === 'next') {
      // graphql-transport-ws uses 'next' for data
      if (message.payload.errors && message.payload.errors.length > 0) {
        logger.error('GraphQL subscription errors:', message.payload.errors);
        const firstError = message.payload.errors[0];
        const errorCode = firstError.extensions?.code;
        const errorMessage = firstError.message || 'GraphQL subscription error';
        onError?.(new GraphQLError(errorMessage, errorCode, firstError));
        return;
      }
      
      if (message.payload.data?.logStream) {
        const logEvent: LogEvent = {
          containerId: message.payload.data.logStream.containerId,
          agentId: message.payload.data.logStream.agentId,
          timestamp: message.payload.data.logStream.timestamp,
          content: message.payload.data.logStream.content,
          level: message.payload.data.logStream.level.toUpperCase() as 'STDOUT' | 'STDERR',
          sequence: message.payload.data.logStream.sequence,
          format: message.payload.data.logStream.format,
          parseSuccess: message.payload.data.logStream.parseSuccess,
          groupedLines: message.payload.data.logStream.groupedLines || [],
          lineCount: message.payload.data.logStream.lineCount || 1,
          isGrouped: message.payload.data.logStream.isGrouped || false,
          parsed: message.payload.data.logStream.parsed
        };
        onMessage(logEvent);
      }
    } else if (message.type === 'error') {
      logger.error('Subscription error:', message.payload);
      const firstError = message.payload[0];
      const errorCode = firstError?.extensions?.code;
      const errorMessage = firstError?.message || 'Subscription error';
      onError?.(new GraphQLError(errorMessage, errorCode, firstError));
    } else if (message.type === 'complete') {
      logger.debug('Subscription completed');
    }
  };

  ws.onerror = (error) => {
    logger.error('WebSocket error:', error);
    onError?.(new GraphQLError('WebSocket connection error', 'WEBSOCKET_ERROR'));
  };

  ws.onclose = () => {
    logger.debug('WebSocket closed');
  };

  // Return cleanup function
  return () => {
    if (ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify({ id: '1', type: 'complete' }));
      ws.close();
    }
  };
}

/**
 * Subscribe to aggregated logs from multiple containers
 * 
 * Opens a single WebSocket connection that streams logs from multiple
 * containers across potentially different agents. Logs are roughly
 * ordered by timestamp.
 * 
 * @param containers - Array of container sources (container ID + agent ID)
 * @param onMessage - Callback invoked for each log entry
 * @param onError - Optional callback invoked on errors
 * @returns Cleanup function to close the subscription
 * 
 * @example
 * ```typescript
 * const unsubscribe = subscribeToMultipleContainerLogs(
 *   [
 *     { containerId: 'abc123', agentId: 'agent-1' },
 *     { containerId: 'def456', agentId: 'agent-2' }
 *   ],
 *   (log) => {
 *     console.log(`[${log.containerId}] ${log.content}`);
 *   },
 *   (error) => console.error('Error:', error)
 * );
 * ```
 */
export function subscribeToMultipleContainerLogs(
  containers: Array<{ containerId: string; agentId: string }>,
  onMessage: (log: LogEvent) => void,
  onError?: (error: Error) => void
): () => void {
  const ws = new WebSocket(GRAPHQL_WS_ENDPOINT, 'graphql-transport-ws');
  
  ws.onopen = () => {
    logger.debug('WebSocket opened for multi-container logs');
    ws.send(JSON.stringify({ type: 'connection_init', payload: {} }));
  };

  ws.onmessage = (event) => {
    const message = JSON.parse(event.data);
    
    if (message.type === 'connection_ack') {
      logger.debug('Connection acknowledged, subscribing to multiple container logs');
      ws.send(JSON.stringify({
        id: '1',
        type: 'subscribe',
        payload: {
          query: `
            subscription StreamMultipleLogs($containers: [ContainerSource!]!) {
              logsFromContainers(containers: $containers, options: { follow: true, tail: 50 }) {
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
          `,
          variables: { containers },
        },
      }));
    } else if (message.type === 'next') {
      if (message.payload.errors && message.payload.errors.length > 0) {
        logger.error('GraphQL subscription errors:', message.payload.errors);
        const firstError = message.payload.errors[0];
        const errorCode = firstError.extensions?.code;
        const errorMessage = firstError.message || 'GraphQL subscription error';
        onError?.(new GraphQLError(errorMessage, errorCode, firstError));
        return;
      }
      
      if (message.payload.data?.logsFromContainers) {
        const logData = message.payload.data.logsFromContainers;
        const logEvent: LogEvent = {
          containerId: logData.containerId,
          agentId: logData.agentId,
          timestamp: logData.timestamp,
          content: logData.content,
          level: logData.level.toUpperCase() as 'STDOUT' | 'STDERR',
          sequence: logData.sequence,
          format: logData.format,
          parseSuccess: logData.parseSuccess,
          groupedLines: logData.groupedLines || [],
          lineCount: logData.lineCount || 1,
          isGrouped: logData.isGrouped || false,
          parsed: logData.parsed
        };
        onMessage(logEvent);
      }
    } else if (message.type === 'error') {
      logger.error('Subscription error:', message.payload);
      const firstError = message.payload[0];
      const errorCode = firstError?.extensions?.code;
      const errorMessage = firstError?.message || 'Subscription error';
      onError?.(new GraphQLError(errorMessage, errorCode, firstError));
    }
  };

  ws.onerror = (error) => {
    logger.error('WebSocket error:', error);
    onError?.(new GraphQLError('WebSocket connection error', 'WEBSOCKET_ERROR'));
  };

  ws.onclose = () => {
    logger.debug('WebSocket closed');
  };

  return () => {
    if (ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify({ id: '1', type: 'complete' }));
      ws.close();
    }
  };
}

// ============================================================================
// AGENT HEALTH SUBSCRIPTIONS
// ============================================================================

/**
 * Subscribe to real-time agent health updates
 * 
 * Receives periodic health check results from the agent, including
 * parsing metrics and status changes. Updates every ~5 seconds.
 * 
 * @param agentId - Agent ID to monitor
 * @param onMessage - Callback invoked for each health event
 * @param onError - Optional callback invoked on errors
 * @returns Cleanup function to close the subscription
 * 
 * @example
 * ```typescript
 * const unsubscribe = subscribeToAgentHealth(
 *   'agent-local',
 *   (event) => {
 *     console.log(`Agent ${event.agentId}: ${event.status}`);
 *     console.log('Metadata:', event.metadata);
 *   },
 *   (error) => console.error('Health stream error:', error)
 * );
 * ```
 */
export function subscribeToAgentHealth(
  agentId: string,
  onMessage: (event: AgentHealthEvent) => void,
  onError?: (error: Error) => void
): () => void {
  const ws = new WebSocket(GRAPHQL_WS_ENDPOINT, 'graphql-transport-ws');

  ws.onopen = () => {
    logger.debug('Agent health WebSocket opened');
    // Initialize connection
    ws.send(JSON.stringify({ type: 'connection_init' }));
  };

  ws.onmessage = (event) => {
    const message = JSON.parse(event.data);

    if (message.type === 'connection_ack') {
      logger.debug('Agent health connection acknowledged');
      const subscriptionQuery = `
        subscription AgentHealth($agentId: String!) {
          agentHealthStream(agentId: $agentId) {
            agentId
            status
            message
            timestamp
            metadata {
              key
              value
            }
          }
        }
      `;

      ws.send(JSON.stringify({
        id: '1',
        type: 'subscribe',
        payload: { 
          query: subscriptionQuery,
          variables: { agentId }
        }
      }));
    } else if (message.type === 'next' && message.payload?.data) {
      if (message.payload.errors) {
        const firstError = message.payload.errors[0];
        const errorCode = firstError.extensions?.code;
        const errorMessage = firstError.message || 'GraphQL subscription error';
        onError?.(new GraphQLError(errorMessage, errorCode, firstError));
        return;
      }

      if (message.payload.data?.agentHealthStream) {
        onMessage(message.payload.data.agentHealthStream);
      }
    } else if (message.type === 'error') {
      logger.error('Health subscription error:', message.payload);
      const firstError = Array.isArray(message.payload) ? message.payload[0] : message.payload;
      const errorCode = firstError?.extensions?.code;
      const errorMessage = firstError?.message || 'Health subscription error';
      onError?.(new GraphQLError(errorMessage, errorCode, firstError));
    }
  };

  ws.onerror = (error) => {
    logger.error('WebSocket error:', error);
    onError?.(new GraphQLError('WebSocket connection error', 'WEBSOCKET_ERROR'));
  };

  ws.onclose = () => {
    logger.debug('Health stream closed');
  };

  return () => {
    if (ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify({ id: '1', type: 'complete' }));
      ws.close();
    }
  };
}

// ============================================================================
// CONTAINER STATS SUBSCRIPTIONS
// ============================================================================

/**
 * Subscribe to real-time container resource statistics
 * 
 * Receives continuous stream of resource metrics (CPU, memory, network, disk)
 * from a running container. Updates every ~1 second.
 * 
 * @param containerId - Container ID to monitor
 * @param agentId - Agent ID where the container is running
 * @param onMessage - Callback invoked for each stats update
 * @param onError - Optional callback invoked on errors
 * @returns Cleanup function to close the subscription
 * 
 * @example
 * ```typescript
 * const unsubscribe = subscribeToContainerStats(
 *   'abc123',
 *   'agent-local',
 *   (stats) => {
 *     console.log(`CPU: ${stats.cpuStats.cpuPercentage.toFixed(2)}%`);
 *     console.log(`Memory: ${stats.memoryStats.percentage.toFixed(2)}%`);
 *   },
 *   (error) => console.error('Stats stream error:', error)
 * );
 * 
 * // Stop streaming after 30 seconds
 * setTimeout(unsubscribe, 30000);
 * ```
 */
export function subscribeToContainerStats(
  containerId: string,
  agentId: string,
  onMessage: (stats: ContainerStats) => void,
  onError?: (error: Error) => void
): () => void {
  logger.debug(`[WS] Creating WebSocket connection to ${GRAPHQL_WS_ENDPOINT} for container ${containerId}`);
  const ws = new WebSocket(GRAPHQL_WS_ENDPOINT, 'graphql-transport-ws');

  ws.onopen = () => {
    logger.debug(`[WS] WebSocket opened, initializing connection...`);
    // Initialize connection
    ws.send(JSON.stringify({ type: 'connection_init' }));
  };

  ws.onmessage = (event) => {
    const message = JSON.parse(event.data);
    logger.debug(`[WS-Stats] Message received - type: ${message.type}`, message);

    if (message.type === 'connection_ack') {
      logger.debug(`[WS-Stats] Connection acknowledged, sending subscription query`);
      const subscriptionQuery = `
        subscription ContainerStats($containerId: String!, $agentId: String!) {
          containerStatsStream(containerId: $containerId, agentId: $agentId) {
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
      `;

      ws.send(JSON.stringify({
        id: '1',
        type: 'subscribe',
        payload: { 
          query: subscriptionQuery,
          variables: { containerId, agentId }
        }
      }));
    } else if (message.type === 'next' && message.payload?.data) {
      if (message.payload.errors) {
        const firstError = message.payload.errors[0];
        const errorCode = firstError.extensions?.code;
        const errorMessage = firstError.message || 'GraphQL subscription error';
        onError?.(new GraphQLError(errorMessage, errorCode, firstError));
        return;
      }

      if (message.payload.data?.containerStatsStream) {
        onMessage(message.payload.data.containerStatsStream);
      }
    } else if (message.type === 'error') {
      logger.error('[WS-Stats] Subscription error:', message.payload);
      const firstError = Array.isArray(message.payload) ? message.payload[0] : message.payload;
      const errorCode = firstError?.extensions?.code;
      const errorMessage = firstError?.message || 'Stats subscription error';
      onError?.(new GraphQLError(errorMessage, errorCode, firstError));
    }
  };

  ws.onerror = (error) => {
    logger.error('[WS-Stats] WebSocket error:', error);
    onError?.(new GraphQLError('WebSocket connection error', 'WEBSOCKET_ERROR'));
  };

  ws.onclose = (event) => {
    logger.debug(`[WS-Stats] WebSocket closed - code: ${event.code}, reason: ${event.reason || 'none'}`);
  };

  return () => {
    if (ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify({ id: '1', type: 'complete' }));
      ws.close();
    }
  };
}
