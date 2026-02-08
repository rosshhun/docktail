<script lang="ts">
  import { link } from 'svelte-spa-router';
  import { Layers, SlidersHorizontal, Calendar } from '@lucide/svelte';
  import PageHeader from '../lib/common/PageHeader.svelte';
  import SearchInput from '../lib/common/SearchInput.svelte';
  import FilterButton from '../lib/common/FilterButton.svelte';
  import RefreshButton from '../lib/common/RefreshButton.svelte';
  import LoadingState from '../lib/common/LoadingState.svelte';
  import ErrorState from '../lib/common/ErrorState.svelte';
  import EmptyState from '../lib/common/EmptyState.svelte';
  import DataTable from '../lib/common/DataTable.svelte';
  import Badge from '../lib/common/Badge.svelte';
  import StatusDot from '../lib/common/StatusDot.svelte';
  import {
    fetchAgents,
    fetchSwarmInfo,
    fetchServices,
    type Agent,
    type ServiceView,
    GraphQLError,
  } from '../lib/api';
  import { logger } from '../lib/utils/logger';

  let allServices = $state<ServiceView[]>([]);
  let isLoading = $state(true);
  let error = $state<GraphQLError | null>(null);
  let searchQuery = $state('');
  let modeFilter = $state<string[]>([]);
  let sortBy = $state<'name' | 'replicas' | 'updated'>('name');
  let sortOrder = $state<'asc' | 'desc'>('asc');
  let swarmAgentId = $state('');

  $effect(() => { loadServices(); });

  async function loadServices() {
    try {
      isLoading = true;
      error = null;
      const agents = await fetchAgents();
      for (const agent of agents) {
        try {
          const info = await fetchSwarmInfo(agent.id);
          if (info?.isSwarmMode) { swarmAgentId = agent.id; break; }
        } catch { /* next */ }
      }
      if (swarmAgentId) {
        allServices = await fetchServices(swarmAgentId);
      }
    } catch (err: any) {
      error = err instanceof GraphQLError ? err : new GraphQLError(err.message, 'INTERNAL_SERVER_ERROR');
      logger.error('[Services] Failed:', err);
    } finally {
      isLoading = false;
    }
  }

  const filteredServices = $derived(() => {
    let result = allServices;
    if (searchQuery) {
      result = result.filter(s => s.name.toLowerCase().includes(searchQuery.toLowerCase()) || s.image.toLowerCase().includes(searchQuery.toLowerCase()));
    }
    if (modeFilter.length > 0) {
      result = result.filter(s => modeFilter.includes(s.mode));
    }
    result = [...result].sort((a, b) => {
      let cmp = 0;
      if (sortBy === 'name') cmp = a.name.localeCompare(b.name);
      else if (sortBy === 'replicas') cmp = a.replicasRunning - b.replicasRunning;
      else if (sortBy === 'updated') cmp = new Date(a.updatedAt).getTime() - new Date(b.updatedAt).getTime();
      return sortOrder === 'asc' ? cmp : -cmp;
    });
    return result;
  });

  function getReplicaVariant(svc: ServiceView): 'success' | 'warning' | 'error' {
    if (svc.replicasRunning >= svc.replicasDesired) return 'success';
    if (svc.replicasRunning > 0) return 'warning';
    return 'error';
  }

  function timeAgo(dateStr: string): string {
    const s = Math.floor((Date.now() - new Date(dateStr).getTime()) / 1000);
    if (s < 60) return 'just now';
    const m = Math.floor(s / 60);
    if (m < 60) return `${m}m ago`;
    const h = Math.floor(m / 60);
    if (h < 24) return `${h}h ago`;
    return `${Math.floor(h / 24)}d ago`;
  }
</script>

