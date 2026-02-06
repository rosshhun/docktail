<script lang="ts">
  import { subscribeToContainerStats, type ContainerStatsEvent } from '../api';
  import { Activity, Cpu, HardDrive, Network, AlertCircle } from '@lucide/svelte';
  
  let { containerId, agentId } = $props<{
    containerId: string;
    agentId: string;
  }>();
  
  let stats = $state<ContainerStatsEvent | null>(null);
  let error = $state<string | null>(null);
  let statsHistory = $state<Array<{ timestamp: number; cpu: number; memory: number }>>([]);
  const MAX_HISTORY = 60; // Keep last 60 data points
  
  // Use $effect to manage subscription lifecycle
  $effect(() => {
    // Subscribe to real-time stats
    const unsubscribe = subscribeToContainerStats(
      containerId,
      agentId,
      (newStats) => {
        stats = newStats;
        error = null;
        
        // Update history for sparkline
        statsHistory = [
          ...statsHistory.slice(-MAX_HISTORY + 1),
          {
            timestamp: newStats.timestamp,
            cpu: newStats.cpuStats.cpuPercentage,
            memory: newStats.memoryStats.percentage
          }
        ];
      },
      (err) => {
        error = err.message;
        stats = null;
      }
    );
    
    // Cleanup function (called when effect re-runs or component unmounts)
    return () => {
      unsubscribe();
    };
  });
  
  function formatBytes(bytes: number): string {
    if (bytes === 0) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return `${(bytes / Math.pow(k, i)).toFixed(2)} ${sizes[i]}`;
  }
  
  function formatPercentage(value: number): string {
    return value.toFixed(1) + '%';
  }
  
  function getColorClass(percentage: number, type: 'cpu' | 'memory'): string {
    const thresholds = type === 'cpu' 
      ? { warning: 70, danger: 90 }
      : { warning: 80, danger: 95 };
    
    if (percentage >= thresholds.danger) return 'text-red-600 bg-red-50';
    if (percentage >= thresholds.warning) return 'text-yellow-600 bg-yellow-50';
    return 'text-green-600 bg-green-50';
  }
  
  function getBarColor(percentage: number, type: 'cpu' | 'memory'): string {
    const thresholds = type === 'cpu' 
      ? { warning: 70, danger: 90 }
      : { warning: 80, danger: 95 };
    
    if (percentage >= thresholds.danger) return 'bg-red-500';
    if (percentage >= thresholds.warning) return 'bg-yellow-500';
    return 'bg-green-500';
  }
</script>

