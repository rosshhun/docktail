<script lang="ts">
  import { link } from 'svelte-spa-router';
  import { ArrowLeft, Server, Activity, Shield, Cpu, HardDrive } from '@lucide/svelte';
  import Badge from '../lib/common/Badge.svelte';
  import StatusDot from '../lib/common/StatusDot.svelte';
  import Breadcrumbs from '../lib/common/Breadcrumbs.svelte';
  import LoadingState from '../lib/common/LoadingState.svelte';
  import DataTable from '../lib/common/DataTable.svelte';
  import StatCard from '../lib/common/StatCard.svelte';
  import {
    fetchAgents,
    fetchSwarmInfo,
    fetchNode,
    fetchTasks,
    fetchServices,
    updateNode,
    type NodeView,
    type TaskView,
    type ServiceView,
    GraphQLError,
  } from '../lib/api';
  import { formatBytes } from '../lib/utils/formatting';

  let { params = { id: '' } }: { params?: { id: string } } = $props();

  let node = $state<NodeView | null>(null);
  let nodeTasks = $state<TaskView[]>([]);
  let isLoading = $state(true);
  let error = $state<GraphQLError | null>(null);
  let swarmAgentId = $state('');
  let activeTab = $state<'overview' | 'tasks'>('overview');
  let actionMessage = $state('');

  $effect(() => { if (params.id) loadNodeData(); });

  async function loadNodeData() {
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
        node = await fetchNode(params.id, swarmAgentId);
        // Get tasks running on this node
        const allServices = await fetchServices(swarmAgentId);
        const allTasks: TaskView[] = [];
        for (const svc of allServices) {
          try {
            const tasks = await fetchTasks(svc.id, swarmAgentId);
            allTasks.push(...tasks.filter(t => t.nodeId === params.id));
          } catch { /* skip */ }
        }
        nodeTasks = allTasks;
      }
    } catch (err: any) {
      error = err instanceof GraphQLError ? err : new GraphQLError(err.message, 'INTERNAL_SERVER_ERROR');
    } finally {
      isLoading = false;
    }
  }

  async function toggleDrain() {
    if (!node) return;
    try {
      const newAvail = node.availability === 'DRAIN' ? 'active' : 'drain';
      const result = await updateNode(node.id, swarmAgentId, { availability: newAvail });
      actionMessage = result.message;
      if (result.success) setTimeout(() => loadNodeData(), 1500);
    } catch (err: any) {
      actionMessage = err.message;
    }
  }

  const runningTasks = $derived(nodeTasks.filter(t => t.state.toLowerCase() === 'running').length);
</script>

