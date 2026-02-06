<script lang="ts">
  import { link } from 'svelte-spa-router';
  import { ArrowLeft, Server, Activity, Box, AlertCircle, Info, BarChart3, Zap, Terminal } from '@lucide/svelte';
  import Button from '../lib/common/Button.svelte';
  import Badge from '../lib/common/Badge.svelte';
  import StatusDot from '../lib/common/StatusDot.svelte';
  import Breadcrumbs from '../lib/common/Breadcrumbs.svelte';
  import TabNav from '../lib/common/TabNav.svelte';
  import StatCard from '../lib/common/StatCard.svelte';
  import SectionCard from '../lib/common/SectionCard.svelte';
  import InfoGrid from '../lib/common/InfoGrid.svelte';
  import LoadingState from '../lib/common/LoadingState.svelte';
  import ErrorState from '../lib/common/ErrorState.svelte';
  import EmptyState from '../lib/common/EmptyState.svelte';
  import DataTable from '../lib/common/DataTable.svelte';
  import ContainerListRow from '../lib/containers/ContainerListRow.svelte';
  import AgentHealthMonitor from '../lib/components/AgentHealthMonitor.svelte';
  import RealTimeStats from '../lib/components/RealTimeStats.svelte';
  import { 
    fetchAgents, 
    fetchContainers,
    fetchContainer,
    subscribeToLogs,
    fetchHistoricalLogs,
    type Agent, 
    type Container,
    type ContainerWithAgent,
    type LogEvent,
    GraphQLError 
  } from '../lib/api';
  import { logger } from '../lib/utils/logger';

  // Get agent ID from route params
  let { params = { id: '' } }: { params?: { id: string } } = $props();

  let agent = $state<Agent | null>(null);
  let containers = $state<ContainerWithAgent[]>([]);
  let agentContainer = $state<Container | null>(null); // The agent's own container
  let isLoading = $state(true);
  let error = $state<GraphQLError | null>(null);
  let activeTab = $state<'dashboard' | 'details' | 'stats' | 'realtime' | 'logs'>('dashboard');

  // Logs state (for agent container logs)
  let logs = $state<LogEvent[]>([]);
  let isStreaming = $state(false);
  let isPaused = $state(false);
  let logContainer = $state<HTMLDivElement>();
  let unsubscribe: (() => void) | null = null;
  let logsError = $state<GraphQLError | null>(null);

  // Use $effect to load agent data on mount or when params change
  $effect(() => {
    if (!params.id) {
      error = new GraphQLError('No agent ID provided', 'BAD_REQUEST');
      return;
    }

    loadAgentData();
  });

  async function loadAgentData() {
    try {
      isLoading = true;
      error = null;
      
      // Fetch all agents and containers
      const [agents, allContainers] = await Promise.all([
        fetchAgents(),
        fetchContainers(),
      ]);
      
      // Find the specific agent
      agent = agents.find(a => a.id === params.id) || null;
      
      if (!agent) {
        error = new GraphQLError('Agent not found', 'AGENT_NOT_FOUND');
        return;
      }
      
      // Filter containers for this agent (including agent containers)
      const agentContainers = allContainers.filter(c => c.agentId === params.id);
      
      // Separate agent container from workload containers
      // Many agents are named "docktail-agent" or similar, but some just "agent"
      agentContainer = agentContainers.find(c => 
        c.name.includes('docktail-agent') || 
        c.name === 'agent' || 
        c.name === '/agent' ||
        c.image.includes('docktail/agent')
      ) || null;
      
      // Filter out internal containers from the main workload list
      containers = agentContainers.filter(c => {
        const isInternal = c.name.includes('docktail-agent') || 
                          c.name === 'agent' || 
                          c.name === '/agent' ||
                          c.image.includes('docktail/agent') ||
                          c.name.includes('docktail-cluster');
        return !isInternal;
      });
      
      logger.debug('[AgentDetails] Loaded agent:', agent.name, 'containers:', containers.length, 'agentContainer:', agentContainer?.name);
    } catch (err: any) {
      if (err instanceof GraphQLError) {
        error = err;
      } else {
        error = new GraphQLError(err.message || 'Failed to load agent', 'INTERNAL_SERVER_ERROR');
      }
      logger.error('[AgentDetails] Failed to load:', err);
    } finally {
      isLoading = false;
    }
  }

  // Helper function to calculate time ago
  function timeAgo(dateString: string): string {
    const date = new Date(dateString);
    const now = new Date();
    const seconds = Math.floor((now.getTime() - date.getTime()) / 1000);
    
    if (seconds < 60) return 'just now';
    const minutes = Math.floor(seconds / 60);
    if (minutes < 60) return `${minutes}m ago`;
    const hours = Math.floor(minutes / 60);
    if (hours < 24) return `${hours}h ago`;
    const days = Math.floor(hours / 24);
    if (days < 30) return `${days}d ago`;
    return `${Math.floor(days / 30)}mo ago`;
  }

  const statusVariant = $derived(
    agent?.status === 'HEALTHY' ? 'success' : 
    agent?.status === 'UNHEALTHY' ? 'error' : 
    agent?.status === 'DEGRADED' ? 'warning' : 'default'
  );

  const runningContainers = $derived(containers.filter(c => c.state === 'RUNNING').length);
  const stoppedContainers = $derived(containers.length - runningContainers);

  const tabs = [
    { id: 'dashboard', label: 'Dashboard', icon: Activity },
    { id: 'details', label: 'Details', icon: Info },
    { id: 'stats', label: 'Statistics', icon: BarChart3 },
    { id: 'realtime', label: 'Realtime', icon: Zap },
    { id: 'logs', label: 'Logs', icon: Terminal }
  ];

  // Load logs when logs tab is activated
  let logsInitialized = $state(false);
  $effect(() => {
    if (activeTab === 'logs' && !logsInitialized && agentContainer) {
      logsInitialized = true;
      startStreaming();
    }
  });

  // Auto-scroll logs to bottom when new logs arrive
  $effect(() => {
    if (!isPaused && logContainer && logs.length > 0) {
      requestAnimationFrame(() => {
        if (logContainer) {
          logContainer.scrollTop = logContainer.scrollHeight;
        }
      });
    }
  });

  async function startStreaming() {
    if (!agentContainer || isStreaming) return;

    try {
      logsError = null;
      isStreaming = true;
      isPaused = false;

      // Load historical logs first
      const historicalLogs = await fetchHistoricalLogs(
        agentContainer.id,
        agentContainer.agentId,
        { tail: 100 }
      );
      logs = historicalLogs;

      // Start streaming new logs
      unsubscribe = await subscribeToLogs(
        agentContainer.id,
        agentContainer.agentId,
        (log) => {
          if (!isPaused) {
            logs = [...logs, log];
          }
        },
        (err) => {
          if (err instanceof GraphQLError) {
            logsError = err;
          } else {
            logsError = new GraphQLError(String(err), 'INTERNAL_SERVER_ERROR');
          }
          isStreaming = false;
        }
      );
    } catch (err: any) {
      if (err instanceof GraphQLError) {
        logsError = err;
      } else {
        logsError = new GraphQLError(err.message || 'Failed to stream logs', 'INTERNAL_SERVER_ERROR');
      }
      isStreaming = false;
    }
  }

  function stopStreaming() {
    if (unsubscribe) {
      unsubscribe();
      unsubscribe = null;
    }
    isStreaming = false;
  }

  function togglePause() {
    isPaused = !isPaused;
  }

  function clearLogs() {
    logs = [];
  }

  // Cleanup on unmount
  $effect(() => {
    return () => {
      if (unsubscribe) {
        unsubscribe();
      }
    };
  });
