// Container stats store for managing real-time statistics using Svelte 5 runes
import { subscribeToContainerStats, type ContainerStatsEvent } from '../api';
import { logger } from '../utils/logger';

interface ContainerStatsMap {
  [containerId: string]: ContainerStatsEvent | null;
}

interface SubscriptionMap {
  [containerId: string]: {
    unsubscribe: () => void;
    refCount: number;
  };
}

// Shared reactive state using Svelte 5 $state rune
// Export as object (not directly reassignable) per Svelte 5 best practices
export const containerStatsState = $state<{ stats: ContainerStatsMap }>({
  stats: {}
});

// Track active subscriptions (non-reactive)
const subscriptions: SubscriptionMap = {};

/**
 * Subscribe to stats for a specific container
 * Automatically manages subscription lifecycle and ref counting
 */
export function subscribeToStats(containerId: string, agentId: string): () => void {
  logger.debug(`[Stats] Subscribing to stats for container ${containerId} on agent ${agentId}`);
  
  // If already subscribed, increment ref count
  if (subscriptions[containerId]) {
    logger.debug(`[Stats] Already subscribed, incrementing ref count`);
    subscriptions[containerId].refCount++;
    return () => unsubscribeFromStats(containerId);
  }

  // Create new subscription
  const unsubscribe = subscribeToContainerStats(
    containerId,
    agentId,
    (stats) => {
      logger.debug(`[Stats] Received stats for ${containerId}:`, {
        cpu: stats.cpuStats.cpuPercentage,
        memory: stats.memoryStats.percentage
      });
      // Update the reactive state
      containerStatsState.stats[containerId] = stats;
    },
    (error) => {
      logger.error(`[Stats] Subscription error for ${containerId}:`, error);
      // Clear stats on error
      delete containerStatsState.stats[containerId];
    }
  );

  subscriptions[containerId] = {
    unsubscribe,
    refCount: 1
  };

  return () => unsubscribeFromStats(containerId);
}

/**
 * Unsubscribe from stats for a specific container
 * Uses ref counting to handle multiple components
 */
function unsubscribeFromStats(containerId: string): void {
  const subscription = subscriptions[containerId];
  if (!subscription) return;

  subscription.refCount--;
  logger.debug(`[Stats] Unsubscribing from ${containerId}, ref count: ${subscription.refCount}`);

  // Only unsubscribe when no more components are using it
  if (subscription.refCount <= 0) {
    logger.debug(`[Stats] Closing subscription for ${containerId}`);
    subscription.unsubscribe();
    delete subscriptions[containerId];
    
    // Remove stats from state
    delete containerStatsState.stats[containerId];
  }
}

/**
 * Get stats for a specific container
 * Returns a getter function that accesses the reactive state
 */
export function getContainerStats(containerId: string): () => ContainerStatsEvent | null {
  return () => containerStatsState.stats[containerId] || null;
}

/**
 * Clear all stats and unsubscribe from everything
 */
export function clearAllStats(): void {
  Object.keys(subscriptions).forEach(containerId => {
    subscriptions[containerId].unsubscribe();
  });
  Object.keys(subscriptions).forEach(key => delete subscriptions[key]);
  containerStatsState.stats = {};
}