<div class="h-full flex flex-col bg-[rgb(var(--color-bg-primary))]">
  <div class="border-b border-[rgb(var(--color-border-primary))] px-8 py-2 sticky top-0 bg-[rgb(var(--color-bg-primary))] z-10 shadow-sm">
    {#if node}
      <div class="flex items-center justify-between mb-4 mt-3">
        <div class="flex items-center gap-3 flex-1">
          <a use:link href="/swarm/nodes" class="flex items-center justify-center w-7 h-7 rounded hover:bg-[rgb(var(--color-bg-secondary))] border border-[rgb(var(--color-border-primary))] text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))] transition-all">
            <ArrowLeft class="w-3.5 h-3.5" />
          </a>
          <Breadcrumbs items={[{ label: 'Swarm', href: '/swarm' }, { label: 'Nodes', href: '/swarm/nodes' }, { label: node.hostname }]} />
          <Badge variant={node.status === 'READY' ? 'success' : 'error'} size="sm">{node.status}</Badge>
          <Badge variant={node.role === 'MANAGER' ? 'info' : 'default'} size="sm">{node.role}{node.managerStatus?.leader ? ' ★' : ''}</Badge>
        </div>
        <button onclick={toggleDrain} class="text-xs font-medium px-3 py-1.5 rounded border cursor-pointer transition-all {node.availability === 'DRAIN' ? 'border-green-500/30 text-green-400 hover:bg-green-500/10' : 'border-amber-500/30 text-amber-400 hover:bg-amber-500/10'}">
          {node.availability === 'DRAIN' ? 'Activate' : 'Drain'}
        </button>
      </div>
      <div class="flex items-center gap-1">
        <button onclick={() => activeTab = 'overview'} class="px-3 py-1.5 cursor-pointer text-sm font-medium transition-colors {activeTab === 'overview' ? 'text-[rgb(var(--color-accent-blue))] border-b-2 border-[rgb(var(--color-accent-blue))]' : 'text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))]'}">
          <Server class="w-3.5 h-3.5 inline-block mr-1.5" />Overview
        </button>
        <button onclick={() => activeTab = 'tasks'} class="px-3 py-1.5 cursor-pointer text-sm font-medium transition-colors {activeTab === 'tasks' ? 'text-[rgb(var(--color-accent-blue))] border-b-2 border-[rgb(var(--color-accent-blue))]' : 'text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))]'}">
          <Activity class="w-3.5 h-3.5 inline-block mr-1.5" />Tasks ({nodeTasks.length})
        </button>
      </div>
    {/if}
  </div>

  <div class="flex-1 overflow-auto">
    {#if isLoading}
      <LoadingState message="Loading node..." />
    {:else if error || !node}
      <div class="p-8 text-center">
        <p class="text-sm text-red-400">{error?.getUserMessage() || 'Node not found'}</p>
        <a use:link href="/swarm/nodes" class="text-xs text-[rgb(var(--color-accent-blue))] hover:underline mt-2 inline-block">Back to Nodes</a>
      </div>
    {:else}
      <div class="px-8 py-5 space-y-4">
        {#if actionMessage}
          <div class="px-4 py-2 bg-[rgb(var(--color-bg-secondary))] border border-[rgb(var(--color-border-primary))] rounded text-xs text-[rgb(var(--color-text-secondary))]">{actionMessage}</div>
        {/if}

        {#if activeTab === 'overview'}
          <!-- Stats -->
          <div class="grid grid-cols-1 md:grid-cols-4 gap-4">
            <StatCard title="Status" icon={Activity}>
              <div class="text-center">
                <StatusDot status={node.status === 'READY' ? 'running' : 'stopped'} animated={node.status === 'READY'} size="md" />
                <p class="text-xs text-[rgb(var(--color-text-secondary))] mt-2">{node.status}</p>
              </div>
            </StatCard>
            <StatCard title="Tasks" icon={Activity} value={nodeTasks.length}>
              <div class="text-center">
                <div class="text-3xl font-bold text-[rgb(var(--color-text-primary))]">{nodeTasks.length}</div>
                <p class="text-xs text-[rgb(var(--color-text-secondary))] mt-1"><span class="text-green-400 font-semibold">{runningTasks}</span> running</p>
              </div>
            </StatCard>
            <StatCard title="CPUs" icon={Cpu} value={(Number(node.nanoCpus) / 1e9).toFixed(0)} subtitle="cores" />
            <StatCard title="Memory" icon={HardDrive} value={formatBytes(Number(node.memoryBytes))} subtitle="total" />
          </div>

          <!-- Node Info -->
          <div class="bg-[rgb(var(--color-bg-secondary))] rounded-lg border border-[rgb(var(--color-border-primary))] p-6">
            <h2 class="text-sm font-semibold text-[rgb(var(--color-text-primary))] mb-4 flex items-center gap-2">
              <Server class="w-4 h-4" /> Node Details
            </h2>
            <div class="grid grid-cols-2 md:grid-cols-3 gap-4">
              <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase mb-1">Node ID</dt>
                <dd class="text-xs font-mono text-[rgb(var(--color-text-primary))] truncate">{node.id}</dd>
              </div>
              <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase mb-1">Address</dt>
                <dd class="text-xs font-mono text-[rgb(var(--color-text-primary))]">{node.addr}</dd>
              </div>
              <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase mb-1">Engine</dt>
                <dd class="text-xs text-[rgb(var(--color-text-primary))]">{node.engineVersion}</dd>
              </div>
              <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase mb-1">OS</dt>
                <dd class="text-xs text-[rgb(var(--color-text-primary))]">{node.os}</dd>
              </div>
              <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase mb-1">Architecture</dt>
                <dd class="text-xs text-[rgb(var(--color-text-primary))]">{node.architecture}</dd>
              </div>
              <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase mb-1">Availability</dt>
                <dd><Badge variant={node.availability === 'ACTIVE' ? 'success' : node.availability === 'DRAIN' ? 'error' : 'warning'} size="xs">{node.availability}</Badge></dd>
              </div>
            </div>
          </div>

          <!-- Manager Status -->
          {#if node.managerStatus}
            <div class="bg-[rgb(var(--color-bg-secondary))] rounded-lg border border-[rgb(var(--color-border-primary))] p-6">
              <h2 class="text-sm font-semibold text-[rgb(var(--color-text-primary))] mb-4 flex items-center gap-2">
                <Shield class="w-4 h-4" /> Manager Status
              </h2>
              <div class="grid grid-cols-3 gap-4">
                <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                  <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase mb-1">Leader</dt>
                  <dd class="text-xs font-semibold {node.managerStatus.leader ? 'text-green-400' : 'text-[rgb(var(--color-text-primary))]'}">{node.managerStatus.leader ? 'Yes ★' : 'No'}</dd>
                </div>
                <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                  <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase mb-1">Reachability</dt>
                  <dd class="text-xs text-[rgb(var(--color-text-primary))]">{node.managerStatus.reachability}</dd>
                </div>
                <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                  <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase mb-1">Address</dt>
                  <dd class="text-xs font-mono text-[rgb(var(--color-text-primary))]">{node.managerStatus.addr}</dd>
                </div>
              </div>
            </div>
          {/if}

          <!-- Labels -->
          {#if node.labels.length > 0}
            <div class="bg-[rgb(var(--color-bg-secondary))] rounded-lg border border-[rgb(var(--color-border-primary))] overflow-hidden">
              <div class="px-5 py-3 border-b border-[rgb(var(--color-border-primary))]">
                <h2 class="text-sm font-semibold text-[rgb(var(--color-text-primary))]">Labels <Badge variant="default" size="xs">{node.labels.length}</Badge></h2>
              </div>
              <table class="w-full">
                <tbody class="divide-y divide-[rgb(var(--color-border-primary))]/50">
                  {#each node.labels as label}
                    <tr class="hover:bg-[rgb(var(--color-bg-tertiary))]/30"><td class="px-6 py-2.5 text-xs font-mono text-[rgb(var(--color-text-secondary))]">{label.key}</td><td class="px-6 py-2.5 text-xs font-mono text-[rgb(var(--color-text-primary))]">{label.value}</td></tr>
                  {/each}
                </tbody>
              </table>
            </div>
          {/if}

        {:else if activeTab === 'tasks'}
          <DataTable columns={[
            { key: 'status', label: 'State', width: 'w-16' },
            { key: 'service', label: 'Service' },
            { key: 'task', label: 'Task ID' },
            { key: 'slot', label: 'Slot', width: 'w-16' },
            { key: 'desired', label: 'Desired', width: 'w-24' },
            { key: 'message', label: 'Message' },
          ]}>
            {#each nodeTasks as task (task.id)}
              <tr class="hover:bg-[rgb(var(--color-bg-tertiary))]/50">
                <td class="px-4 py-3"><StatusDot status={task.state.toLowerCase() === 'running' ? 'running' : 'stopped'} animated={task.state.toLowerCase() === 'running'} size="md" /></td>
                <td class="px-4 py-3 text-sm text-[rgb(var(--color-text-primary))]">{task.serviceName}</td>
                <td class="px-4 py-3 text-xs font-mono text-[rgb(var(--color-text-secondary))] truncate max-w-[200px]">{task.id}</td>
                <td class="px-4 py-3 text-xs text-[rgb(var(--color-text-secondary))]">{task.slot}</td>
                <td class="px-4 py-3"><Badge variant="default" size="xs">{task.desiredState}</Badge></td>
                <td class="px-4 py-3 text-xs text-[rgb(var(--color-text-secondary))] truncate">{task.statusMessage || '-'}</td>
              </tr>
            {/each}
          </DataTable>
        {/if}
      </div>
    {/if}
  </div>
</div>
