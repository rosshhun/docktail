<script lang="ts">
  import { link, push } from 'svelte-spa-router';
  import { ArrowLeft, Layers, Activity, Terminal, Server, Settings, Minus, Plus, RefreshCw } from '@lucide/svelte';
  import Badge from '../lib/common/Badge.svelte';
  import StatusDot from '../lib/common/StatusDot.svelte';
  import Breadcrumbs from '../lib/common/Breadcrumbs.svelte';
  import LoadingState from '../lib/common/LoadingState.svelte';
  import DataTable from '../lib/common/DataTable.svelte';
  import {
    fetchService,
    fetchTasks,
    fetchAgents,
    fetchSwarmInfo,
    updateServiceReplicas,
    subscribeToServiceLogs,
    type ServiceView,
    type TaskView,
    type LogEvent,
    GraphQLError,
  } from '../lib/api';
  import { formatNsDuration } from '../lib/utils/formatting';
  import { logger } from '../lib/utils/logger';

  let { params = { id: '' } }: { params?: { id: string } } = $props();

  let service = $state<ServiceView | null>(null);
  let tasks = $state<TaskView[]>([]);
  let isLoading = $state(true);
  let error = $state<GraphQLError | null>(null);
  let activeTab = $state<'overview' | 'tasks' | 'logs' | 'config'>('overview');
  let swarmAgentId = $state('');

  // Scaling state
  let desiredReplicas = $state(0);
  let isScaling = $state(false);
  let scaleMessage = $state('');

  // Logs state
  let logs = $state<LogEvent[]>([]);
  let logUnsubscribe: (() => void) | null = null;
  let isStreaming = $state(false);
  let logContainer = $state<HTMLDivElement>();

  $effect(() => {
    if (params.id) loadServiceData();
    return () => { logUnsubscribe?.(); };
  });

  async function loadServiceData() {
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
        service = await fetchService(params.id, swarmAgentId);
        if (service) {
          desiredReplicas = service.replicasDesired;
          tasks = await fetchTasks(params.id, swarmAgentId);
        }
      }
    } catch (err: any) {
      error = err instanceof GraphQLError ? err : new GraphQLError(err.message, 'INTERNAL_SERVER_ERROR');
    } finally {
      isLoading = false;
    }
  }

  async function handleScale() {
    if (!service || isScaling) return;
    try {
      isScaling = true;
      scaleMessage = '';
      const result = await updateServiceReplicas(service.id, swarmAgentId, desiredReplicas);
      scaleMessage = result.success ? `Scaled to ${desiredReplicas} replicas` : result.message;
      if (result.success) {
        setTimeout(() => loadServiceData(), 2000);
      }
    } catch (err: any) {
      scaleMessage = err.message;
    } finally {
      isScaling = false;
    }
  }

  let logsInitialized = $state(false);
  $effect(() => {
    if (activeTab === 'logs' && service && !logsInitialized) {
      logsInitialized = true;
      logUnsubscribe = subscribeToServiceLogs(
        service.id,
        swarmAgentId,
        (log) => { logs = [...logs, log]; },
        (err) => logger.error('[ServiceLogs] Error:', err),
      );
      isStreaming = true;
    }
    if (activeTab !== 'logs' && logsInitialized) {
      logUnsubscribe?.();
      logs = [];
      logsInitialized = false;
      isStreaming = false;
    }
  });

  $effect(() => {
    if (logContainer && logs.length > 0) {
      requestAnimationFrame(() => { if (logContainer) logContainer.scrollTop = logContainer.scrollHeight; });
    }
  });

  function taskStatusDot(state: string): 'running' | 'stopped' | 'warning' {
    if (state.toLowerCase() === 'running') return 'running';
    if (state.toLowerCase() === 'failed' || state.toLowerCase() === 'rejected') return 'stopped';
    return 'warning';
  }

  function timeAgo(d: string): string {
    const s = Math.floor((Date.now() - new Date(d).getTime()) / 1000);
    if (s < 60) return 'just now';
    const m = Math.floor(s / 60);
    if (m < 60) return `${m}m ago`;
    const h = Math.floor(m / 60);
    if (h < 24) return `${h}h ago`;
    return `${Math.floor(h / 24)}d ago`;
  }
</script>

