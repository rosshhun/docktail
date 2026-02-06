/**
 * Docktail GraphQL API
 * 
 * This is the main entry point for interacting with the Docktail cluster API.
 * 
 * ## Organization
 * 
 * The API is organized into the following modules:
 * 
 * - **types** - TypeScript interfaces and types
 * - **client** - Base GraphQL client
 * - **queries** - Query functions for one-time data fetching
 * - **subscriptions** - Subscription functions for real-time data streams
 * - **errors** - Error handling utilities
 * - **helpers** - Utility functions (Compose metadata, grouping, etc.)
 * 
 * ## Quick Start
 * 
 * ```typescript
 * import {
 *   fetchContainers,
 *   fetchAgents,
 *   subscribeToLogs,
 *   subscribeToContainerStats
 * } from '@/lib/api';
 * 
 * // Fetch all containers
 * const containers = await fetchContainers();
 * 
 * // Subscribe to real-time logs
 * const unsubscribe = subscribeToLogs(
 *   containerId,
 *   agentId,
 *   (log) => console.log(log.content),
 *   (error) => console.error(error)
 * );
 * ```
 * 
 * @module api
 */

// Export all types
export type * from './types';

// Export error classes
export { GraphQLError } from './errors';

// Export client
export { query } from './client';

// Export all query functions
export {
  fetchHealth,
  fetchVersion,
  fetchAgents,
  fetchAgent,
  fetchAgentHealth,
  fetchContainers,
  fetchContainer,
  fetchContainerDetails,
  fetchContainerStats,
  fetchHistoricalLogs,
} from './queries';

// Export all subscription functions
export {
  subscribeToLogs,
  subscribeToMultipleContainerLogs,
  subscribeToAgentHealth,
  subscribeToContainerStats,
} from './subscriptions';

// Export helper functions
export {
  extractComposeMetadata,
  groupContainersByCompose,
} from './helpers';
