<script lang="ts">
  import { link } from 'svelte-spa-router';
  import StatusDot from '../common/StatusDot.svelte';
  import { Server, RefreshCw, Trash2 } from '@lucide/svelte';

  type Agent = {
    id: string;
    name: string;
    status: 'healthy' | 'degraded' | 'unhealthy' | 'unknown';
    endpoint: string;
    version: string;
    containerCount: number;
    runningCount: number;
  };

  let { agent } = $props<{ agent: Agent }>();

  const displayStatus = $derived(
    agent.status === 'healthy' ? 'healthy' : 
    agent.status === 'degraded' ? 'degraded' : 'unhealthy'
  );
</script>

<tr class="group hover:bg-[rgb(var(--color-bg-tertiary))] transition-colors">
  <!-- Status -->
  <td class="px-4 py-3">
    <div class="flex items-center">
      <StatusDot status={displayStatus} animated={agent.status === 'healthy'} size="md" />
    </div>
  </td>

  <!-- Agent Name -->
  <td class="px-4 py-3">
    <div class="min-w-0">
      <a 
        use:link 
        href="/agents/{agent.id}"
        class="text-sm font-semibold text-[rgb(var(--color-text-primary))] hover:text-[rgb(var(--color-accent-blue))] transition-colors block"
      >
        {agent.name}
      </a>
      <div class="text-xs text-[rgb(var(--color-text-secondary))] font-mono truncate mt-0.5">
        {agent.endpoint}
      </div>
    </div>
  </td>

  <!-- Containers -->
  <td class="px-4 py-3">
    <div class="text-xs text-[rgb(var(--color-text-primary))] font-medium">
      <span class="text-green-600 font-semibold">{agent.runningCount}</span> / {agent.containerCount}
    </div>
  </td>

  <!-- Version -->
  <td class="px-4 py-3">
    <div class="text-xs text-[rgb(var(--color-text-secondary))] font-mono">
      {agent.version}
    </div>
  </td>

  <!-- Actions -->
  <td class="px-4 py-3">
    <div class="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
      <a 
        use:link
        href="/agents/{agent.id}"
        title="View Details"
        class="p-1.5 rounded hover:bg-[rgb(var(--color-bg-secondary))] text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))] transition-colors"
      >
        <Server class="w-4 h-4" strokeWidth={2} />
      </a>
      <button 
        title="Refresh"
        class="p-1.5 rounded hover:bg-[rgb(var(--color-bg-secondary))] text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))] transition-colors"
      >
        <RefreshCw class="w-4 h-4" strokeWidth={2} />
      </button>
      <button 
        title="Delete"
        class="p-1.5 rounded hover:bg-[rgb(var(--color-bg-secondary))] text-[rgb(var(--color-text-secondary))] hover:text-red-600 transition-colors"
      >
        <Trash2 class="w-4 h-4" strokeWidth={2} />
      </button>
    </div>
  </td>
</tr>