<div class="h-full flex flex-col bg-[rgb(var(--color-bg-primary))]">
  <!-- Header -->
  <div class="border-b border-[rgb(var(--color-border-primary))] px-8 py-2 sticky top-0 bg-[rgb(var(--color-bg-primary))] z-10 shadow-sm">
    {#if service}
      <div class="flex items-center justify-between mb-4 mt-3">
        <div class="flex items-center gap-3 flex-1">
          <a use:link href="/swarm/services" class="flex items-center justify-center w-7 h-7 rounded hover:bg-[rgb(var(--color-bg-secondary))] border border-[rgb(var(--color-border-primary))] text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))] transition-all">
            <ArrowLeft class="w-3.5 h-3.5" />
          </a>
          <Breadcrumbs items={[{ label: 'Swarm', href: '/swarm' }, { label: 'Services', href: '/swarm/services' }, { label: service.name }]} />
          <Badge variant={service.replicasRunning >= service.replicasDesired ? 'success' : 'warning'} size="sm">
            {service.replicasRunning}/{service.replicasDesired}
          </Badge>
          <Badge variant="default" size="sm">{service.mode}</Badge>
        </div>
      </div>
      <div class="flex items-center gap-1">
        <button onclick={() => activeTab = 'overview'} class="px-3 py-1.5 cursor-pointer text-sm font-medium transition-colors {activeTab === 'overview' ? 'text-[rgb(var(--color-accent-blue))] border-b-2 border-[rgb(var(--color-accent-blue))]' : 'text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))]'}">
          <Layers class="w-3.5 h-3.5 inline-block mr-1.5" />Overview
        </button>
        <button onclick={() => activeTab = 'tasks'} class="px-3 py-1.5 cursor-pointer text-sm font-medium transition-colors {activeTab === 'tasks' ? 'text-[rgb(var(--color-accent-blue))] border-b-2 border-[rgb(var(--color-accent-blue))]' : 'text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))]'}">
          <Activity class="w-3.5 h-3.5 inline-block mr-1.5" />Tasks
        </button>
        <button onclick={() => activeTab = 'logs'} class="px-3 py-1.5 cursor-pointer text-sm font-medium transition-colors {activeTab === 'logs' ? 'text-[rgb(var(--color-accent-blue))] border-b-2 border-[rgb(var(--color-accent-blue))]' : 'text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))]'}">
          <Terminal class="w-3.5 h-3.5 inline-block mr-1.5" />Logs
        </button>
        <button onclick={() => activeTab = 'config'} class="px-3 py-1.5 cursor-pointer text-sm font-medium transition-colors {activeTab === 'config' ? 'text-[rgb(var(--color-accent-blue))] border-b-2 border-[rgb(var(--color-accent-blue))]' : 'text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))]'}">
          <Settings class="w-3.5 h-3.5 inline-block mr-1.5" />Config
        </button>
      </div>
    {/if}
  </div>

  <!-- Content -->
  <div class="flex-1 overflow-auto">
    {#if isLoading}
      <LoadingState message="Loading service..." />
    {:else if error}
      <div class="p-8 text-center">
        <p class="text-sm text-red-400">{error.getUserMessage()}</p>
        <a use:link href="/swarm/services" class="text-xs text-[rgb(var(--color-accent-blue))] hover:underline mt-2 inline-block">Back to Services</a>
      </div>
    {:else if !service}
      <div class="p-8 text-center"><p class="text-sm text-[rgb(var(--color-text-secondary))]">Service not found</p></div>
    {:else}
      <div class="px-8 py-5 space-y-4">
        {#if activeTab === 'overview'}
          <!-- Service Overview -->
          <div class="bg-[rgb(var(--color-bg-secondary))] rounded-lg border border-[rgb(var(--color-border-primary))] p-6">
            <h2 class="text-sm font-semibold text-[rgb(var(--color-text-primary))] mb-4 flex items-center gap-2">
              <Layers class="w-4 h-4" /> Service Overview
            </h2>
            <dl class="space-y-4">
              <div class="grid grid-cols-2 md:grid-cols-3 gap-4">
                <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                  <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-1">Service ID</dt>
                  <dd class="text-xs font-mono text-[rgb(var(--color-text-primary))] truncate">{service.id}</dd>
                </div>
                <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                  <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-1">Image</dt>
                  <dd class="text-xs font-mono text-[rgb(var(--color-text-primary))] truncate">{service.image}</dd>
                </div>
                <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                  <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-1">Mode</dt>
                  <dd class="text-xs font-semibold text-[rgb(var(--color-text-primary))]">{service.mode}</dd>
                </div>
              </div>
              <div class="grid grid-cols-2 gap-4">
                <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                  <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-1">Created</dt>
                  <dd class="text-xs text-[rgb(var(--color-text-primary))]">{new Date(service.createdAt).toLocaleString()}</dd>
                </div>
                <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                  <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-1">Updated</dt>
                  <dd class="text-xs text-[rgb(var(--color-text-primary))]">{new Date(service.updatedAt).toLocaleString()}</dd>
                </div>
              </div>
              {#if service.updateStatus}
                <div class="bg-blue-500/10 rounded-md p-3 border border-blue-500/20">
                  <dt class="text-[10px] font-bold text-blue-400 uppercase tracking-wider mb-1">Update Status</dt>
                  <dd class="text-xs text-blue-300">{service.updateStatus.state} — {service.updateStatus.message}</dd>
                </div>
              {/if}
            </dl>
          </div>

          <!-- Scaling -->
          {#if service.mode === 'REPLICATED'}
            <div class="bg-[rgb(var(--color-bg-secondary))] rounded-lg border border-[rgb(var(--color-border-primary))] p-6">
              <h2 class="text-sm font-semibold text-[rgb(var(--color-text-primary))] mb-4">Scale Replicas</h2>
              <div class="flex items-center gap-3">
                <button onclick={() => { if (desiredReplicas > 0) desiredReplicas--; }} class="w-8 h-8 rounded border border-[rgb(var(--color-border-primary))] flex items-center justify-center hover:bg-[rgb(var(--color-bg-tertiary))] cursor-pointer"><Minus class="w-4 h-4" /></button>
                <span class="text-2xl font-bold text-[rgb(var(--color-text-primary))] w-12 text-center">{desiredReplicas}</span>
                <button onclick={() => desiredReplicas++} class="w-8 h-8 rounded border border-[rgb(var(--color-border-primary))] flex items-center justify-center hover:bg-[rgb(var(--color-bg-tertiary))] cursor-pointer"><Plus class="w-4 h-4" /></button>
                <button onclick={handleScale} disabled={isScaling || desiredReplicas === service.replicasDesired} class="ml-4 px-4 py-2 text-xs font-medium rounded bg-[rgb(var(--color-accent-blue))] text-white hover:opacity-90 disabled:opacity-50 cursor-pointer">
                  {isScaling ? 'Scaling...' : 'Apply'}
                </button>
                {#if scaleMessage}
                  <span class="text-xs text-[rgb(var(--color-text-secondary))]">{scaleMessage}</span>
                {/if}
              </div>
            </div>
          {/if}

          <!-- Ports -->
          {#if service.ports.length > 0}
            <div class="bg-[rgb(var(--color-bg-secondary))] rounded-lg border border-[rgb(var(--color-border-primary))] p-6">
              <h2 class="text-sm font-semibold text-[rgb(var(--color-text-primary))] mb-3">Published Ports</h2>
              <div class="flex flex-wrap gap-2">
                {#each service.ports as port}
                  <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md px-3 py-2 border border-[rgb(var(--color-border-primary))]/50 text-xs font-mono">
                    <span class="text-[rgb(var(--color-accent-blue))]">{port.publishedPort}</span>
                    <span class="text-[rgb(var(--color-text-tertiary))]">→</span>
                    <span class="text-[rgb(var(--color-text-primary))]">{port.targetPort}/{port.protocol}</span>
                    <span class="text-[10px] text-[rgb(var(--color-text-tertiary))] ml-1">({port.publishMode})</span>
                  </div>
                {/each}
              </div>
            </div>
          {/if}

        {:else if activeTab === 'tasks'}
          <DataTable columns={[
            { key: 'status', label: 'State', width: 'w-16' },
            { key: 'id', label: 'Task ID' },
            { key: 'slot', label: 'Slot', width: 'w-16' },
            { key: 'node', label: 'Node', width: 'w-32' },
            { key: 'desired', label: 'Desired', width: 'w-24' },
            { key: 'message', label: 'Message' },
            { key: 'updated', label: 'Updated', width: 'w-28' },
          ]}>
            {#each tasks as task (task.id)}
              <tr class="hover:bg-[rgb(var(--color-bg-tertiary))]/50 transition-colors">
                <td class="px-4 py-3"><StatusDot status={taskStatusDot(task.state)} animated={task.state.toLowerCase() === 'running'} size="md" /></td>
                <td class="px-4 py-3">
                  <div class="text-xs font-mono text-[rgb(var(--color-text-primary))] truncate max-w-[200px]">{task.id}</div>
                  <div class="text-[10px] text-[rgb(var(--color-text-tertiary))]">{task.state}</div>
                </td>
                <td class="px-4 py-3 text-xs text-[rgb(var(--color-text-secondary))]">{task.slot}</td>
                <td class="px-4 py-3 text-xs text-[rgb(var(--color-text-secondary))] truncate">{task.nodeId.slice(0, 12)}</td>
                <td class="px-4 py-3"><Badge variant="default" size="xs">{task.desiredState}</Badge></td>
                <td class="px-4 py-3 text-xs text-[rgb(var(--color-text-secondary))] truncate max-w-[300px]">{task.statusMessage || '-'}</td>
                <td class="px-4 py-3 text-xs text-[rgb(var(--color-text-secondary))]">{timeAgo(task.updatedAt)}</td>
              </tr>
            {/each}
          </DataTable>

        {:else if activeTab === 'logs'}
          <div class="bg-[#0D0E12] rounded-lg border border-[rgb(var(--color-border-primary))] overflow-hidden h-[600px] flex flex-col">
            <div class="px-4 py-2 border-b border-[rgb(var(--color-border-primary))] flex items-center justify-between">
              <span class="text-xs text-[rgb(var(--color-text-secondary))]">
                {#if isStreaming}
                  <StatusDot status="running" animated size="sm" />
                  <span class="ml-1">Streaming {logs.length} logs</span>
                {:else}
                  Waiting...
                {/if}
              </span>
            </div>
            <div bind:this={logContainer} class="flex-1 overflow-auto p-4 font-mono text-xs">
              {#each logs as log}
                <div class="py-0.5 leading-relaxed {log.level === 'STDERR' ? 'text-red-400' : 'text-[rgb(var(--color-text-primary))]'}">
                  <span class="text-[rgb(var(--color-text-tertiary))] mr-2">{new Date(log.timestamp).toLocaleTimeString()}</span>
                  {log.content}
                </div>
              {/each}
            </div>
          </div>

        {:else if activeTab === 'config'}
          <div class="space-y-4">
            {#if service.placementConstraints.length > 0}
              <div class="bg-[rgb(var(--color-bg-secondary))] rounded-lg border border-[rgb(var(--color-border-primary))] p-6">
                <h2 class="text-sm font-semibold text-[rgb(var(--color-text-primary))] mb-3">Placement Constraints</h2>
                <div class="space-y-1">
                  {#each service.placementConstraints as c}
                    <div class="text-xs font-mono text-[rgb(var(--color-text-primary))] bg-[rgb(var(--color-bg-tertiary))]/20 rounded px-3 py-2">{c}</div>
                  {/each}
                </div>
              </div>
            {/if}

            {#if service.updateConfig}
              <div class="bg-[rgb(var(--color-bg-secondary))] rounded-lg border border-[rgb(var(--color-border-primary))] p-6">
                <h2 class="text-sm font-semibold text-[rgb(var(--color-text-primary))] mb-3">Update Config</h2>
                <div class="grid grid-cols-3 gap-4">
                  <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                    <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase mb-1">Parallelism</dt>
                    <dd class="text-xs text-[rgb(var(--color-text-primary))]">{service.updateConfig.parallelism}</dd>
                  </div>
                  <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                    <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase mb-1">Delay</dt>
                    <dd class="text-xs text-[rgb(var(--color-text-primary))]">{formatNsDuration(service.updateConfig.delayNs)}</dd>
                  </div>
                  <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                    <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase mb-1">Failure Action</dt>
                    <dd class="text-xs text-[rgb(var(--color-text-primary))]">{service.updateConfig.failureAction}</dd>
                  </div>
                </div>
              </div>
            {/if}

            {#if service.restartPolicy}
              <div class="bg-[rgb(var(--color-bg-secondary))] rounded-lg border border-[rgb(var(--color-border-primary))] p-6">
                <h2 class="text-sm font-semibold text-[rgb(var(--color-text-primary))] mb-3">Restart Policy</h2>
                <div class="grid grid-cols-3 gap-4">
                  <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                    <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase mb-1">Condition</dt>
                    <dd class="text-xs text-[rgb(var(--color-text-primary))]">{service.restartPolicy.condition}</dd>
                  </div>
                  <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                    <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase mb-1">Max Attempts</dt>
                    <dd class="text-xs text-[rgb(var(--color-text-primary))]">{service.restartPolicy.maxAttempts || 'Unlimited'}</dd>
                  </div>
                  <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                    <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase mb-1">Delay</dt>
                    <dd class="text-xs text-[rgb(var(--color-text-primary))]">{formatNsDuration(service.restartPolicy.delayNs)}</dd>
                  </div>
                </div>
              </div>
            {/if}

            {#if service.labels.length > 0}
              <div class="bg-[rgb(var(--color-bg-secondary))] rounded-lg border border-[rgb(var(--color-border-primary))] overflow-hidden">
                <div class="px-5 py-3 border-b border-[rgb(var(--color-border-primary))]">
                  <h2 class="text-sm font-semibold text-[rgb(var(--color-text-primary))]">Labels <Badge variant="default" size="xs">{service.labels.length}</Badge></h2>
                </div>
                <table class="w-full">
                  <tbody class="divide-y divide-[rgb(var(--color-border-primary))]/50">
                    {#each service.labels as label}
                      <tr class="hover:bg-[rgb(var(--color-bg-tertiary))]/30 transition-colors">
                        <td class="px-6 py-2.5 text-xs font-mono text-[rgb(var(--color-text-secondary))]">{label.key}</td>
                        <td class="px-6 py-2.5 text-xs font-mono text-[rgb(var(--color-text-primary))]">{label.value}</td>
                      </tr>
                    {/each}
                  </tbody>
                </table>
              </div>
            {/if}
          </div>
        {/if}
      </div>
    {/if}
  </div>
</div>