</script>

<div class="h-full flex flex-col bg-[rgb(var(--color-bg-primary))]">
  <!-- Header - Compact style matching Container Details -->
  <div class="border-b border-[rgb(var(--color-border-primary))] px-8 py-2 sticky top-0 bg-[rgb(var(--color-bg-primary))] z-10 shadow-sm">
    {#if agent}
      <!-- Top Row: Breadcrumb & Status -->
      <div class="flex items-center justify-between mb-4 mt-3">
        <!-- Left: Breadcrumb & Status -->
        <div class="flex items-center gap-3 flex-1">
          <a
            use:link
            href="/agents"
            class="flex items-center justify-center w-7 h-7 rounded hover:bg-[rgb(var(--color-bg-secondary))] border border-[rgb(var(--color-border-primary))] text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))] transition-all"
            title="Back to Agents"
          >
            <ArrowLeft class="w-3.5 h-3.5" />
          </a>
          
          <Breadcrumbs items={[
            { label: 'Home', href: '/' },
            { label: 'Agents', href: '/agents' },
            { label: agent.name }
          ]} />
          
          <Badge variant={statusVariant} size="sm">
            {agent.status}
          </Badge>
        </div>

        <!-- Right: Container Stats -->
        <div class="flex items-center gap-3">
          <div class="flex items-center gap-2 text-sm">
            <Box class="w-3.5 h-3.5 text-[rgb(var(--color-text-secondary))]" />
            <span class="text-[9px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wide">Containers</span>
            <span class="text-xs font-bold text-[rgb(var(--color-text-primary))] tabular-nums">
              <span class="text-green-600">{runningContainers}</span> / {containers.length}
            </span>
          </div>
        </div>
      </div>

      <!-- Bottom Row: Tabs -->
      <TabNav tabs={tabs} activeTab={activeTab} onTabChange={(tabId) => activeTab = tabId as typeof activeTab} />
    {/if}
  </div>

  <!-- Content -->
  <div class="flex-1 overflow-auto">
    {#if error}
      <div class="h-full flex items-center justify-center">
        <div class="text-center max-w-md px-4">
          <div class="inline-flex items-center justify-center w-16 h-16 rounded-full mb-4 bg-red-50">
            <AlertCircle class="w-8 h-8 text-red-600" />
          </div>
          <h3 class="text-lg font-semibold text-[rgb(var(--color-text-primary))] mb-2">Error</h3>
          <p class="text-sm text-[rgb(var(--color-text-secondary))] mb-4">{error.getUserMessage()}</p>
          <a use:link href="/agents">
            <Button variant="primary" size="md">Back to Agents</Button>
          </a>
        </div>
      </div>
    {:else if isLoading}
      <LoadingState message="Loading agent details..." />
    {:else if !agent}
      <EmptyState icon={Server} title="Agent not found" />
    {:else}
      <!-- Dashboard Tab -->
      {#if activeTab === 'dashboard'}
        <div class="p-6 space-y-6">
          <!-- Overview Cards -->
          <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
            <!-- Agent Info Card -->
            <StatCard title="Agent Information" icon={Server}>
              <dl class="space-y-3 text-left">
                <div>
                  <dt class="text-xs text-[rgb(var(--color-text-secondary))] mb-1">Status</dt>
                  <dd class="flex items-center gap-2">
                    <StatusDot 
                      status={agent.status === 'HEALTHY' ? 'healthy' : agent.status === 'UNHEALTHY' ? 'unhealthy' : agent.status === 'DEGRADED' ? 'degraded' : 'stopped'} 
                      animated={agent.status === 'HEALTHY'} 
                    />
                    <span class="text-sm font-medium text-[rgb(var(--color-text-primary))]">
                      {agent.status}
                    </span>
                  </dd>
                </div>
                <div>
                  <dt class="text-xs text-[rgb(var(--color-text-secondary))] mb-1">Last Seen</dt>
                  <dd class="text-sm text-[rgb(var(--color-text-primary))]">
                    {timeAgo(agent.lastSeen)}
                  </dd>
                </div>
                {#if agent.version}
                  <div>
                    <dt class="text-xs text-[rgb(var(--color-text-secondary))] mb-1">Version</dt>
                    <dd class="text-sm font-mono text-[rgb(var(--color-text-primary))]">
                      {agent.version}
                    </dd>
                  </div>
                {/if}
                <div>
                  <dt class="text-xs text-[rgb(var(--color-text-secondary))] mb-1">Address</dt>
                  <dd class="text-sm font-mono text-[rgb(var(--color-text-primary))] break-all">
                    {agent.address}
                  </dd>
                </div>
              </dl>
            </StatCard>

            <!-- Container Stats Card -->
            <StatCard title="Containers" icon={Box}>
              <div class="space-y-4">
                <div>
                  <div class="flex items-center justify-between mb-2">
                    <span class="text-xs text-[rgb(var(--color-text-secondary))]">Total</span>
                    <span class="text-2xl font-bold text-[rgb(var(--color-text-primary))]">
                      {containers.length}
                    </span>
                  </div>
                </div>
                <div class="grid grid-cols-2 gap-3">
                  <div class="bg-green-50 rounded-lg p-3 border border-green-200">
                    <div class="text-xs text-green-700 mb-1">Running</div>
                    <div class="text-xl font-bold text-green-800">{runningContainers}</div>
                  </div>
                  <div class="bg-[rgb(var(--color-bg-tertiary))] rounded-lg p-3 border border-[rgb(var(--color-border-primary))]">
                    <div class="text-xs text-[rgb(var(--color-text-secondary))] mb-1">Stopped</div>
                    <div class="text-xl font-bold text-[rgb(var(--color-text-primary))]">{stoppedContainers}</div>
                  </div>
                </div>
              </div>
            </StatCard>

            <!-- Health Status Card -->
            <StatCard title="System Health" icon={Activity}>
              <div class="py-2">
                <AgentHealthMonitor agentId={agent.id} />
              </div>
            </StatCard>
          </div>

          <!-- Real-time Health Monitoring -->
          <AgentHealthMonitor agentId={agent.id} />

          <!-- Containers List -->
          <SectionCard title="Managed Containers" badge={containers.length}>
            {#if containers.length === 0}
              <EmptyState icon={Box} title="No containers found" message="No containers found on this agent" />
            {:else}
              <div class="-m-6">
                <DataTable columns={[
                  { key: 'status', label: 'Status', width: 'w-16' },
                  { key: 'name', label: 'Name' },
                  { key: 'resources', label: 'Resources', width: 'w-40' },
                  { key: 'ports', label: 'Ports', width: 'w-32' },
                  { key: 'uptime', label: 'Uptime', width: 'w-28' },
                  { key: 'actions', label: 'Actions', width: 'w-24' }
                ]}>
                  {#each containers as container (container.id)}
                    <ContainerListRow {container} />
                  {/each}
                </DataTable>
              </div>
            {/if}
          </SectionCard>
        </div>
      {/if}

      <!-- Details Tab -->
      {#if activeTab === 'details'}
        <div class="p-6 space-y-6">
          <!-- Agent Configuration -->
          <SectionCard title="Agent Configuration">
            <InfoGrid 
              columns={2}
              items={[
                { label: 'Agent ID', value: agent.id, variant: 'code' },
                { label: 'Name', value: agent.name },
                { label: 'Address', value: agent.address, variant: 'code' },
                { label: 'Version', value: agent.version || 'N/A', variant: 'code' },
                { 
                  label: 'Status', 
                  value: agent.status,
                  badge: () => {
                    // Safety check for TypeScript
                    if (!agent) return '';
                    return `<div class="flex items-center gap-2">
                      <StatusDot status="${agent.status === 'HEALTHY' ? 'healthy' : agent.status === 'UNHEALTHY' ? 'unhealthy' : agent.status === 'DEGRADED' ? 'degraded' : 'stopped'}" animated={${agent.status === 'HEALTHY'}} />
                      <span class="text-sm font-medium text-[rgb(var(--color-text-primary))]">${agent.status}</span>
                    </div>`;
                  }
                },
                { label: 'Last Seen', value: timeAgo(agent.lastSeen) }
              ]}
            />
          </SectionCard>

          <!-- Labels -->
          {#if agent.labels && agent.labels.length > 0}
            <div class="bg-[rgb(var(--color-bg-secondary))] rounded-lg border border-[rgb(var(--color-border-primary))] p-6">
              <h2 class="text-lg font-semibold text-[rgb(var(--color-text-primary))] mb-4">Labels</h2>
              <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
                {#each agent.labels as label}
                  <div class="flex items-start gap-3 p-4 bg-[rgb(var(--color-bg-tertiary))] rounded-lg border border-[rgb(var(--color-border-primary))]">
                    <dt class="text-xs font-mono text-[rgb(var(--color-text-secondary))] min-w-30 font-semibold">{label.key}</dt>
                    <dd class="text-xs font-mono text-[rgb(var(--color-text-primary))] flex-1 break-all">{label.value}</dd>
                  </div>
                {/each}
              </div>
            </div>
          {/if}
        </div>
      {/if}

      <!-- Statistics Tab -->
      {#if activeTab === 'stats'}
        <div class="p-6">
          <div class="bg-[rgb(var(--color-bg-secondary))] rounded-lg border border-[rgb(var(--color-border-primary))] p-12 text-center">
            <BarChart3 class="w-16 h-16 mx-auto mb-4 text-[rgb(var(--color-text-tertiary))]" />
            <h3 class="text-lg font-semibold text-[rgb(var(--color-text-primary))] mb-2">Statistics Coming Soon</h3>
            <p class="text-sm text-[rgb(var(--color-text-secondary))]">
              Agent-level resource statistics will be available in a future update.
            </p>
          </div>
        </div>
      {/if}

      <!-- Realtime Tab -->
      {#if activeTab === 'realtime'}
        <div class="p-6 space-y-6">
          <SectionCard title="Health Telemetry" icon={Activity}>
            <AgentHealthMonitor agentId={agent.id} />
          </SectionCard>
          
          {#if agentContainer}
            <SectionCard title="System Resources (Agent Container)" icon={Activity}>
              <RealTimeStats containerId={agentContainer.id} agentId={agent.id} />
            </SectionCard>
          {:else}
            <div class="bg-[rgb(var(--color-bg-secondary))] border border-[rgb(var(--color-border-primary))] rounded-lg p-12 text-center">
              <Box class="w-16 h-16 mx-auto mb-4 text-[rgb(var(--color-text-tertiary))]" />
              <h3 class="text-lg font-semibold text-[rgb(var(--color-text-primary))] mb-2">Agent Container Not Found</h3>
              <p class="text-sm text-[rgb(var(--color-text-secondary))] max-w-sm mx-auto">
                Real-time resource metrics are only available when the agent is identified as a container. 
                Ensure the agent container is named with 'docktail-agent' or 'agent'.
              </p>
            </div>
          {/if}
        </div>
      {/if}

      <!-- Logs Tab -->
      {#if activeTab === 'logs'}
        <div class="flex flex-col h-full">
          {#if !agentContainer}
            <div class="flex-1 flex items-center justify-center p-6">
              <div class="text-center max-w-md">
                <Terminal class="w-16 h-16 mx-auto mb-4 text-[rgb(var(--color-text-tertiary))]" />
                <h3 class="text-lg font-semibold text-[rgb(var(--color-text-primary))] mb-2">No Agent Container Found</h3>
                <p class="text-sm text-[rgb(var(--color-text-secondary))]">
                  Unable to locate the agent container to stream logs from.
                </p>
              </div>
            </div>
          {:else}
            <!-- Logs Controls -->
            <div class="px-6 py-3 border-b border-[rgb(var(--color-border-primary))] bg-[rgb(var(--color-bg-secondary))] flex items-center justify-between">
              <div class="flex items-center gap-2">
                <span class="text-xs text-[rgb(var(--color-text-secondary))]">
                  {logs.length} lines
                </span>
                {#if isStreaming}
                  <span class="flex items-center gap-1 text-xs text-green-600">
                    <span class="w-2 h-2 bg-green-600 rounded-full animate-pulse"></span>
                    Live
                  </span>
                {/if}
              </div>
              <div class="flex items-center gap-2">
                <Button
                  variant="ghost"
                  size="sm"
                  onclick={togglePause}
                  disabled={!isStreaming}
                >
                  {isPaused ? 'Resume' : 'Pause'}
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  onclick={clearLogs}
                >
                  Clear
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  onclick={isStreaming ? stopStreaming : startStreaming}
                >
                  {isStreaming ? 'Stop' : 'Start'}
                </Button>
              </div>
            </div>

            <!-- Logs Display -->
            <div
              bind:this={logContainer}
              class="flex-1 overflow-auto bg-[rgb(var(--color-bg-primary))] p-4 font-mono text-xs"
            >
              {#if logsError}
                <div class="text-red-600 mb-4">
                  Error: {logsError.getUserMessage()}
                </div>
              {/if}
              {#if logs.length === 0}
                <div class="text-[rgb(var(--color-text-tertiary))] text-center py-8">
                  No logs yet. {isStreaming ? 'Waiting for logs...' : 'Click Start to begin streaming.'}
                </div>
              {:else}
                {#each logs as log}
                  <div class="mb-1 hover:bg-[rgb(var(--color-bg-tertiary))] px-2 py-1 rounded">
                    <span class="text-[rgb(var(--color-text-tertiary))] mr-2">
                      {new Date(log.timestamp).toLocaleTimeString()}
                    </span>
                    <span class="{log.level === 'STDERR' ? 'text-red-500' : 'text-[rgb(var(--color-text-primary))]'}">
                      {log.content}
                    </span>
                  </div>
                {/each}
              {/if}
            </div>
          {/if}
        </div>
      {/if}
    {/if}
  </div>
</div>