<div class="flex flex-col h-full bg-[rgb(var(--color-bg-primary))]">
  <PageHeader title="Services" subtitle="Docker Swarm services">
    <div class="flex items-center gap-3 flex-wrap">
      <div class="flex-1 max-w-md">
        <SearchInput placeholder="Search services..." bind:value={searchQuery} />
      </div>

      <FilterButton
        icon={SlidersHorizontal}
        label="Mode"
        active={modeFilter.length > 0}
        count={modeFilter.length}
        dropdownId="mode-filter"
      >
        <div class="min-w-[180px] py-3 px-2">
          {#each ['REPLICATED', 'GLOBAL'] as mode}
            <label class="group flex items-center gap-2.5 cursor-pointer hover:bg-[rgb(var(--color-bg-secondary))] px-2.5 py-2 rounded-lg">
              <input type="checkbox" checked={modeFilter.includes(mode)} onchange={() => modeFilter = modeFilter.includes(mode) ? modeFilter.filter(m => m !== mode) : [...modeFilter, mode]} class="w-4 h-4 rounded border-2 border-[rgb(var(--color-border-secondary))] bg-[rgb(var(--color-bg-primary))]" />
              <span class="text-xs font-medium text-[rgb(var(--color-text-primary))]">{mode}</span>
            </label>
          {/each}
        </div>
      </FilterButton>

      <RefreshButton onclick={loadServices} disabled={isLoading} />
    </div>
  </PageHeader>

  <div class="flex-1 overflow-auto px-8 py-4">
    {#if isLoading}
      <LoadingState message="Loading services..." />
    {:else if error}
      <ErrorState error={error} onRetry={loadServices} title="Failed to load services" />
    {:else if filteredServices().length === 0}
      <EmptyState icon={Layers} title="No services found" message={searchQuery ? 'Try adjusting your search' : 'No swarm services are running'} />
    {:else}
      <DataTable columns={[
        { key: 'status', label: 'Status', width: 'w-16' },
        { key: 'name', label: 'Service' },
        { key: 'image', label: 'Image' },
        { key: 'mode', label: 'Mode', width: 'w-28' },
        { key: 'replicas', label: 'Replicas', width: 'w-28' },
        { key: 'ports', label: 'Ports', width: 'w-32' },
        { key: 'updated', label: 'Updated', width: 'w-28' },
        { key: 'actions', label: '', width: 'w-20' },
      ]}>
        {#each filteredServices() as svc (svc.id)}
          <tr class="hover:bg-[rgb(var(--color-bg-tertiary))]/50 transition-colors cursor-pointer" onclick={() => window.location.hash = `/swarm/services/${svc.id}`}>
            <td class="px-4 py-3">
              <StatusDot status={svc.replicasRunning >= svc.replicasDesired ? 'running' : svc.replicasRunning > 0 ? 'warning' : 'stopped'} animated={svc.replicasRunning > 0} size="md" />
            </td>
            <td class="px-4 py-3">
              <div class="text-sm font-medium text-[rgb(var(--color-text-primary))]">{svc.name}</div>
              {#if svc.stackNamespace}
                <div class="text-[10px] text-[rgb(var(--color-text-tertiary))]">Stack: {svc.stackNamespace}</div>
              {/if}
            </td>
            <td class="px-4 py-3">
              <span class="text-xs font-mono text-[rgb(var(--color-text-secondary))] truncate block max-w-[200px]">{svc.image}</span>
            </td>
            <td class="px-4 py-3">
              <Badge variant={svc.mode === 'GLOBAL' ? 'info' : 'default'} size="xs">{svc.mode}</Badge>
            </td>
            <td class="px-4 py-3">
              <Badge variant={getReplicaVariant(svc)} size="sm">{svc.replicasRunning}/{svc.replicasDesired}</Badge>
            </td>
            <td class="px-4 py-3">
              <div class="flex flex-wrap gap-1">
                {#each svc.ports.slice(0, 2) as port}
                  <span class="text-[10px] font-mono bg-[rgb(var(--color-bg-tertiary))] px-1.5 py-0.5 rounded text-[rgb(var(--color-text-secondary))]">
                    {port.publishedPort}:{port.targetPort}
                  </span>
                {/each}
                {#if svc.ports.length > 2}
                  <span class="text-[10px] text-[rgb(var(--color-text-tertiary))]">+{svc.ports.length - 2}</span>
                {/if}
              </div>
            </td>
            <td class="px-4 py-3">
              <span class="text-xs text-[rgb(var(--color-text-secondary))]">{timeAgo(svc.updatedAt)}</span>
            </td>
            <td class="px-4 py-3">
              <a href="#/swarm/services/{svc.id}" class="text-xs text-[rgb(var(--color-accent-blue))] hover:underline">View</a>
            </td>
          </tr>
        {/each}
      </DataTable>
    {/if}
  </div>
</div>
