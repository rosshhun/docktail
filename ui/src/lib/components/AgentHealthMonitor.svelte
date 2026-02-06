<script lang="ts">
  import { subscribeToAgentHealth, type AgentHealthEvent } from '../api';
  import { Activity, CheckCircle, AlertTriangle, XCircle, HelpCircle } from '@lucide/svelte';
  
  let { agentId } = $props<{
    agentId: string;
  }>();
  
  let health = $state<AgentHealthEvent | null>(null);
  let error = $state<string | null>(null);
  
  // Derived values for dynamic icon and color (capitalized for component usage)
  const StatusIcon = $derived(health ? getStatusIcon(health.status) : HelpCircle);
  const statusColor = $derived(health ? getStatusColor(health.status) : '');
  
  // Use $effect to manage subscription lifecycle
  $effect(() => {
    // Subscribe to real-time health updates
    const unsubscribe = subscribeToAgentHealth(
      agentId,
      (newHealth) => {
        health = newHealth;
        error = null;
      },
      (err) => {
        error = err.message;
        health = null;
      }
    );
    
    // Cleanup function (called when effect re-runs or component unmounts)
    return () => {
      unsubscribe();
    };
  });
  
  function getStatusIcon(status: string) {
    switch (status) {
      case 'HEALTHY': return CheckCircle;
      case 'DEGRADED': return AlertTriangle;
      case 'UNHEALTHY': return XCircle;
      default: return HelpCircle;
    }
  }
  
  function getStatusColor(status: string) {
    switch (status) {
      case 'HEALTHY': return 'text-green-600 bg-green-50 border-green-200';
      case 'DEGRADED': return 'text-yellow-600 bg-yellow-50 border-yellow-200';
      case 'UNHEALTHY': return 'text-red-600 bg-red-50 border-red-200';
      default: return 'text-[rgb(var(--color-text-secondary))] bg-[rgb(var(--color-bg-tertiary))] border-[rgb(var(--color-border-primary))]';
    }
  }
  
  function getMetricDisplay(key: string): string {
    // Format metric keys for display
    return key
      .split('_')
      .map(word => word.charAt(0).toUpperCase() + word.slice(1))
      .join(' ');
  }
  
  function getMetricValue(key: string, value: string): string {
    // Format specific metrics
    if (key.includes('rate') || key.includes('percentage')) {
      const num = parseFloat(value);
      return !isNaN(num) ? (num * 100).toFixed(1) + '%' : value;
    }
    if (key.includes('time') && key.includes('us')) {
      return value + ' μs';
    }
    if (key === 'total_parsed' || key === 'json_parsed' || key === 'logfmt_parsed' || key === 'docker_failures') {
      const num = parseInt(value);
      return !isNaN(num) ? num.toLocaleString() : value;
    }
    return value;
  }
  
  function isImportantMetric(key: string): boolean {
    const important = ['total_parsed', 'success_rate', 'parse_panics', 'avg_parse_time_us', 'docker_failures'];
    return important.includes(key);
  }
</script>

<div class="space-y-4">
  {#if error}
    <div class="bg-red-50 border border-red-200 rounded-lg p-4">
      <p class="text-sm text-red-700">{error}</p>
    </div>
  {:else if !health}
    <div class="flex items-center justify-center p-6">
      <div class="flex items-center gap-3 text-[rgb(var(--color-text-tertiary))]">
        <div class="animate-spin rounded-full h-5 w-5 border-2 border-[rgb(var(--color-border-primary))] border-t-blue-600"></div>
        <span class="text-sm">Loading health status...</span>
      </div>
    </div>
  {:else}
    <!-- Status Card -->
    <div class="border rounded-lg p-5 {statusColor}">
      <div class="flex items-start gap-3">
        <StatusIcon class="w-6 h-6 shrink-0 mt-0.5" />
        <div class="flex-1 min-w-0">
          <div class="flex items-center justify-between mb-1">
            <h3 class="text-sm font-semibold">Agent Status: {health.status}</h3>
            <span class="text-xs opacity-75">
              {new Date(health.timestamp * 1000).toLocaleTimeString()}
            </span>
          </div>
          <p class="text-sm opacity-90">{health.message}</p>
        </div>
      </div>
    </div>
    
    <!-- Important Metrics -->
    {#if health.metadata.filter(m => isImportantMetric(m.key)).length > 0}
      <div class="bg-[rgb(var(--color-bg-secondary))] border border-[rgb(var(--color-border-primary))] rounded-lg p-5">
        <div class="flex items-center gap-2 mb-4">
          <Activity class="w-5 h-5 text-blue-600" />
          <h3 class="text-sm font-semibold text-[rgb(var(--color-text-primary))]">Key Metrics</h3>
        </div>
        
        <div class="grid grid-cols-2 gap-4">
          {#each health.metadata.filter(m => isImportantMetric(m.key)) as metric}
            <div class="bg-[rgb(var(--color-bg-tertiary))] rounded-md p-3">
              <div class="text-xs text-[rgb(var(--color-text-secondary))] mb-1">{getMetricDisplay(metric.key)}</div>
              <div class="text-lg font-semibold text-[rgb(var(--color-text-primary))]">
                {getMetricValue(metric.key, metric.value)}
              </div>
            </div>
          {/each}
        </div>
      </div>
    {/if}
    
    <!-- All Metrics (Collapsible) -->
    {#if health.metadata.length > 0}
      <details class="bg-[rgb(var(--color-bg-secondary))] border border-[rgb(var(--color-border-primary))] rounded-lg overflow-hidden">
        <summary class="px-5 py-3 cursor-pointer hover:bg-[rgb(var(--color-bg-tertiary))] text-sm font-medium text-[rgb(var(--color-text-secondary))] select-none">
          All Parsing Metrics ({health.metadata.length})
        </summary>
        <div class="border-t border-[rgb(var(--color-border-primary))] px-5 py-4">
          <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
            {#each health.metadata as metric}
              <div class="flex justify-between items-center py-2 border-b border-gray-100 last:border-b-0">
                <span class="text-sm text-[rgb(var(--color-text-secondary))]">{getMetricDisplay(metric.key)}</span>
                <span class="text-sm font-medium text-[rgb(var(--color-text-primary))]">
                  {getMetricValue(metric.key, metric.value)}
                </span>
              </div>
            {/each}
          </div>
        </div>
      </details>
    {/if}
    
    <div class="text-xs text-[rgb(var(--color-text-tertiary))] text-center">
      Updates every ~5 seconds
    </div>
  {/if}
</div>
