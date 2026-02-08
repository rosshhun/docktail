<script lang="ts">
  import { link } from 'svelte-spa-router';
  import { Server, Box, Layers, Network, Activity, Shield, ArrowRight } from '@lucide/svelte';
  import PageHeader from '../lib/common/PageHeader.svelte';
  import RefreshButton from '../lib/common/RefreshButton.svelte';
  import LoadingState from '../lib/common/LoadingState.svelte';
  import ErrorState from '../lib/common/ErrorState.svelte';
  import EmptyState from '../lib/common/EmptyState.svelte';
  import StatCard from '../lib/common/StatCard.svelte';
  import Badge from '../lib/common/Badge.svelte';
  import StatusDot from '../lib/common/StatusDot.svelte';
  import {
    fetchAgents,
    fetchSwarmInfo,
    fetchNodes,
    fetchServices,
    fetchStacks,
    type Agent,
    type SwarmInfo,
    type NodeView,
    type ServiceView,
    type StackView,
    GraphQLError
  } from '../lib/api';
  import { logger } from '../lib/utils/logger';
  import { formatBytes } from '../lib/utils/formatting';

  let agents = $state<Agent[]>([]);
  let swarmInfo = $state<SwarmInfo | null>(null);
  let nodes = $state<NodeView[]>([]);
  let services = $state<ServiceView[]>([]);
  let stacks = $state<StackView[]>([]);
  let isLoading = $state(true);
  let error = $state<GraphQLError | null>(null);
  let swarmAgentId = $state<string>('');

  $effect(() => {
    loadSwarmData();
  });

  async function loadSwarmData() {
    try {
      isLoading = true;
      error = null;
      agents = await fetchAgents();

      // Try each agent to find a swarm manager
      for (const agent of agents) {
        try {
          const info = await fetchSwarmInfo(agent.id);
          if (info?.isSwarmMode) {
            swarmInfo = info;
            swarmAgentId = agent.id;
            break;
          }
        } catch { /* try next agent */ }
      }

      if (swarmAgentId) {
        const [n, s, st] = await Promise.all([
          fetchNodes(swarmAgentId),
          fetchServices(swarmAgentId),
          fetchStacks(swarmAgentId),
        ]);
        nodes = n;
        services = s;
        stacks = st;
      }
    } catch (err: any) {
      error = err instanceof GraphQLError ? err : new GraphQLError(err.message || 'Failed to load swarm data', 'INTERNAL_SERVER_ERROR');
      logger.error('[SwarmOverview] Failed to load:', err);
    } finally {
      isLoading = false;
    }
  }

  const readyNodes = $derived(nodes.filter(n => n.status === 'READY').length);
  const managerNodes = $derived(nodes.filter(n => n.role === 'MANAGER').length);
  const workerNodes = $derived(nodes.filter(n => n.role === 'WORKER').length);
  const totalDesiredReplicas = $derived(services.reduce((sum, s) => sum + s.replicasDesired, 0));
  const totalRunningReplicas = $derived(services.reduce((sum, s) => sum + s.replicasRunning, 0));
</script>

