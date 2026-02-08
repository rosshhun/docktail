<script lang="ts">
  import { link } from 'svelte-spa-router';
  import { ArrowLeft, Layers, Activity, FileText, HeartPulse, RotateCw } from '@lucide/svelte';
  import Badge from '../lib/common/Badge.svelte';
  import StatusDot from '../lib/common/StatusDot.svelte';
  import Breadcrumbs from '../lib/common/Breadcrumbs.svelte';
  import LoadingState from '../lib/common/LoadingState.svelte';
  import DataTable from '../lib/common/DataTable.svelte';
  import StatCard from '../lib/common/StatCard.svelte';
  import {
    fetchAgents,
    fetchSwarmInfo,
    fetchStack,
    fetchStackHealth,
    subscribeToStackLogs,
    type StackView,
    type StackHealthView,
    type LogEvent,
    GraphQLError,
  } from '../lib/api';

  let { params = { name: '' } }: { params?: { name: string } } = $props();

  let stack = $state<StackView | null>(null);
  let health = $state<StackHealthView | null>(null);
  let isLoading = $state(true);
  let error = $state<GraphQLError | null>(null);
  let swarmAgentId = $state('');
  let activeTab = $state<'overview' | 'health' | 'logs'>('overview');
  let logEntries = $state<LogEvent[]>([]);
  let logUnsub: (() => void) | null = null;
  let logContainerEl: HTMLDivElement | undefined = $state(undefined);
  let autoScroll = $state(true);

  $effect(() => { if (params.name) loadData(); return () => { if (logUnsub) logUnsub(); }; });

  $effect(() => {
    if (activeTab === 'logs' && swarmAgentId && params.name && !logUnsub) {
      startLogStream();
    } else if (activeTab !== 'logs' && logUnsub) {
      logUnsub();
      logUnsub = null;
    }
  });

  $effect(() => {
    if (logEntries.length && autoScroll && logContainerEl) {
      logContainerEl.scrollTop = logContainerEl.scrollHeight;
    }
  });

  async function loadData() {
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
        stack = await fetchStack(params.name, swarmAgentId);
        try { health = await fetchStackHealth(params.name, swarmAgentId); } catch { /* skip */ }
      }
    } catch (err: any) {
      error = err instanceof GraphQLError ? err : new GraphQLError(err.message, 'INTERNAL_SERVER_ERROR');
    } finally {
      isLoading = false;
    }
  }

  function startLogStream() {
    logEntries = [];
    logUnsub = subscribeToStackLogs(params.name, swarmAgentId, (log) => {
      logEntries = [...logEntries, log].slice(-500);
    });
  }

  function healthVariant(status?: string): 'success' | 'warning' | 'error' | 'default' {
    if (!status) return 'default';
    switch (status.toUpperCase()) {
      case 'HEALTHY': return 'success';
      case 'DEGRADED': return 'warning';
      case 'UNHEALTHY': return 'error';
      default: return 'default';
    }
  }

  function statusDot(status?: string): 'running' | 'paused' | 'stopped' {
    if (!status) return 'running';
    switch (status.toUpperCase()) {
      case 'HEALTHY': return 'running';
      case 'DEGRADED': return 'paused';
      default: return 'stopped';
    }
  }
</script>

