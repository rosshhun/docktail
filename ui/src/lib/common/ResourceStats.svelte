<script lang="ts">
  import ResourceBar from './ResourceBar.svelte';
  import { formatBytes } from '../utils/formatting';
  import type { ContainerStatsEvent } from '../api';

  let { 
    stats,
    loading = false
  } = $props<{
    stats: ContainerStatsEvent | null;
    loading?: boolean;
  }>();

  const cpuPercentage = $derived(stats?.cpuStats.cpuPercentage || 0);
  const memoryUsage = $derived(stats?.memoryStats.usage || 0);
  const memoryLimit = $derived(stats?.memoryStats.limit || 0);
  const memoryPercentage = $derived(stats?.memoryStats.percentage || 0);
  
  // Format display values
  const cpuDisplay = $derived(cpuPercentage.toFixed(1) + '%');
  const memDisplay = $derived(() => {
    if (memoryLimit > 0 && memoryLimit < Number.MAX_SAFE_INTEGER) {
      // Show usage / limit
      return `${formatBytes(memoryUsage)} / ${formatBytes(memoryLimit)}`;
    } else {
      // No limit set - just show usage
      return formatBytes(memoryUsage);
    }
  });
</script>

{#if loading}
  <!-- Skeleton Loader -->
  <div class="space-y-2 w-full">
    <!-- CPU Skeleton -->
    <div class="space-y-1">
      <div class="flex items-center justify-between">
        <span class="text-[10px] font-medium text-transparent bg-[rgb(var(--color-bg-tertiary))] rounded animate-pulse">CPU</span>
        <span class="text-xs font-semibold text-transparent bg-[rgb(var(--color-bg-tertiary))] rounded animate-pulse">00.0%</span>
      </div>
      <div class="h-1.5 bg-[rgb(var(--color-bg-tertiary))] rounded-full overflow-hidden animate-pulse">
        <div class="h-full w-0"></div>
      </div>
    </div>
    <!-- Memory Skeleton -->
    <div class="space-y-1">
      <div class="flex items-center justify-between">
        <span class="text-[10px] font-medium text-transparent bg-[rgb(var(--color-bg-tertiary))] rounded animate-pulse">MEM</span>
        <span class="text-xs font-semibold text-transparent bg-[rgb(var(--color-bg-tertiary))] rounded animate-pulse">000MB</span>
      </div>
      <div class="h-1.5 bg-[rgb(var(--color-bg-tertiary))] rounded-full overflow-hidden animate-pulse">
        <div class="h-full w-0"></div>
      </div>
    </div>
  </div>
{:else if stats}
  <!-- Real Stats -->
  <div class="space-y-2 w-full min-w-[140px]">
    <ResourceBar 
      label="CPU"
      value={cpuDisplay}
      percentage={cpuPercentage}
    />
    <ResourceBar 
      label="MEM"
      value={memDisplay()}
      percentage={memoryPercentage}
    />
  </div>
{:else}
  <span class="text-xs text-[rgb(var(--color-text-tertiary))]">-</span>
{/if}
