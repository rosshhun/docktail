<script lang="ts">
  import { link } from 'svelte-spa-router';
  import StatusDot from '../common/StatusDot.svelte';
  import ResourceStats from '../common/ResourceStats.svelte';
  import PortsList from '../common/PortsList.svelte';
  import { FileText } from '@lucide/svelte';
  import type { ContainerWithAgent } from '../api';
  import { extractComposeMetadata } from '../api';
  import { subscribeToStats, getContainerStats } from '../stores/containerStats.svelte';
  import { parseUptime, parseExitCode } from '../utils/containerParsing';
  import { logger } from '../utils/logger';

  let { 
    container,
    showAgentColumn = true
  } = $props<{ 
    container: ContainerWithAgent;
    showAgentColumn?: boolean;
  }>();

  const displayState = $derived(container.state.toLowerCase());
  const composeMetadata = $derived(extractComposeMetadata(container.labels));

  // Subscribe to real-time stats using Svelte 5 pattern
  // Use $derived to reactively get stats for the current container
  const stats = $derived(getContainerStats(container.id)());

  const uptimeDisplay = $derived(parseUptime(container.status, container.state));
  const exitCode = $derived(parseExitCode(container.status, container.state));

  function handleRowClick(event: MouseEvent) {
    const target = event.target as HTMLElement;
    if (target.closest('button') || target.closest('a')) {
      return;
    }
    // Store container name in localStorage for sidebar navigation state
    localStorage.setItem('currentContainerName', container.name);
    window.location.hash = `/containers/${container.id}`;
  }

  // Use $effect to manage subscription lifecycle
  $effect(() => {
    if (container.state === 'RUNNING') {
      logger.debug(`[ContainerRow] Subscribing to stats for ${container.name}`);
      const unsubscribe = subscribeToStats(container.id, container.agentId);
      
      // Cleanup function (called when effect re-runs or component unmounts)
      return () => {
        logger.debug(`[ContainerRow] Unsubscribing from stats for ${container.name}`);
        unsubscribe();
      };
    }
  });
</script>

<tr 
  class="group border-b border-[rgb(var(--color-border-primary))] hover:bg-[rgb(var(--color-bg-tertiary))] transition-colors cursor-pointer"
  onclick={handleRowClick}
>
  <!-- Status Column: Clean Status Dot -->
  <td class="px-4 py-3">
    <div class="flex items-center justify-center">
      <StatusDot status={displayState} animated={container.state === 'RUNNING'} size="md" />
    </div>
  </td>

  <!-- Name Column: Container Name + Image + Tags -->
  <td class="px-4 py-3">
    <div class="min-w-0">
      <div class="flex items-center gap-2 flex-wrap">
        <span class="text-sm font-semibold text-[rgb(var(--color-text-primary))]">
          {container.name}
        </span>
        {#if container.labels.find((l: { key: string; value: string }) => l.key === 'environment')}
          {@const env = container.labels.find((l: { key: string; value: string }) => l.key === 'environment')?.value}
          <span class="inline-flex items-center px-1.5 py-0.5 rounded text-[9px] font-semibold border"
            class:bg-red-50={env === 'prod'}
            class:text-red-700={env === 'prod'}
            class:border-red-200={env === 'prod'}
            class:bg-green-50={env === 'dev'}
            class:text-green-700={env === 'dev'}
            class:border-green-200={env === 'dev'}
          >
            {env}
          </span>
        {/if}
      </div>
      <div class="text-[11px] text-[rgb(var(--color-text-tertiary))] font-mono truncate mt-0.5">
        {container.image}
      </div>
    </div>
  </td>

  <!-- Resources Column: CPU + Memory with Progress Bar -->
  <td class="px-4 py-3">
    {#if container.state === 'RUNNING'}
      <ResourceStats stats={stats} loading={!stats} />
    {:else}
      <span class="text-xs text-[rgb(var(--color-text-tertiary))]">-</span>
    {/if}
  </td>

  <!-- Ports Column: Simplified summary view -->
  <td class="px-4 py-3">
    <PortsList ports={container.ports} />
  </td>

  <!-- Uptime Column -->
  <td class="px-4 py-3">
    <div class="space-y-0.5">
      {#if container.state === 'RUNNING'}
        <div class="text-xs font-medium text-green-600">
          {uptimeDisplay}
        </div>
      {:else if container.state === 'EXITED' || container.state === 'DEAD'}
        <div class="text-xs text-[rgb(var(--color-text-secondary))]">
          exited
        </div>
        {#if exitCode !== null}
          <div class="text-[10px] font-mono" 
            class:text-red-600={exitCode !== 0}
            class:text-[rgb(var(--color-text-tertiary))]={exitCode === 0}
          >
            ({exitCode})
          </div>
        {/if}
        <div class="text-[10px] text-[rgb(var(--color-text-tertiary))]">
          {uptimeDisplay}
        </div>
      {:else}
        <div class="text-xs text-yellow-600">
          {displayState}
        </div>
      {/if}
    </div>
  </td>

  {#if showAgentColumn}
  <!-- Agent Column -->
  <td class="px-4 py-3">
    <div class="text-xs text-[rgb(var(--color-text-primary))] font-medium truncate">
      {container.agentName}
    </div>
  </td>
  {/if}

  <!-- Actions Column -->
  <td class="px-4 py-3">
    <div class="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
      <a 
        use:link 
        href="/logs/{container.id}"
        title="View Logs"
        class="p-1.5 rounded hover:bg-[rgb(var(--color-bg-tertiary))] text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))] transition-colors"
      >
        <FileText class="w-4 h-4" strokeWidth={2} />
      </a>
    </div>
  </td>
</tr>
