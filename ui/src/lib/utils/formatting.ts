// Shared formatting utilities for containers

/**
 * Format bytes to human-readable format (B, KB, MB, GB, TB)
 */
export function formatBytes(bytes: number): string {
  if (bytes === 0) return '0B';
  if (bytes < 0) return '-';
  
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  
  // Use 1 decimal place for values >= 10, otherwise show 2 decimals for precision
  const value = bytes / Math.pow(k, i);
  const decimals = value >= 10 ? 1 : 2;
  
  return value.toFixed(decimals) + sizes[i];
}

/**
 * Format CPU limit for display
 */
export function formatCpuLimit(limit?: number): string {
  if (!limit) return 'Unlimited';
  return `${limit} shares`;
}

/**
 * Format nanoseconds to human-readable duration (e.g., "30s", "5m", "1h 30m")
 */
export function formatNsDuration(ns: number): string {
  if (ns <= 0) return '-';
  const seconds = Math.floor(ns / 1_000_000_000);
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  const remainingSecs = seconds % 60;
  if (minutes < 60) {
    return remainingSecs > 0 ? `${minutes}m ${remainingSecs}s` : `${minutes}m`;
  }
  const hours = Math.floor(minutes / 60);
  const remainingMins = minutes % 60;
  return remainingMins > 0 ? `${hours}h ${remainingMins}m` : `${hours}h`;
}

/**
 * Get resource bar color based on usage percentage
 * Uses gradient colors for better visual feedback
 */
export function getResourceColor(percentage: number): string {
  if (percentage >= 90) return 'bg-gradient-to-r from-red-500 to-red-600';
  if (percentage >= 75) return 'bg-gradient-to-r from-orange-500 to-orange-600';
  if (percentage >= 60) return 'bg-gradient-to-r from-yellow-500 to-yellow-600';
  if (percentage >= 40) return 'bg-gradient-to-r from-blue-500 to-blue-600';
  return 'bg-gradient-to-r from-green-500 to-green-600';
}