<div class="space-y-6">
  {#if error}
    <div class="bg-red-50 border border-red-200 rounded-lg p-4 flex items-start gap-3">
      <AlertCircle class="w-5 h-5 text-red-600 shrink-0 mt-0.5" />
      <div class="flex-1 min-w-0">
        <h3 class="text-sm font-semibold text-red-900">Failed to load stats</h3>
        <p class="text-sm text-red-700 mt-1">{error}</p>
      </div>
    </div>
  {:else if !stats}
    <div class="flex items-center justify-center p-8">
      <div class="flex items-center gap-3 text-[rgb(var(--color-text-tertiary))]">
        <div class="animate-spin rounded-full h-5 w-5 border-2 border-[rgb(var(--color-border-primary))] border-t-blue-600"></div>
        <span class="text-sm">Loading real-time stats...</span>
      </div>
    </div>
  {:else}
    <!-- CPU Stats -->
    <div class="bg-[rgb(var(--color-bg-secondary))] border border-[rgb(var(--color-border-primary))] rounded-lg p-5">
      <div class="flex items-center justify-between mb-4">
        <div class="flex items-center gap-2">
          <Cpu class="w-5 h-5 text-blue-600" />
          <h3 class="text-sm font-semibold text-[rgb(var(--color-text-primary))]">CPU Usage</h3>
        </div>
        <span class="text-2xl font-bold {getColorClass(stats.cpuStats.cpuPercentage, 'cpu')} px-3 py-1 rounded-lg">
          {formatPercentage(stats.cpuStats.cpuPercentage)}
        </span>
      </div>
      
      <div class="space-y-3">
        <!-- Progress bar -->
        <div class="relative pt-1">
          <div class="overflow-hidden h-3 text-xs flex rounded-full bg-[rgb(var(--color-bg-tertiary))]">
            <div 
              style="width: {Math.min(stats.cpuStats.cpuPercentage, 100)}%"
              class="{getBarColor(stats.cpuStats.cpuPercentage, 'cpu')} transition-all duration-500 flex flex-col text-center whitespace-nowrap text-white justify-center"
            ></div>
          </div>
        </div>
        
        <div class="grid grid-cols-2 gap-4 text-sm">
          <div>
            <div class="text-[rgb(var(--color-text-tertiary))]">Cores</div>
            <div class="text-[rgb(var(--color-text-primary))] font-medium">{stats.cpuStats.onlineCpus}</div>
          </div>
          <div>
            <div class="text-[rgb(var(--color-text-tertiary))]">System Usage</div>
            <div class="text-[rgb(var(--color-text-primary))] font-medium">{(stats.cpuStats.systemUsage / 1_000_000).toFixed(2)}ms</div>
          </div>
        </div>
        
        {#if stats.cpuStats.throttling}
          <div class="bg-orange-50 border border-orange-200 rounded-md p-3">
            <div class="text-xs font-medium text-orange-800 mb-1">CPU Throttling Detected</div>
            <div class="text-xs text-orange-700">
              {stats.cpuStats.throttling.throttledPeriods} / {stats.cpuStats.throttling.totalPeriods} periods throttled
            </div>
          </div>
        {/if}
      </div>
    </div>
    
    <!-- Memory Stats -->
    <div class="bg-[rgb(var(--color-bg-secondary))] border border-[rgb(var(--color-border-primary))] rounded-lg p-5">
      <div class="flex items-center justify-between mb-4">
        <div class="flex items-center gap-2">
          <Activity class="w-5 h-5 text-purple-600" />
          <h3 class="text-sm font-semibold text-[rgb(var(--color-text-primary))]">Memory Usage</h3>
        </div>
        <span class="text-2xl font-bold {getColorClass(stats.memoryStats.percentage, 'memory')} px-3 py-1 rounded-lg">
          {formatPercentage(stats.memoryStats.percentage)}
        </span>
      </div>
      
      <div class="space-y-3">
        <!-- Progress bar -->
        <div class="relative pt-1">
          <div class="overflow-hidden h-3 text-xs flex rounded-full bg-[rgb(var(--color-bg-tertiary))]">
            <div 
              style="width: {Math.min(stats.memoryStats.percentage, 100)}%"
              class="{getBarColor(stats.memoryStats.percentage, 'memory')} transition-all duration-500 flex flex-col text-center whitespace-nowrap text-white justify-center"
            ></div>
          </div>
        </div>
        
        <div class="grid grid-cols-2 gap-4 text-sm">
          <div>
            <div class="text-[rgb(var(--color-text-tertiary))]">Usage</div>
            <div class="text-[rgb(var(--color-text-primary))] font-medium">{formatBytes(stats.memoryStats.usage)}</div>
          </div>
          <div>
            <div class="text-[rgb(var(--color-text-tertiary))]">Limit</div>
            <div class="text-[rgb(var(--color-text-primary))] font-medium">
              {stats.memoryStats.limit === 0 ? 'Unlimited' : formatBytes(stats.memoryStats.limit)}
            </div>
          </div>
          <div>
            <div class="text-[rgb(var(--color-text-tertiary))]">Cache</div>
            <div class="text-[rgb(var(--color-text-primary))] font-medium">{formatBytes(stats.memoryStats.cache)}</div>
          </div>
          <div>
            <div class="text-[rgb(var(--color-text-tertiary))]">RSS</div>
            <div class="text-[rgb(var(--color-text-primary))] font-medium">{formatBytes(stats.memoryStats.rss)}</div>
          </div>
        </div>
        
        {#if stats.memoryStats.swap && stats.memoryStats.swap > 0}
          <div class="bg-blue-50 border border-blue-200 rounded-md p-3">
            <div class="text-xs font-medium text-blue-800 mb-1">Swap Usage</div>
            <div class="text-xs text-blue-700">{formatBytes(stats.memoryStats.swap)}</div>
          </div>
        {/if}
      </div>
    </div>
    
    <!-- Network Stats -->
    {#if stats.networkStats.length > 0}
      <div class="bg-[rgb(var(--color-bg-secondary))] border border-[rgb(var(--color-border-primary))] rounded-lg p-5">
        <div class="flex items-center gap-2 mb-4">
          <Network class="w-5 h-5 text-green-600" />
          <h3 class="text-sm font-semibold text-[rgb(var(--color-text-primary))]">Network I/O</h3>
        </div>
        
        <div class="space-y-4">
          {#each stats.networkStats as netStats}
            <div>
              <div class="text-xs font-medium text-[rgb(var(--color-text-secondary))] mb-2">{netStats.interfaceName}</div>
              <div class="grid grid-cols-2 gap-4 text-sm">
                <div>
                  <div class="text-[rgb(var(--color-text-tertiary))] text-xs">RX</div>
                  <div class="text-[rgb(var(--color-text-primary))] font-medium">{formatBytes(netStats.rxBytes)}</div>
                  <div class="text-[rgb(var(--color-text-tertiary))] text-xs">{netStats.rxPackets.toLocaleString()} packets</div>
                </div>
                <div>
                  <div class="text-[rgb(var(--color-text-tertiary))] text-xs">TX</div>
                  <div class="text-[rgb(var(--color-text-primary))] font-medium">{formatBytes(netStats.txBytes)}</div>
                  <div class="text-[rgb(var(--color-text-tertiary))] text-xs">{netStats.txPackets.toLocaleString()} packets</div>
                </div>
              </div>
              {#if netStats.rxErrors > 0 || netStats.txErrors > 0}
                <div class="mt-2 text-xs text-red-600">
                  Errors: RX {netStats.rxErrors}, TX {netStats.txErrors}
                </div>
              {/if}
            </div>
          {/each}
        </div>
      </div>
    {/if}
    
    <!-- Disk I/O Stats -->
    <div class="bg-[rgb(var(--color-bg-secondary))] border border-[rgb(var(--color-border-primary))] rounded-lg p-5">
      <div class="flex items-center gap-2 mb-4">
        <HardDrive class="w-5 h-5 text-orange-600" />
        <h3 class="text-sm font-semibold text-[rgb(var(--color-text-primary))]">Disk I/O</h3>
      </div>
      
      <div class="grid grid-cols-2 gap-4 text-sm">
        <div>
          <div class="text-[rgb(var(--color-text-tertiary))]">Read</div>
          <div class="text-[rgb(var(--color-text-primary))] font-medium">{formatBytes(stats.blockIoStats.readBytes)}</div>
          <div class="text-[rgb(var(--color-text-tertiary))] text-xs">{stats.blockIoStats.readOps.toLocaleString()} ops</div>
        </div>
        <div>
          <div class="text-[rgb(var(--color-text-tertiary))]">Write</div>
          <div class="text-[rgb(var(--color-text-primary))] font-medium">{formatBytes(stats.blockIoStats.writeBytes)}</div>
          <div class="text-[rgb(var(--color-text-tertiary))] text-xs">{stats.blockIoStats.writeOps.toLocaleString()} ops</div>
        </div>
      </div>
    </div>
    
    {#if stats.pidsCount}
      <div class="bg-[rgb(var(--color-bg-tertiary))] border border-[rgb(var(--color-border-primary))] rounded-lg p-4">
        <div class="text-sm text-[rgb(var(--color-text-secondary))]">
          <span class="font-medium">Processes:</span>
          <span class="ml-2 text-[rgb(var(--color-text-primary))] font-semibold">{stats.pidsCount}</span>
        </div>
      </div>
    {/if}
    
    <div class="text-xs text-[rgb(var(--color-text-tertiary))] text-center">
      Updates every ~1 second • Last update: {new Date(stats.timestamp * 1000).toLocaleTimeString()}
    </div>
  {/if}
</div>
