<script lang="ts">
  import StatusDot from '../common/StatusDot.svelte';
  import Button from '../common/Button.svelte';
  import Badge from '../common/Badge.svelte';
  import { Server, ExternalLink, RefreshCw, Trash2, Edit, Activity } from '@lucide/svelte';

  type Agent = {
    id: string;
    name: string;
    status: 'healthy' | 'degraded' | 'unhealthy' | 'unknown';
    endpoint: string;
    uptime: string;
    lastSeen: string;
    containerCount: number;
    runningCount: number;
    cpuUsage?: string;
    memoryUsage?: string;
    diskUsage?: string;
  };

  let { agent } = $props<{ agent: Agent }>();
</script>

<div class="bg-[rgb(var(--color-bg-secondary))] border border-[rgb(var(--color-border-primary))] rounded-lg p-4 hover:border-[rgb(var(--color-text-secondary))]/40 transition-all duration-200">
  <div class="flex items-start justify-between mb-4">
    <div class="flex items-center gap-3">
      <div class="p-2 bg-[rgb(var(--color-accent-blue))]/5 rounded-lg border border-[rgb(var(--color-accent-blue))]/10">
        <Server class="w-5 h-5 text-[rgb(var(--color-accent-blue))]" strokeWidth={1.5} />
      </div>
      <div>
        <div class="flex items-center gap-2 mb-0.5">
          <h3 class="text-base font-semibold text-[rgb(var(--color-text-primary))]">{agent.name}</h3>
          <StatusDot status={agent.status} animated={agent.status === 'healthy'} />
        </div>
        <p class="text-[10px] text-[rgb(var(--color-text-secondary))] font-mono mt-0.5">{agent.endpoint}</p>
      </div>
    </div>
    <Badge variant={agent.status === 'healthy' ? 'success' : agent.status === 'degraded' ? 'warning' : 'error'} size="xs">
      {agent.status === 'healthy' ? 'HEALTHY' : agent.status === 'degraded' ? 'DEGRADED' : 'DISCONNECTED'}
    </Badge>
  </div>

  {#if agent.status === 'healthy' || agent.status === 'degraded'}
    <!-- Stats Grid -->
    <div class="grid grid-cols-2 md:grid-cols-4 gap-3 mb-4 p-3 bg-[rgb(var(--color-bg-secondary))] rounded-lg border border-[rgb(var(--color-border-primary))]">
      <div>
        <p class="text-[9px] text-[rgb(var(--color-text-secondary))] font-medium uppercase tracking-wide mb-1">Uptime</p>
        <p class="text-xs font-semibold text-[rgb(var(--color-text-primary))]">{agent.uptime}</p>
      </div>
      <div>
        <p class="text-[9px] text-[rgb(var(--color-text-secondary))] font-medium uppercase tracking-wide mb-1">Containers</p>
        <p class="text-xs font-semibold text-[rgb(var(--color-text-primary))]">{agent.runningCount} / {agent.containerCount}</p>
      </div>
      {#if agent.cpuUsage}
        <div>
          <p class="text-[9px] text-[rgb(var(--color-text-secondary))] font-medium uppercase tracking-wide mb-1">CPU Usage</p>
          <p class="text-xs font-semibold text-[rgb(var(--color-text-primary))]">{agent.cpuUsage}</p>
        </div>
      {/if}
      {#if agent.memoryUsage}
        <div>
          <p class="text-[9px] text-[rgb(var(--color-text-secondary))] font-medium uppercase tracking-wide mb-1">Memory</p>
          <p class="text-xs font-semibold text-[rgb(var(--color-text-primary))]">{agent.memoryUsage}</p>
        </div>
      {/if}
    </div>

    <!-- Degraded Warning -->
    {#if agent.status === 'degraded'}
      <div class="p-3 bg-amber-50 border border-amber-200 rounded-lg mb-4">
        <p class="text-xs text-amber-700 mb-0.5 font-medium">⚠️ Agent Performance Degraded</p>
        <p class="text-[10px] text-[rgb(var(--color-text-secondary))]">Parse success rate below threshold. Check agent logs for details.</p>
      </div>
    {/if}

    <!-- Action Buttons -->
    <div class="flex gap-2 flex-wrap">
      <Button variant="primary" size="xs">
        <ExternalLink class="w-3 h-3 mr-1" strokeWidth={2} />
        View
      </Button>
      <Button variant="default" size="xs">
        <Activity class="w-3 h-3 mr-1" strokeWidth={2} />
        Logs
      </Button>
      <Button variant="default" size="xs">
        <RefreshCw class="w-3 h-3 mr-1" strokeWidth={2} />
        Health
      </Button>
      <Button variant="ghost" size="xs">
        <Edit class="w-3 h-3 mr-1" strokeWidth={2} />
        Edit
      </Button>
      <Button variant="danger" size="xs">
        <Trash2 class="w-3 h-3" strokeWidth={2} />
      </Button>
    </div>
  {:else}
    <!-- Disconnected State -->
    <div class="p-3 bg-red-50 border border-red-200 rounded-lg mb-4">
      <p class="text-xs text-red-600 mb-0.5 font-medium">⚠️ Agent Disconnected</p>
      <p class="text-[10px] text-[rgb(var(--color-text-secondary))]">Last seen: {agent.lastSeen}</p>
    </div>

    <div class="flex gap-2">
      <Button variant="primary" size="xs">
        <RefreshCw class="w-3 h-3 mr-1" strokeWidth={2} />
        Reconnect
      </Button>
      <Button variant="default" size="xs">
        <Activity class="w-3 h-3 mr-1" strokeWidth={2} />
        View Logs
      </Button>
      <Button variant="danger" size="xs">
        <Trash2 class="w-3 h-3 mr-1" strokeWidth={2} />
        Remove
      </Button>
    </div>
  {/if}
</div>