<div class="h-full flex flex-col bg-[rgb(var(--color-bg-primary))]">
  <div class="border-b border-[rgb(var(--color-border-primary))] px-8 py-2 sticky top-0 bg-[rgb(var(--color-bg-primary))] z-10 shadow-sm">
    {#if stack}
      <div class="flex items-center justify-between mb-4 mt-3">
        <div class="flex items-center gap-3 flex-1">
          <a use:link href="/swarm/stacks" class="flex items-center justify-center w-7 h-7 rounded hover:bg-[rgb(var(--color-bg-secondary))] border border-[rgb(var(--color-border-primary))] text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))] transition-all">
            <ArrowLeft class="w-3.5 h-3.5" />
          </a>
          <Breadcrumbs items={[{ label: 'Swarm', href: '/swarm' }, { label: 'Stacks', href: '/swarm/stacks' }, { label: stack.namespace }]} />
          {#if health}
            <Badge variant={healthVariant(health.status)} size="sm">{health.status}</Badge>
          {/if}
        </div>
      </div>
      <div class="flex items-center gap-1">
        <button onclick={() => activeTab = 'overview'} class="px-3 py-1.5 cursor-pointer text-sm font-medium transition-colors {activeTab === 'overview' ? 'text-[rgb(var(--color-accent-blue))] border-b-2 border-[rgb(var(--color-accent-blue))]' : 'text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))]'}">
          <Layers class="w-3.5 h-3.5 inline-block mr-1.5" />Overview
        </button>
        <button onclick={() => activeTab = 'health'} class="px-3 py-1.5 cursor-pointer text-sm font-medium transition-colors {activeTab === 'health' ? 'text-[rgb(var(--color-accent-blue))] border-b-2 border-[rgb(var(--color-accent-blue))]' : 'text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))]'}">
          <HeartPulse class="w-3.5 h-3.5 inline-block mr-1.5" />Health
        </button>
        <button onclick={() => activeTab = 'logs'} class="px-3 py-1.5 cursor-pointer text-sm font-medium transition-colors {activeTab === 'logs' ? 'text-[rgb(var(--color-accent-blue))] border-b-2 border-[rgb(var(--color-accent-blue))]' : 'text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))]'}">
          <FileText class="w-3.5 h-3.5 inline-block mr-1.5" />Logs
        </button>
      </div>
    {/if}
  </div>

  <div class="flex-1 overflow-auto">
    {#if isLoading}
      <LoadingState message="Loading stack..." />
    {:else if error || !stack}
      <div class="p-8 text-center">
        <p class="text-sm text-red-400">{error?.getUserMessage() || 'Stack not found'}</p>
        <a use:link href="/swarm/stacks" class="text-xs text-[rgb(var(--color-accent-blue))] hover:underline mt-2 inline-block">Back to Stacks</a>
      </div>
    {:else}
      <div class="px-8 py-5 space-y-4">
        {#if activeTab === 'overview'}
          <!-- Stats -->
          <div class="grid grid-cols-1 md:grid-cols-4 gap-4">
            <StatCard title="Services" icon={Layers}>
              <div class="text-center"><div class="text-3xl font-bold text-[rgb(var(--color-text-primary))]">{stack.serviceCount}</div></div>
            </StatCard>
            <StatCard title="Running" icon={Activity}>
              <div class="text-center">
                <div class="text-3xl font-bold text-green-400">{stack.replicasRunning}</div>
                <p class="text-xs text-[rgb(var(--color-text-secondary))] mt-1">of {stack.replicasDesired} desired</p>
              </div>
            </StatCard>
            <StatCard title="Health" icon={HeartPulse}>
              <div class="text-center">
                {#if health}
                  <StatusDot status={statusDot(health.status)} animated={health.status === 'HEALTHY'} size="lg" />
                  <p class="text-xs text-[rgb(var(--color-text-secondary))] mt-2">{health.status}</p>
                {:else}
                  <p class="text-xs text-[rgb(var(--color-text-secondary))]">—</p>
                {/if}
              </div>
            </StatCard>
            <StatCard title="Failed" icon={RotateCw}>
              <div class="text-center">
                <div class="text-3xl font-bold {health && health.totalFailed > 0 ? 'text-red-400' : 'text-[rgb(var(--color-text-primary))]'}">{health?.totalFailed ?? 0}</div>
                <p class="text-xs text-[rgb(var(--color-text-secondary))] mt-1">failed tasks</p>
              </div>
            </StatCard>
          </div>

          <!-- Services in Stack -->
          <div class="bg-[rgb(var(--color-bg-secondary))] rounded-lg border border-[rgb(var(--color-border-primary))] overflow-hidden">
            <div class="px-5 py-3 border-b border-[rgb(var(--color-border-primary))]">
              <h2 class="text-sm font-semibold text-[rgb(var(--color-text-primary))]">Services</h2>
            </div>
            {#if stack.services.length === 0}
              <div class="p-8 text-center text-xs text-[rgb(var(--color-text-secondary))]">No services in this stack</div>
            {:else}
              <DataTable columns={[
                { key: 'status', label: '', width: 'w-10' },
                { key: 'name', label: 'Name' },
                { key: 'image', label: 'Image' },
                { key: 'mode', label: 'Mode', width: 'w-28' },
                { key: 'replicas', label: 'Replicas', width: 'w-28' },
              ]}>
                {#each stack.services as svc}
                  <tr class="hover:bg-[rgb(var(--color-bg-tertiary))]/50 cursor-pointer" onclick={() => window.location.hash = `#/swarm/services/${svc.id}`}>
                    <td class="px-4 py-3">
                      <StatusDot status={svc.replicasRunning >= svc.replicasDesired ? 'running' : svc.replicasRunning > 0 ? 'paused' : 'stopped'} animated={svc.replicasRunning >= svc.replicasDesired} size="md" />
                    </td>
                    <td class="px-4 py-3 text-sm font-medium text-[rgb(var(--color-text-primary))]">{svc.name}</td>
                    <td class="px-4 py-3 text-xs font-mono text-[rgb(var(--color-text-secondary))] truncate max-w-[250px]">{svc.image}</td>
                    <td class="px-4 py-3"><Badge variant={svc.mode === 'REPLICATED' ? 'info' : 'default'} size="xs">{svc.mode}</Badge></td>
                    <td class="px-4 py-3">
                      <span class="text-xs font-medium"><span class="text-green-400">{svc.replicasRunning}</span> / {svc.replicasDesired}</span>
                    </td>
                  </tr>
                {/each}
              </DataTable>
            {/if}
          </div>

        {:else if activeTab === 'health'}
          {#if health}
            <!-- Health Summary -->
            <div class="grid grid-cols-1 md:grid-cols-4 gap-4">
              <StatCard title="Healthy" icon={HeartPulse}>
                <div class="text-center"><div class="text-3xl font-bold text-green-400">{health.healthyServices}</div></div>
              </StatCard>
              <StatCard title="Degraded" icon={HeartPulse}>
                <div class="text-center"><div class="text-3xl font-bold text-amber-400">{health.degradedServices}</div></div>
              </StatCard>
              <StatCard title="Unhealthy" icon={HeartPulse}>
                <div class="text-center"><div class="text-3xl font-bold text-red-400">{health.unhealthyServices}</div></div>
              </StatCard>
              <StatCard title="Total Running" icon={Activity}>
                <div class="text-center">
                  <div class="text-3xl font-bold text-[rgb(var(--color-text-primary))]">{health.totalRunning}</div>
                  <p class="text-xs text-[rgb(var(--color-text-secondary))] mt-1">of {health.totalDesired} desired</p>
                </div>
              </StatCard>
            </div>

            <!-- Per-service health -->
            <DataTable columns={[
              { key: 'status', label: '', width: 'w-10' },
              { key: 'name', label: 'Service' },
              { key: 'health', label: 'Health', width: 'w-28' },
              { key: 'replicas', label: 'Replicas', width: 'w-32' },
              { key: 'failed', label: 'Failed', width: 'w-20' },
              { key: 'updating', label: 'Updating', width: 'w-20' },
              { key: 'errors', label: 'Recent Errors' },
            ]}>
              {#each health.serviceHealths as svc}
                <tr class="hover:bg-[rgb(var(--color-bg-tertiary))]/50">
                  <td class="px-4 py-3"><StatusDot status={statusDot(svc.status)} animated={svc.status === 'HEALTHY'} size="md" /></td>
                  <td class="px-4 py-3 text-sm font-medium text-[rgb(var(--color-text-primary))]">{svc.serviceName}</td>
                  <td class="px-4 py-3"><Badge variant={healthVariant(svc.status)} size="xs">{svc.status}</Badge></td>
                  <td class="px-4 py-3">
                    <span class="text-xs font-medium"><span class="text-green-400">{svc.replicasRunning}</span> / {svc.replicasDesired}</span>
                  </td>
                  <td class="px-4 py-3 text-xs {svc.replicasFailed > 0 ? 'text-red-400 font-semibold' : 'text-[rgb(var(--color-text-secondary))]'}">{svc.replicasFailed}</td>
                  <td class="px-4 py-3">
                    {#if svc.updateInProgress}
                      <Badge variant="warning" size="xs">Yes</Badge>
                    {:else}
                      <span class="text-xs text-[rgb(var(--color-text-secondary))]">No</span>
                    {/if}
                  </td>
                  <td class="px-4 py-3 text-xs text-red-400 truncate max-w-[200px]">{svc.recentErrors.length > 0 ? svc.recentErrors[0] : '—'}</td>
                </tr>
              {/each}
            </DataTable>
          {:else}
            <div class="p-8 text-center text-sm text-[rgb(var(--color-text-secondary))]">Health data not available</div>
          {/if}

        {:else if activeTab === 'logs'}
          <div class="bg-[rgb(var(--color-bg-secondary))] rounded-lg border border-[rgb(var(--color-border-primary))] overflow-hidden flex flex-col" style="height: calc(100vh - 220px);">
            <div class="flex items-center justify-between px-4 py-2 border-b border-[rgb(var(--color-border-primary))]">
              <h3 class="text-xs font-semibold text-[rgb(var(--color-text-secondary))] uppercase">Stack Logs</h3>
              <div class="flex items-center gap-2">
                <label class="flex items-center gap-1.5 text-xs text-[rgb(var(--color-text-secondary))] cursor-pointer">
                  <input type="checkbox" bind:checked={autoScroll} class="rounded text-xs" /> Auto-scroll
                </label>
                <button onclick={() => { if (logUnsub) { logUnsub(); logUnsub = null; } startLogStream(); }} class="text-xs text-[rgb(var(--color-accent-blue))] hover:underline cursor-pointer">Clear & Restart</button>
              </div>
            </div>
            <div bind:this={logContainerEl} class="flex-1 overflow-auto p-4 font-mono text-xs leading-relaxed bg-[rgb(var(--color-bg-tertiary))]/30">
              {#if logEntries.length === 0}
                <p class="text-[rgb(var(--color-text-secondary))]">Waiting for logs...</p>
              {:else}
                {#each logEntries as entry}
                  <div class="hover:bg-[rgb(var(--color-bg-secondary))]/50 px-2 py-0.5 rounded">
                    <span class="text-[rgb(var(--color-text-secondary))]">{new Date(entry.timestamp).toLocaleTimeString()}</span>
                    {#if entry.swarmContext?.serviceName}
                      <span class="text-[rgb(var(--color-accent-blue))]">[{entry.swarmContext.serviceName}]</span>
                    {/if}
                    {#if entry.level}
                      <span class="font-semibold {entry.level === 'ERROR' ? 'text-red-400' : entry.level === 'WARN' ? 'text-amber-400' : 'text-green-400'}">{entry.level}</span>
                    {/if}
                    <span class="text-[rgb(var(--color-text-primary))]">{entry.content}</span>
                  </div>
                {/each}
              {/if}
            </div>
          </div>
        {/if}
      </div>
    {/if}
  </div>
</div>
