<script lang="ts">
  import { Server, Shield, Activity } from '@lucide/svelte';
  import PageHeader from '../lib/common/PageHeader.svelte';
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
    fetchNodes,
    updateNode,
    type NodeView,
    GraphQLError,
  } from '../lib/api';
  import { formatBytes } from '../lib/utils/formatting';
  import { logger } from '../lib/utils/logger';

  let allNodes = $state<NodeView[]>([]);
  let isLoading = $state(true);
  let error = $state<GraphQLError | null>(null);
  let swarmAgentId = $state('');
  let actionMessage = $state('');

  $effect(() => { loadNodes(); });

  async function loadNodes() {
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
        allNodes = await fetchNodes(swarmAgentId);
      }
    } catch (err: any) {
      error = err instanceof GraphQLError ? err : new GraphQLError(err.message, 'INTERNAL_SERVER_ERROR');
      logger.error('[Nodes] Failed:', err);
    } finally {
      isLoading = false;
    }
  }

  function nodeStatusDot(node: NodeView): 'running' | 'stopped' | 'warning' {
    if (node.status === 'READY' && node.availability === 'ACTIVE') return 'running';
    if (node.status === 'DOWN' || node.status === 'DISCONNECTED') return 'stopped';
    return 'warning';
  }

  async function toggleDrain(node: NodeView) {
    try {
      actionMessage = '';
      const newAvailability = node.availability === 'DRAIN' ? 'active' : 'drain';
      const result = await updateNode(node.id, swarmAgentId, { availability: newAvailability });
      actionMessage = result.message;
      if (result.success) setTimeout(() => loadNodes(), 1500);
    } catch (err: any) {
      actionMessage = err.message;
    }
  }
</script>

<div class="flex flex-col h-full bg-[rgb(var(--color-bg-primary))]">
  <PageHeader title="Nodes" subtitle="Docker Swarm nodes">
    {#snippet actions()}
      <RefreshButton onclick={loadNodes} disabled={isLoading} />
    {/snippet}
  </PageHeader>

  <div class="flex-1 overflow-auto px-8 py-4">
    {#if actionMessage}
      <div class="mb-4 px-4 py-2 bg-[rgb(var(--color-bg-secondary))] border border-[rgb(var(--color-border-primary))] rounded text-xs text-[rgb(var(--color-text-secondary))]">
        {actionMessage}
      </div>
    {/if}

    {#if isLoading}
      <LoadingState message="Loading nodes..." />
    {:else if error}
      <ErrorState error={error} onRetry={loadNodes} title="Failed to load nodes" />
    {:else if allNodes.length === 0}
      <EmptyState icon={Server} title="No nodes found" message="No swarm nodes detected" />
    {:else}
      <DataTable columns={[
        { key: 'status', label: 'Status', width: 'w-16' },
        { key: 'hostname', label: 'Hostname' },
        { key: 'role', label: 'Role', width: 'w-24' },
        { key: 'availability', label: 'Availability', width: 'w-28' },
        { key: 'state', label: 'State', width: 'w-24' },
        { key: 'engine', label: 'Engine', width: 'w-24' },
        { key: 'os', label: 'OS / Arch', width: 'w-28' },
        { key: 'resources', label: 'Resources', width: 'w-36' },
        { key: 'actions', label: '', width: 'w-28' },
      ]}>
        {#each allNodes as node (node.id)}
          <tr class="hover:bg-[rgb(var(--color-bg-tertiary))]/50 transition-colors cursor-pointer" onclick={() => window.location.hash = `/swarm/nodes/${node.id}`}>
            <td class="px-4 py-3">
              <StatusDot status={nodeStatusDot(node)} animated={node.status === 'READY'} size="md" />
            </td>
            <td class="px-4 py-3">
              <div class="text-sm font-medium text-[rgb(var(--color-text-primary))]">{node.hostname}</div>
              <div class="text-[10px] text-[rgb(var(--color-text-tertiary))] font-mono truncate max-w-[200px]">{node.id.slice(0, 12)}</div>
            </td>
            <td class="px-4 py-3">
              <Badge variant={node.role === 'MANAGER' ? 'info' : 'default'} size="xs">
                {node.role}
                {#if node.managerStatus?.leader}
                  ★
                {/if}
              </Badge>
            </td>
            <td class="px-4 py-3">
              <Badge variant={node.availability === 'ACTIVE' ? 'success' : node.availability === 'DRAIN' ? 'error' : 'warning'} size="xs">
                {node.availability}
              </Badge>
            </td>
            <td class="px-4 py-3">
              <Badge variant={node.status === 'READY' ? 'success' : 'error'} size="xs">{node.status}</Badge>
            </td>
            <td class="px-4 py-3 text-xs text-[rgb(var(--color-text-secondary))]">{node.engineVersion}</td>
            <td class="px-4 py-3 text-xs text-[rgb(var(--color-text-secondary))]">{node.os}/{node.architecture}</td>
            <td class="px-4 py-3">
              <div class="text-[10px] text-[rgb(var(--color-text-secondary))]">
                <div>{(Number(node.nanoCpus) / 1e9).toFixed(0)} CPUs</div>
                <div>{formatBytes(Number(node.memoryBytes))}</div>
              </div>
            </td>
            <td class="px-4 py-3" onclick={(e) => e.stopPropagation()}>
              <button
                onclick={() => toggleDrain(node)}
                class="text-[10px] font-medium px-2.5 py-1 rounded border transition-all cursor-pointer {node.availability === 'DRAIN' ? 'border-green-500/30 text-green-400 hover:bg-green-500/10' : 'border-amber-500/30 text-amber-400 hover:bg-amber-500/10'}"
              >
                {node.availability === 'DRAIN' ? 'Activate' : 'Drain'}
              </button>
            </td>
          </tr>
        {/each}
      </DataTable>
    {/if}
  </div>
</div>
