<script lang="ts">
  import { push, link } from 'svelte-spa-router';
  import { Layers, Search, HeartPulse, RefreshCw } from '@lucide/svelte';
  import PageHeader from '../lib/common/PageHeader.svelte';
  import DataTable from '../lib/common/DataTable.svelte';
  import Badge from '../lib/common/Badge.svelte';
  import StatusDot from '../lib/common/StatusDot.svelte';
  import LoadingState from '../lib/common/LoadingState.svelte';
  import EmptyState from '../lib/common/EmptyState.svelte';
  import SearchInput from '../lib/common/SearchInput.svelte';
  import RefreshButton from '../lib/common/RefreshButton.svelte';
  import {
    fetchAgents,
    fetchSwarmInfo,
    fetchStacks,
    fetchStackHealth,
    type StackView,
    type StackHealthView,
    GraphQLError,
  } from '../lib/api';

  let stacks = $state<StackView[]>([]);
  let stackHealthMap = $state<Record<string, StackHealthView>>({});
  let isLoading = $state(true);
  let error = $state<GraphQLError | null>(null);
  let searchQuery = $state('');
  let swarmAgentId = $state('');

  $effect(() => { loadStacks(); });

  async function loadStacks() {
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
        stacks = await fetchStacks(swarmAgentId);
        // Load health for each stack
        const healthMap: Record<string, StackHealthView> = {};
        for (const stack of stacks) {
          try {
            healthMap[stack.namespace] = await fetchStackHealth(stack.namespace, swarmAgentId);
          } catch { /* skip */ }
        }
        stackHealthMap = healthMap;
      }
    } catch (err: any) {
      error = err instanceof GraphQLError ? err : new GraphQLError(err.message, 'INTERNAL_SERVER_ERROR');
    } finally {
      isLoading = false;
    }
  }

  const filteredStacks = $derived(
    stacks.filter(s => s.namespace.toLowerCase().includes(searchQuery.toLowerCase()))
  );

  function healthVariant(status?: string): 'success' | 'warning' | 'error' | 'default' {
    if (!status) return 'default';
    switch (status.toUpperCase()) {
      case 'HEALTHY': return 'success';
      case 'DEGRADED': return 'warning';
      case 'UNHEALTHY': return 'error';
      default: return 'default';
    }
  }
</script>

<div class="h-full flex flex-col bg-[rgb(var(--color-bg-primary))]">
  <div class="px-8 pt-3">
    <PageHeader title="Stacks" subtitle="Docker Swarm stacks" icon={Layers}>
      {#snippet actions()}
        <SearchInput bind:value={searchQuery} placeholder="Search stacks..." />
        <RefreshButton onclick={loadStacks} />
      {/snippet}
    </PageHeader>
  </div>

  <div class="flex-1 overflow-auto px-8 pb-8">
    {#if isLoading}
      <LoadingState message="Loading stacks..." />
    {:else if error}
      <div class="p-8 text-center"><p class="text-sm text-red-400">{error.getUserMessage()}</p></div>
    {:else if filteredStacks.length === 0}
      <EmptyState icon={Layers} title="No stacks found" message={searchQuery ? 'No stacks match your search.' : 'No stacks deployed in this swarm.'} />
    {:else}
      <DataTable columns={[
        { key: 'health', label: '', width: 'w-10' },
        { key: 'name', label: 'Name' },
        { key: 'services', label: 'Services', width: 'w-24' },
        { key: 'replicas', label: 'Replicas', width: 'w-32' },
        { key: 'healthStatus', label: 'Health', width: 'w-28' },
      ]}>
        {#each filteredStacks as stack (stack.namespace)}
          {@const health = stackHealthMap[stack.namespace]}
          <tr class="hover:bg-[rgb(var(--color-bg-tertiary))]/50 cursor-pointer" onclick={() => push(`/swarm/stacks/${stack.namespace}`)}>
            <td class="px-4 py-3">
              <StatusDot status={health?.status?.toUpperCase() === 'HEALTHY' ? 'running' : health?.status?.toUpperCase() === 'DEGRADED' ? 'paused' : health ? 'stopped' : 'running'} animated={health?.status?.toUpperCase() === 'HEALTHY'} size="md" />
            </td>
            <td class="px-4 py-3 text-sm font-medium text-[rgb(var(--color-text-primary))]">{stack.namespace}</td>
            <td class="px-4 py-3">
              <Badge variant="default" size="xs">{stack.serviceCount}</Badge>
            </td>
            <td class="px-4 py-3">
              <span class="text-xs font-medium">
                <span class="text-green-400">{stack.replicasRunning}</span>
                <span class="text-[rgb(var(--color-text-secondary))]"> / {stack.replicasDesired}</span>
              </span>
            </td>
            <td class="px-4 py-3">
              {#if health}
                <Badge variant={healthVariant(health.status)} size="xs">{health.status}</Badge>
              {:else}
                <span class="text-xs text-[rgb(var(--color-text-secondary))]">—</span>
              {/if}
            </td>
          </tr>
        {/each}
      </DataTable>
    {/if}
  </div>
</div>