<div class="flex flex-col h-full bg-[rgb(var(--color-bg-primary))]">
  <PageHeader title="Swarm" subtitle="Docker Swarm cluster overview">
    {#snippet actions()}
      <RefreshButton onclick={loadSwarmData} disabled={isLoading} />
    {/snippet}
  </PageHeader>

  <div class="flex-1 overflow-auto px-8 py-4">
    {#if isLoading}
      <LoadingState message="Loading swarm data..." />
    {:else if error}
      <ErrorState error={error} onRetry={loadSwarmData} title="Failed to load swarm data" />
    {:else if !swarmInfo}
      <EmptyState
        icon={Layers}
        title="Swarm Not Active"
        message="No Docker Swarm mode detected on any connected agent. Initialize a swarm to get started."
      />
    {:else}
      <div class="space-y-6">
        <!-- Overview Stats -->
        <div class="grid grid-cols-1 md:grid-cols-5 gap-4">
          <StatCard title="Swarm Status" icon={Activity}>
            <div class="text-center">
              <div class="w-10 h-10 mx-auto mb-2 rounded-full bg-green-500/10 flex items-center justify-center">
                <StatusDot status="running" animated size="md" />
              </div>
              <p class="text-xs font-medium text-green-400">Active</p>
            </div>
          </StatCard>

          <StatCard title="Nodes" icon={Server} value={nodes.length}>
            <div class="text-center">
              <div class="text-3xl font-bold text-[rgb(var(--color-text-primary))]">{nodes.length}</div>
              <p class="text-xs text-[rgb(var(--color-text-secondary))] mt-1">
                <span class="text-green-400 font-semibold">{readyNodes}</span> ready
              </p>
            </div>
          </StatCard>

          <StatCard title="Services" icon={Layers} value={services.length}>
            <div class="text-center">
              <div class="text-3xl font-bold text-[rgb(var(--color-text-primary))]">{services.length}</div>
              <p class="text-xs text-[rgb(var(--color-text-secondary))] mt-1">
                <span class="text-green-400 font-semibold">{totalRunningReplicas}</span>/{totalDesiredReplicas} replicas
              </p>
            </div>
          </StatCard>

          <StatCard title="Stacks" icon={Box} value={stacks.length} subtitle="Deployed" />

          <StatCard title="Managers" icon={Shield}>
            <div class="text-center">
              <div class="text-3xl font-bold text-[rgb(var(--color-text-primary))]">{managerNodes}</div>
              <p class="text-xs text-[rgb(var(--color-text-secondary))] mt-1">
                <span class="font-semibold">{workerNodes}</span> workers
              </p>
            </div>
          </StatCard>
        </div>

        <!-- Swarm ID Info -->
        <div class="bg-[rgb(var(--color-bg-secondary))] rounded-lg border border-[rgb(var(--color-border-primary))] p-5">
          <h3 class="text-sm font-semibold text-[rgb(var(--color-text-primary))] mb-3 flex items-center gap-2">
            <Activity class="w-4 h-4" />
            Swarm Info
          </h3>
          <div class="grid grid-cols-2 md:grid-cols-4 gap-4">
            <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
              <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-1">Swarm ID</dt>
              <dd class="text-xs font-mono text-[rgb(var(--color-text-primary))] truncate">{swarmInfo.swarmId}</dd>
            </div>
            <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
              <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-1">Node ID</dt>
              <dd class="text-xs font-mono text-[rgb(var(--color-text-primary))] truncate">{swarmInfo.nodeId}</dd>
            </div>
            <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
              <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-1">Role</dt>
              <dd class="text-xs font-semibold text-[rgb(var(--color-text-primary))]">{swarmInfo.isManager ? 'Manager' : 'Worker'}</dd>
            </div>
            <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
              <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-1">Total Nodes</dt>
              <dd class="text-xs font-semibold text-[rgb(var(--color-text-primary))]">{swarmInfo.managers + swarmInfo.workers}</dd>
            </div>
          </div>
        </div>

        <!-- Quick Navigation -->
        <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
          <!-- Nodes Summary -->
          <a href="#/swarm/nodes" class="group bg-[rgb(var(--color-bg-secondary))] rounded-lg border border-[rgb(var(--color-border-primary))] p-5 hover:border-[rgb(var(--color-border-secondary))] transition-all">
            <div class="flex items-center justify-between mb-3">
              <h3 class="text-sm font-semibold text-[rgb(var(--color-text-primary))] flex items-center gap-2">
                <Server class="w-4 h-4" /> Nodes
              </h3>
              <ArrowRight class="w-4 h-4 text-[rgb(var(--color-text-tertiary))] group-hover:text-[rgb(var(--color-accent-blue))] transition-colors" />
            </div>
            <div class="space-y-2">
              {#each nodes.slice(0, 3) as node}
                <div class="flex items-center gap-2 text-xs">
                  <StatusDot status={node.status === 'READY' ? 'running' : 'stopped'} size="sm" />
                  <span class="text-[rgb(var(--color-text-primary))] flex-1 truncate">{node.hostname}</span>
                  <Badge variant={node.role === 'MANAGER' ? 'info' : 'default'} size="xs">{node.role}</Badge>
                </div>
              {/each}
              {#if nodes.length > 3}
                <p class="text-[10px] text-[rgb(var(--color-text-tertiary))]">+{nodes.length - 3} more</p>
              {/if}
            </div>
          </a>

          <!-- Services Summary -->
          <a href="#/swarm/services" class="group bg-[rgb(var(--color-bg-secondary))] rounded-lg border border-[rgb(var(--color-border-primary))] p-5 hover:border-[rgb(var(--color-border-secondary))] transition-all">
            <div class="flex items-center justify-between mb-3">
              <h3 class="text-sm font-semibold text-[rgb(var(--color-text-primary))] flex items-center gap-2">
                <Layers class="w-4 h-4" /> Services
              </h3>
              <ArrowRight class="w-4 h-4 text-[rgb(var(--color-text-tertiary))] group-hover:text-[rgb(var(--color-accent-blue))] transition-colors" />
            </div>
            <div class="space-y-2">
              {#each services.slice(0, 3) as svc}
                <div class="flex items-center gap-2 text-xs">
                  <StatusDot status={svc.replicasRunning >= svc.replicasDesired ? 'running' : svc.replicasRunning > 0 ? 'warning' : 'stopped'} size="sm" />
                  <span class="text-[rgb(var(--color-text-primary))] flex-1 truncate">{svc.name}</span>
                  <span class="text-[rgb(var(--color-text-secondary))] font-mono">{svc.replicasRunning}/{svc.replicasDesired}</span>
                </div>
              {/each}
              {#if services.length > 3}
                <p class="text-[10px] text-[rgb(var(--color-text-tertiary))]">+{services.length - 3} more</p>
              {/if}
            </div>
          </a>

          <!-- Stacks Summary -->
          <a href="#/swarm/stacks" class="group bg-[rgb(var(--color-bg-secondary))] rounded-lg border border-[rgb(var(--color-border-primary))] p-5 hover:border-[rgb(var(--color-border-secondary))] transition-all">
            <div class="flex items-center justify-between mb-3">
              <h3 class="text-sm font-semibold text-[rgb(var(--color-text-primary))] flex items-center gap-2">
                <Box class="w-4 h-4" /> Stacks
              </h3>
              <ArrowRight class="w-4 h-4 text-[rgb(var(--color-text-tertiary))] group-hover:text-[rgb(var(--color-accent-blue))] transition-colors" />
            </div>
            <div class="space-y-2">
              {#each stacks.slice(0, 3) as stack}
                <div class="flex items-center gap-2 text-xs">
                  <StatusDot status={stack.replicasRunning >= stack.replicasDesired ? 'running' : 'warning'} size="sm" />
                  <span class="text-[rgb(var(--color-text-primary))] flex-1 truncate">{stack.namespace}</span>
                  <span class="text-[rgb(var(--color-text-secondary))]">{stack.serviceCount} svc</span>
                </div>
              {/each}
              {#if stacks.length > 3}
                <p class="text-[10px] text-[rgb(var(--color-text-tertiary))]">+{stacks.length - 3} more</p>
              {/if}
              {#if stacks.length === 0}
                <p class="text-xs text-[rgb(var(--color-text-tertiary))]">No stacks deployed</p>
              {/if}
            </div>
          </a>
        </div>
      </div>
    {/if}
  </div>
</div>
