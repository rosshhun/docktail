<script lang="ts">
  import { link, push } from 'svelte-spa-router';
  import { ArrowLeft, Activity, Terminal, Database, Network, HardDrive, Cpu, MemoryStick, Layers, Play, Pause, Download, Copy, RefreshCw, Filter, Clock, Zap, Shield, Heart, RotateCw, Server } from '@lucide/svelte';
  import Button from '../lib/common/Button.svelte';
  import Badge from '../lib/common/Badge.svelte';
  import StatusDot from '../lib/common/StatusDot.svelte';
  import Breadcrumbs from '../lib/common/Breadcrumbs.svelte';
  import RealTimeStats from '../lib/components/RealTimeStats.svelte';
  import { 
    fetchContainer, 
    fetchAgent,
    fetchContainerDetails, 
    subscribeToLogs, 
    fetchHistoricalLogs,
    type Container, 
    type ContainerDetails, 
    type LogEvent,
    GraphQLError 
  } from '../lib/api';
  import { formatBytes, formatCpuLimit, formatNsDuration, getResourceColor } from '../lib/utils/formatting';
  import { logger } from '../lib/utils/logger';

  // Get container ID from route params
  let { params = { id: '' } }: { params?: { id: string } } = $props();

  let container = $state<Container | null>(null);
  let details = $state<ContainerDetails | null>(null);
  let agentName = $state<string>('');
  let isLoading = $state(true);
  let error = $state<GraphQLError | null>(null);
  let activeTab = $state<'details' | 'realtime' | 'logs'>('details');

  // Logs state
  let logs = $state<LogEvent[]>([]);
  let isStreaming = $state(false);
  let isPaused = $state(false);
  let logContainer = $state<HTMLDivElement>();
  let unsubscribe: (() => void) | null = null;
  let viewMode = $state<'streaming' | 'historical'>('streaming');
  let filterPattern = $state('');
  let filterMode = $state<'NONE' | 'INCLUDE' | 'EXCLUDE'>('NONE');
  let tailLines = $state(100);
  let sinceDate = $state('');
  let sinceTime = $state('');
  let untilDate = $state('');
  let untilTime = $state('');
  let showFilters = $state(false);
  let isRetryingLogs = $state(false);
  let logsError = $state<GraphQLError | null>(null);

  // Column widths for resizable columns (set to auto initially, will be overridden by manual resize)
  let columnWidths = $state({
    timestamp: 'auto' as string | number,
    container: 'auto' as string | number,
    level: 'auto' as string | number,
    logger: 'auto' as string | number,
    status: 'auto' as string | number,
  });

  let isResizing = $state(false);
  let resizingColumn = $state<string | null>(null);
  let startX = $state(0);
  let startWidth = $state(0);

  function startResize(column: string, event: MouseEvent) {
    isResizing = true;
    resizingColumn = column;
    startX = event.clientX;
    
    const currentWidth = columnWidths[column as keyof typeof columnWidths];
    // If auto, get the actual width from the element
    if (currentWidth === 'auto') {
      const element = event.currentTarget as HTMLElement;
      startWidth = element.parentElement?.offsetWidth || 150;
    } else {
      startWidth = currentWidth as number;
    }
    event.preventDefault();
  }

  function handleMouseMove(event: MouseEvent) {
    if (!isResizing || !resizingColumn) return;
    
    const diff = event.clientX - startX;
    const newWidth = Math.max(80, startWidth + diff); // Minimum width of 80px
    columnWidths[resizingColumn as keyof typeof columnWidths] = newWidth;
  }

  function stopResize() {
    isResizing = false;
    resizingColumn = null;
  }

  // Use $effect to load data when component mounts or params change
  $effect(() => {
    if (!params.id) {
      error = new GraphQLError('No container ID provided', 'BAD_REQUEST');
      return;
    }

    loadContainerData();
    
    // Cleanup: Clear container name from localStorage when component unmounts
    return () => {
      if (typeof window !== 'undefined') {
        localStorage.removeItem('currentContainerName');
      }
    };
  });

  async function loadContainerData() {
    try {
      isLoading = true;
      error = null;
      
      // Fetch container basic info
      container = await fetchContainer(params.id);
      
      // Store container name in localStorage for sidebar navigation
      if (container && typeof window !== 'undefined') {
        localStorage.setItem('currentContainerName', container.name);
      }
      
      // Fetch detailed info and agent in parallel
      if (container) {
        const [detailsData, agent] = await Promise.all([
          fetchContainerDetails(params.id, container.agentId),
          fetchAgent(container.agentId).catch(() => null), // Agent info
        ]);
        
        details = detailsData;
        agentName = agent?.name || container.agentId;
      }
    } catch (err: any) {
      if (err instanceof GraphQLError) {
        error = err;
      } else {
        error = new GraphQLError(err.message || 'Failed to load container', 'INTERNAL_SERVER_ERROR');
      }
    } finally {
      isLoading = false;
    }
  }

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

  // Load logs when logs tab is activated
  let logsInitialized = $state(false);
  $effect(() => {
    if (activeTab === 'logs' && container && !logsInitialized) {
      logsInitialized = true;
      loadLogs();
    }
    
    // Cleanup logs subscription when switching away from logs tab
    if (activeTab !== 'logs' && logsInitialized) {
      unsubscribe?.();
      logs = [];
      logsInitialized = false;
      isStreaming = false;
    }
  });

  async function loadLogs() {
    if (!container) return;
    
    try {
      logsError = null;
      isRetryingLogs = true;

      if (viewMode === 'streaming') {
        console.log('[Logs] Starting log stream for container:', container.id, 'agent:', container.agentId);
        // Start streaming logs
        unsubscribe = subscribeToLogs(
          container.id,
          container.agentId,
          (logEvent) => {
            console.log('[Logs] Received log event:', logEvent);
            logs = [...logs, logEvent];
          },
          (err) => {
            console.error('[Logs] Stream error:', err);
            if (err instanceof GraphQLError) {
              logsError = err;
            } else {
              logsError = new GraphQLError(err.message || 'Stream error', 'INTERNAL_SERVER_ERROR');
            }
            isStreaming = false;
          }
        );
        isStreaming = true;
        console.log('[Logs] Subscription started, isStreaming:', isStreaming);
      } else {
        // Load historical logs
        await loadHistoricalLogs();
      }
    } catch (err: any) {
      console.error('[Logs] Load error:', err);
      if (err instanceof GraphQLError) {
        logsError = err;
      } else {
        logsError = new GraphQLError(err.message || 'Failed to load logs', 'INTERNAL_SERVER_ERROR');
      }
    } finally {
      isRetryingLogs = false;
    }
  }

  async function loadHistoricalLogs() {
    if (!container) return;

    try {
      logsError = null;
      isRetryingLogs = true;

      // Build date-time strings for since/until
      const sinceTimestamp = sinceDate && sinceTime ? `${sinceDate}T${sinceTime}:00Z` : undefined;
      const untilTimestamp = untilDate && untilTime ? `${untilDate}T${untilTime}:00Z` : undefined;

      const historicalLogs = await fetchHistoricalLogs(
        container.id,
        container.agentId,
        {
          tail: tailLines,
          since: sinceTimestamp,
          until: untilTimestamp,
          filter: filterPattern || undefined,
          filterMode: filterMode !== 'NONE' ? filterMode : undefined,
        }
      );

      logs = historicalLogs;
      isStreaming = false;
    } catch (err: any) {
      if (err instanceof GraphQLError) {
        logsError = err;
      } else {
        logsError = new GraphQLError(err.message || 'Failed to load historical logs', 'INTERNAL_SERVER_ERROR');
      }
    } finally {
      isRetryingLogs = false;
    }
  }

  function switchMode(mode: 'streaming' | 'historical') {
    unsubscribe?.();
    logs = [];
    viewMode = mode;
    logsError = null;
    
    if (mode === 'streaming') {
      loadLogs();
    } else {
      isStreaming = false;
      loadHistoricalLogs();
    }
  }

  function togglePause() {
    isPaused = !isPaused;
  }

  function handleDownload() {
    const content = logs
      .map((log) => `${log.timestamp} [${log.level}] ${log.content}`)
      .join('\n');

    const blob = new Blob([content], { type: 'text/plain' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `${container?.name || 'logs'}-${Date.now()}.txt`;
    a.click();
    URL.revokeObjectURL(url);
  }

  function handleCopy() {
    const content = logs
      .map((log) => `${log.timestamp} [${log.level}] ${log.content}`)
      .join('\n');
    navigator.clipboard.writeText(content);
  }

  function getLogColor(log: LogEvent): string {
    if (log.level === 'STDERR') return 'text-red-400';
    // Use parsed level if available
    if (log.parsed?.level) {
      const level = log.parsed.level.toLowerCase();
      if (level === 'error' || level === 'fatal') return 'text-red-400';
      if (level === 'warn' || level === 'warning') return 'text-yellow-400';
      if (level === 'info') return 'text-blue-400';
      if (level === 'debug' || level === 'trace') return 'text-[rgb(var(--color-text-tertiary))]';
    }
    return 'text-[rgb(var(--color-text-primary))]';
  }

  function getBadgeColor(level?: string): string {
    if (!level) return 'bg-gray-700 text-[rgb(var(--color-text-primary))]';
    const l = level.toLowerCase();
    if (l === 'error' || l === 'fatal') return 'bg-red-900/50 text-red-300';
    if (l === 'warn' || l === 'warning') return 'bg-yellow-900/50 text-yellow-300';
    if (l === 'info') return 'bg-blue-900/50 text-blue-300';
    if (l === 'debug' || l === 'trace') return 'bg-gray-800 text-[rgb(var(--color-text-secondary))]';
    return 'bg-gray-700 text-[rgb(var(--color-text-primary))]';
  }

  function formatDuration(ms?: number): string {
    if (!ms) return '';
    if (ms < 1000) return `${ms}ms`;
    return `${(ms / 1000).toFixed(2)}s`;
  }

  function getStatusColor(status?: number): string {
    if (!status) return 'text-[rgb(var(--color-text-secondary))]';
    if (status >= 200 && status < 300) return 'text-green-400';
    if (status >= 300 && status < 400) return 'text-blue-400';
    if (status >= 400 && status < 500) return 'text-yellow-400';
    if (status >= 500) return 'text-red-400';
    return 'text-[rgb(var(--color-text-secondary))]';
  }

  const statusVariant = $derived(
    container?.state === 'RUNNING' ? 'success' : 
    container?.state === 'EXITED' || container?.state === 'DEAD' ? 'default' : 'warning'
  );

  // Check if this is a cluster or agent container for proper breadcrumbs
  const isClusterContainer = $derived(container?.name.startsWith('docktail-cluster') || false);
  const isAgentContainer = $derived(container?.name.startsWith('docktail-agent') || false);
  
  // Determine breadcrumb parent based on container type
  const breadcrumbParent = $derived(() => {
    if (isClusterContainer) return { label: 'Cluster', href: '/cluster' };
    if (isAgentContainer) return { label: 'Agents', href: '/agents' };
    return { label: 'Containers', href: '/containers' };
  });
</script>

<svelte:window 
  onmousemove={handleMouseMove} 
  onmouseup={stopResize}
/>

<div class="h-full flex flex-col bg-[rgb(var(--color-bg-primary))]" class:select-none={isResizing} class:cursor-col-resize={isResizing}>
  <!-- Header - Compact Default -->
  <div class="border-b border-[rgb(var(--color-border-primary))] px-8 py-2 sticky top-0 bg-[rgb(var(--color-bg-primary))] z-10 shadow-sm">
    {#if container}
      <!-- Top Row: Breadcrumb & Status & Stats -->
      <div class="flex items-center justify-between mb-4 mt-3">
        <!-- Left: Breadcrumb & Status -->
        <div class="flex items-center gap-3 flex-1">
          <a
            use:link
            href={breadcrumbParent().href}
            class="flex items-center justify-center w-7 h-7 rounded hover:bg-[rgb(var(--color-bg-secondary))] border border-[rgb(var(--color-border-primary))] text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))] transition-all"
            title="Back to {breadcrumbParent().label}"
          >
            <ArrowLeft class="w-3.5 h-3.5" />
          </a>
          
          {#if container}
            {@const parent = breadcrumbParent()}
            <Breadcrumbs items={[
              { label: 'Home', href: '/' },
              parent,
              { label: container.name }
            ]} />
          {/if}
          
          <Badge variant={statusVariant} size="sm">
            {container.state}
          </Badge>
        </div>

      </div>

      <!-- Bottom Row: Tabs -->
      <div class="flex items-center justify-between gap-1">
        <div class="flex items-center gap-1">
          <button
            onclick={() => activeTab = 'details'}
            class="px-3 py-1.5 cursor-pointer text-sm font-medium transition-colors {activeTab === 'details' ? 'text-[rgb(var(--color-accent-blue))] border-b-2 border-[rgb(var(--color-accent-blue))]' : 'text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))]'}"
          >
            <Database class="w-3.5 h-3.5 inline-block mr-1.5" />
            Details
          </button>
          <button
            onclick={() => activeTab = 'realtime'}
            class="px-3 py-1.5 text-sm cursor-pointer font-medium transition-colors {activeTab === 'realtime' ? 'text-[rgb(var(--color-accent-blue))] border-b-2 border-[rgb(var(--color-accent-blue))]' : 'text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))]'}"
          >
            <Activity class="w-3.5 h-3.5 inline-block mr-1.5" />
            Statistics
          </button>
          <button
            onclick={() => activeTab = 'logs'}
            class="px-3 py-1.5 text-sm cursor-pointer font-medium transition-colors {activeTab === 'logs' ? 'text-[rgb(var(--color-accent-blue))] border-b-2 border-[rgb(var(--color-accent-blue))]' : 'text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))]'}"
          >
            <Terminal class="w-3.5 h-3.5 inline-block mr-1.5" />
            Logs
          </button>
        </div>

        <!-- Logs Controls (shown when logs tab is active) -->
        {#if activeTab === 'logs'}
          <div class="flex items-center gap-2">
            <!-- Action Buttons -->
            <div class="flex items-center gap-1">
              <button
                onclick={() => showFilters = !showFilters}
                class="flex items-center gap-1.5 px-2.5 py-1 text-xs font-medium rounded border transition-all {showFilters ? 'bg-[rgb(var(--color-accent-blue))] text-white border-[rgb(var(--color-accent-blue))]' : 'bg-[rgb(var(--color-bg-secondary))] text-[rgb(var(--color-text-secondary))] border-[rgb(var(--color-border-primary))] hover:border-[rgb(var(--color-accent-blue))]'}"
                title="Filters"
              >
                <Filter class="w-3 h-3" />
              </button>

              {#if viewMode === 'streaming'}
                <button
                  onclick={togglePause}
                  class="flex items-center gap-1.5 px-2.5 py-1 text-xs font-medium rounded border transition-all bg-[rgb(var(--color-bg-secondary))] text-[rgb(var(--color-text-secondary))] border-[rgb(var(--color-border-primary))] hover:border-[rgb(var(--color-accent-blue))] hover:text-[rgb(var(--color-text-primary))]"
                  title={isPaused ? 'Resume' : 'Pause'}
                >
                  {#if isPaused}
                    <Play class="w-3 h-3" />
                  {:else}
                    <Pause class="w-3 h-3" />
                  {/if}
                </button>
              {/if}
              
              <button
                onclick={handleCopy}
                class="flex items-center gap-1.5 px-2.5 py-1 text-xs font-medium rounded border transition-all bg-[rgb(var(--color-bg-secondary))] text-[rgb(var(--color-text-secondary))] border-[rgb(var(--color-border-primary))] hover:border-[rgb(var(--color-accent-blue))] hover:text-[rgb(var(--color-text-primary))]"
                title="Copy"
              >
                <Copy class="w-3 h-3" />
              </button>
              
              <button
                onclick={handleDownload}
                class="flex items-center gap-1.5 px-2.5 py-1 text-xs font-medium rounded border transition-all bg-[rgb(var(--color-bg-secondary))] text-[rgb(var(--color-text-secondary))] border-[rgb(var(--color-border-primary))] hover:border-[rgb(var(--color-accent-blue))] hover:text-[rgb(var(--color-text-primary))]"
                title="Download"
              >
                <Download class="w-3 h-3" />
              </button>
            </div>
          </div>
        {/if}
      </div>
    {/if}
  </div>

  <!-- Content -->
  <div class="flex-1 overflow-auto">
    {#if error}
      <div class="h-full flex items-center justify-center">
        <div class="text-center max-w-md px-4">
          <div class="inline-flex items-center justify-center w-16 h-16 rounded-full mb-4 bg-red-50">
            <svg class="w-8 h-8 text-red-600" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
            </svg>
          </div>
          <h3 class="text-lg font-semibold text-[rgb(var(--color-text-primary))] mb-2">Error</h3>
          <p class="text-sm text-[rgb(var(--color-text-secondary))] mb-4">{error.getUserMessage()}</p>
          <a use:link href="/containers">
            <Button variant="primary" size="md">Back to Containers</Button>
          </a>
        </div>
      </div>
    {:else if isLoading}
      <div class="h-full flex items-center justify-center">
        <div class="text-center">
          <div class="inline-block animate-spin rounded-full h-8 w-8 border-b-2 border-[rgb(var(--color-accent-blue))]"></div>
          <p class="text-sm text-[rgb(var(--color-text-secondary))] mt-3">Loading container details...</p>
        </div>
      </div>
    {:else if !container}
      <div class="h-full flex items-center justify-center">
        <div class="text-center">
          <p class="text-sm text-[rgb(var(--color-text-secondary))]">Container not found</p>
        </div>
      </div>
    {:else}
      <div class="px-8 py-5 space-y-4">
        {#if activeTab === 'details'}
          <!-- Container Overview Card -->
          <div class="bg-[rgb(var(--color-bg-secondary))] rounded-lg border border-[rgb(var(--color-border-primary))] overflow-hidden">
            <div class="px-5 py-3 border-b border-[rgb(var(--color-border-primary))] bg-[rgb(var(--color-bg-tertiary))]/30">
              <h2 class="text-sm font-semibold text-[rgb(var(--color-text-primary))] flex items-center gap-2">
                <Database class="w-4 h-4" />
                Container Overview
              </h2>
            </div>
            <div class="p-6">
              <!-- Primary Info Row -->
              <div class="flex items-start gap-6 mb-6 pb-6 border-b border-[rgb(var(--color-border-primary))]">
                <div class="flex-1">
                  <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2">Container Name</dt>
                  <dd class="text-base font-bold text-[rgb(var(--color-text-primary))]">{container.name}</dd>
                </div>
                <div class="flex items-center gap-3">
                  <div>
                    <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2 text-right">State</dt>
                    <dd class="flex items-center justify-end gap-2">
                      <StatusDot status={container.state.toLowerCase() as any} animated={container.state === 'RUNNING'} size="md" />
                      <Badge variant={statusVariant} size="sm">{container.state}</Badge>
                    </dd>
                  </div>
                </div>
              </div>

              <!-- Details Grid -->
              <dl class="space-y-4">
                <div class="grid grid-cols-3 gap-6">
                  <div class="col-span-2 bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                    <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2">Container ID</dt>
                    <dd class="text-xs font-mono text-[rgb(var(--color-text-primary))] break-all">{container.id}</dd>
                  </div>
                  <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                    <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2">Agent</dt>
                    <dd class="text-xs font-semibold text-[rgb(var(--color-text-primary))]">{agentName}</dd>
                  </div>
                </div>

                <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                  <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2">Image</dt>
                  <dd class="text-xs font-mono text-[rgb(var(--color-text-primary))] break-all">{container.image}</dd>
                </div>

                <div class="grid grid-cols-2 gap-6">
                  <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                    <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2">Status</dt>
                    <dd class="text-xs text-[rgb(var(--color-text-primary))]">{container.status}</dd>
                  </div>
                  <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                    <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2">Created</dt>
                    <dd class="text-xs text-[rgb(var(--color-text-primary))]">{new Date(container.createdAt).toLocaleString()}</dd>
                  </div>
                </div>

                {#if container.state === 'EXITED' || container.state === 'DEAD'}
                  {#if container.stateInfo}
                    <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                      <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2">Exit Code</dt>
                      <dd class="text-xs font-mono font-bold" class:text-red-600={container.stateInfo.exitCode !== 0} class:text-green-600={container.stateInfo.exitCode === 0}>
                        {container.stateInfo.exitCode}
                        {#if container.stateInfo.exitCode === 0}
                          <span class="ml-1 text-[10px]">✓ Clean exit</span>
                        {:else}
                          <span class="ml-1 text-[10px]">✗ Error exit</span>
                        {/if}
                      </dd>
                    </div>
                  {:else}
                    {@const exitMatch = container.status.match(/Exited \((\d+)\)/)}
                    {#if exitMatch}
                      <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                        <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2">Exit Code</dt>
                        <dd class="text-xs font-mono font-bold" class:text-red-600={parseInt(exitMatch[1]) !== 0} class:text-green-600={parseInt(exitMatch[1]) === 0}>
                          {exitMatch[1]}
                          {#if parseInt(exitMatch[1]) === 0}
                            <span class="ml-1 text-[10px]">✓ Clean exit</span>
                          {:else}
                            <span class="ml-1 text-[10px]">✗ Error exit</span>
                          {/if}
                        </dd>
                      </div>
                    {/if}
                  {/if}
                {/if}

                <!-- State Details (from inspect) -->
                {#if container.stateInfo}
                  <div class="grid grid-cols-2 gap-6">
                    {#if container.stateInfo.startedAt}
                      <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                        <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2">Started At</dt>
                        <dd class="text-xs text-[rgb(var(--color-text-primary))]">{new Date(container.stateInfo.startedAt).toLocaleString()}</dd>
                      </div>
                    {/if}
                    {#if container.stateInfo.finishedAt && (container.state === 'EXITED' || container.state === 'DEAD')}
                      <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                        <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2">Finished At</dt>
                        <dd class="text-xs text-[rgb(var(--color-text-primary))]">{new Date(container.stateInfo.finishedAt).toLocaleString()}</dd>
                      </div>
                    {/if}
                  </div>

                  <div class="grid grid-cols-3 gap-6">
                    {#if container.stateInfo.pid > 0}
                      <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                        <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2">Host PID</dt>
                        <dd class="text-xs font-mono text-[rgb(var(--color-text-primary))]">{container.stateInfo.pid}</dd>
                      </div>
                    {/if}
                    {#if container.stateInfo.restartCount > 0}
                      <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                        <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2 flex items-center gap-1">
                          <RotateCw class="w-3 h-3" />
                          Restart Count
                        </dt>
                        <dd class="text-xs font-mono font-bold text-yellow-500">{container.stateInfo.restartCount}</dd>
                      </div>
                    {/if}
                    {#if container.stateInfo.oomKilled}
                      <div class="bg-red-900/20 rounded-md p-3 border border-red-500/30">
                        <dt class="text-[10px] font-bold text-red-400 uppercase tracking-wider mb-2">⚠ OOM Killed</dt>
                        <dd class="text-xs font-bold text-red-400">Container was killed due to memory limit</dd>
                      </div>
                    {/if}
                  </div>
                {/if}

                {#if container.logDriver}
                  <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                    <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2">Log Driver</dt>
                    <dd class="text-xs font-mono text-[rgb(var(--color-text-primary))]">{container.logDriver}</dd>
                  </div>
                {/if}
              </dl>
            </div>
          </div>

          <!-- Docker Compose Information -->
          {#if container.labels && container.labels.some(l => l.key.startsWith('com.docker.compose.'))}
            <div class="bg-[rgb(var(--color-bg-secondary))] rounded-lg border border-[rgb(var(--color-border-primary))] overflow-hidden">
              <div class="px-5 py-3 border-b border-[rgb(var(--color-border-primary))] bg-[rgb(var(--color-bg-tertiary))]/30">
                <h2 class="text-sm font-semibold text-[rgb(var(--color-text-primary))] flex items-center gap-2">
                  <Layers class="w-4 h-4" />
                  Docker Compose
                </h2>
              </div>
              <div class="p-6">
                <dl class="grid grid-cols-2 gap-4">
                  {#each container.labels.filter(l => l.key.startsWith('com.docker.compose.')) as label}
                    <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                      <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2">
                        {label.key.replace('com.docker.compose.', '').replace(/\./g, ' ').replace(/\b\w/g, l => l.toUpperCase())}
                      </dt>
                      <dd class="text-xs font-mono text-[rgb(var(--color-text-primary))]">{label.value}</dd>
                    </div>
                  {/each}
                </dl>
              </div>
            </div>
          {/if}

          <!-- Labels -->
          {#if container.labels && container.labels.length > 0}
            <div class="bg-[rgb(var(--color-bg-secondary))] rounded-lg border border-[rgb(var(--color-border-primary))] overflow-hidden">
              <div class="px-5 py-3 border-b border-[rgb(var(--color-border-primary))] bg-[rgb(var(--color-bg-tertiary))]/30">
                <h2 class="text-sm font-semibold text-[rgb(var(--color-text-primary))] flex items-center gap-2">
                  Labels
                  <Badge variant="default" size="xs">
                    {container.labels.length}
                  </Badge>
                </h2>
              </div>
              <div class="overflow-x-auto">
                <table class="w-full">
                  <thead class="bg-[rgb(var(--color-bg-tertiary))]/50">
                    <tr class="border-b-2 border-[rgb(var(--color-border-primary))]">
                      <th class="px-6 py-3 text-left text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider">Key</th>
                      <th class="px-6 py-3 text-left text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider">Value</th>
                    </tr>
                  </thead>
                  <tbody class="divide-y divide-[rgb(var(--color-border-primary))]/50">
                    {#each container.labels as label}
                      <tr class="hover:bg-[rgb(var(--color-bg-tertiary))]/30 transition-colors">
                        <td class="px-6 py-3.5 text-xs text-[rgb(var(--color-text-secondary))] font-mono align-top" style="max-width: 400px;">
                          <div class="break-all leading-relaxed">{label.key}</div>
                        </td>
                        <td class="px-6 py-3.5 text-xs text-[rgb(var(--color-text-primary))] font-mono align-top">
                          <div class="break-all leading-relaxed">{label.value || '-'}</div>
                        </td>
                      </tr>
                    {/each}
                  </tbody>
                </table>
              </div>
            </div>
          {/if}

          {#if details}
            <!-- Command & Environment -->
            <div class="bg-[rgb(var(--color-bg-secondary))] rounded-lg border border-[rgb(var(--color-border-primary))] overflow-hidden">
              <div class="px-5 py-3 border-b border-[rgb(var(--color-border-primary))] bg-[rgb(var(--color-bg-tertiary))]/30">
                <h2 class="text-sm font-semibold text-[rgb(var(--color-text-primary))] flex items-center gap-2">
                  <Terminal class="w-4 h-4" />
                  Command & Environment
                </h2>
              </div>
              <div class="p-6 space-y-5">
                <!-- Runtime Info Row -->
                {#if details.hostname || details.user || details.restartPolicy || details.platform || details.runtime}
                  <div class="grid grid-cols-2 md:grid-cols-3 gap-4 pb-5 border-b border-[rgb(var(--color-border-primary))]">
                    {#if details.hostname}
                      <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                        <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2 flex items-center gap-1">
                          <Server class="w-3 h-3" />
                          Hostname
                        </dt>
                        <dd class="text-xs font-mono text-[rgb(var(--color-text-primary))]">{details.hostname}</dd>
                      </div>
                    {/if}
                    {#if details.user}
                      <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                        <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2 flex items-center gap-1">
                          <Shield class="w-3 h-3" />
                          User
                        </dt>
                        <dd class="text-xs font-mono text-[rgb(var(--color-text-primary))]">{details.user}</dd>
                      </div>
                    {/if}
                    {#if details.restartPolicy}
                      <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                        <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2 flex items-center gap-1">
                          <RotateCw class="w-3 h-3" />
                          Restart Policy
                        </dt>
                        <dd class="text-xs font-mono text-[rgb(var(--color-text-primary))]">
                          {details.restartPolicy.name}
                          {#if details.restartPolicy.name === 'on-failure' && details.restartPolicy.maxRetryCount > 0}
                            <span class="text-[rgb(var(--color-text-secondary))]"> (max {details.restartPolicy.maxRetryCount} retries)</span>
                          {/if}
                        </dd>
                      </div>
                    {/if}
                    {#if details.platform}
                      <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                        <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2">Platform</dt>
                        <dd class="text-xs font-mono text-[rgb(var(--color-text-primary))]">{details.platform}</dd>
                      </div>
                    {/if}
                    {#if details.runtime}
                      <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                        <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2">Runtime</dt>
                        <dd class="text-xs font-mono text-[rgb(var(--color-text-primary))]">{details.runtime}</dd>
                      </div>
                    {/if}
                  </div>
                {/if}

                {#if details.entrypoint && details.entrypoint.length > 0}
                  <div>
                    <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-3 flex items-center gap-2">
                      <Terminal class="w-3 h-3" />
                      Entrypoint
                    </dt>
                    <dd class="bg-[#0D0E12] rounded-md px-4 py-3 font-mono text-xs text-[rgb(var(--color-text-primary))] border border-[rgb(var(--color-border-primary))] leading-relaxed">
                      {details.entrypoint.join(' ')}
                    </dd>
                  </div>
                {/if}

                {#if details.command && details.command.length > 0}
                  <div>
                    <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-3 flex items-center gap-2">
                      <Terminal class="w-3 h-3" />
                      Command
                    </dt>
                    <dd class="bg-[#0D0E12] rounded-md px-4 py-3 font-mono text-xs text-[rgb(var(--color-text-primary))] border border-[rgb(var(--color-border-primary))] leading-relaxed">
                      {details.command.join(' ')}
                    </dd>
                  </div>
                {/if}

                {#if details.workingDir}
                  <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                    <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2">Working Directory</dt>
                    <dd class="font-mono text-xs text-[rgb(var(--color-text-primary))]">{details.workingDir}</dd>
                  </div>
                {/if}

                {#if details.env && details.env.length > 0}
                  <div>
                    <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-3 flex items-center gap-2">
                      Environment Variables
                      <Badge variant="default" size="xs">{details.env.length}</Badge>
                    </dt>
                    <dd class="bg-[#0D0E12] rounded-md px-4 py-3 font-mono text-[11px] text-[rgb(var(--color-text-primary))] max-h-60 overflow-y-auto border border-[rgb(var(--color-border-primary))]">
                      {#each details.env as envVar}
                        <div class="py-1 leading-relaxed border-b border-[rgb(var(--color-border-primary))]/20 last:border-0">{envVar}</div>
                      {/each}
                    </dd>
                  </div>
                {/if}
              </div>
            </div>

            <!-- Network Configuration -->
            {#if details.networks && details.networks.length > 0}
              <div class="bg-[rgb(var(--color-bg-secondary))] rounded-lg border border-[rgb(var(--color-border-primary))] overflow-hidden">
                <div class="px-5 py-3 border-b border-[rgb(var(--color-border-primary))] bg-[rgb(var(--color-bg-tertiary))]/30">
                  <h2 class="text-sm font-semibold text-[rgb(var(--color-text-primary))] flex items-center gap-2">
                    <Network class="w-4 h-4" />
                    Network Configuration
                  </h2>
                </div>
                <div class="p-6 space-y-4">
                  {#if details.networkMode}
                    <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50 mb-4">
                      <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2">Network Mode</dt>
                      <dd class="text-xs font-mono font-semibold text-[rgb(var(--color-text-primary))]">{details.networkMode}</dd>
                    </div>
                  {/if}
                  {#each details.networks as network}
                    <div class="border border-[rgb(var(--color-border-primary))] rounded-md bg-[rgb(var(--color-bg-tertiary))]/30 p-4">
                      <h3 class="text-xs font-bold text-[rgb(var(--color-text-primary))] mb-4 flex items-center gap-2">
                        <Network class="w-3.5 h-3.5 text-[rgb(var(--color-text-secondary))]" />
                        {network.networkName}
                      </h3>
                      <dl class="grid grid-cols-2 lg:grid-cols-3 gap-4">
                        <div class="bg-[rgb(var(--color-bg-primary))] rounded p-3 border border-[rgb(var(--color-border-primary))]/30">
                          <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2">IP Address</dt>
                          <dd class="text-xs font-mono font-semibold text-[rgb(var(--color-text-primary))]">{network.ipAddress}</dd>
                        </div>
                        <div class="bg-[rgb(var(--color-bg-primary))] rounded p-3 border border-[rgb(var(--color-border-primary))]/30">
                          <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2">Gateway</dt>
                          <dd class="text-xs font-mono font-semibold text-[rgb(var(--color-text-primary))]">{network.gateway}</dd>
                        </div>
                        {#if network.macAddress}
                          <div class="bg-[rgb(var(--color-bg-primary))] rounded p-3 border border-[rgb(var(--color-border-primary))]/30">
                            <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2">MAC Address</dt>
                            <dd class="text-xs font-mono font-semibold text-[rgb(var(--color-text-primary))]">{network.macAddress}</dd>
                          </div>
                        {/if}
                      </dl>
                    </div>
                  {/each}

                  {#if container.ports && container.ports.length > 0}
                    <div class="pt-3 border-t border-[rgb(var(--color-border-primary))]">
                      <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-3">Port Mappings</dt>
                      <dd class="flex flex-wrap gap-2">
                        {#each container.ports as port}
                          {@const portLabel = port.hostPort && port.hostIp 
                            ? `${port.hostIp}:${port.hostPort} → ${port.containerPort}/${port.protocol}`
                            : `${port.containerPort}/${port.protocol}`}
                          {#if port.hostPort}
                            <span class="px-3 py-1.5 rounded text-[11px] font-mono font-semibold border bg-blue-50 text-blue-700 border-blue-200">
                              {portLabel}
                            </span>
                          {:else}
                            <span class="px-3 py-1.5 rounded text-[11px] font-mono font-semibold border bg-[rgb(var(--color-bg-tertiary))]/30 border-[rgb(var(--color-border-primary))] text-[rgb(var(--color-text-secondary))]">
                              {portLabel}
                            </span>
                          {/if}
                        {/each}
                      </dd>
                    </div>
                  {/if}
                </div>
              </div>
            {/if}

            <!-- Volume Mounts -->
            {#if details.mounts && details.mounts.length > 0}
              <div class="bg-[rgb(var(--color-bg-secondary))] rounded-lg border border-[rgb(var(--color-border-primary))] overflow-hidden">
                <div class="px-5 py-3 border-b border-[rgb(var(--color-border-primary))] bg-[rgb(var(--color-bg-tertiary))]/30">
                  <h2 class="text-sm font-semibold text-[rgb(var(--color-text-primary))] flex items-center gap-2">
                    <HardDrive class="w-4 h-4" />
                    Volume Mounts
                    <Badge variant="default" size="xs">{details.mounts.length}</Badge>
                  </h2>
                </div>
                <div class="p-6 space-y-3">
                  {#each details.mounts as mount}
                    <div class="border border-[rgb(var(--color-border-primary))] rounded-md bg-[rgb(var(--color-bg-tertiary))]/30 p-4">
                      <div class="flex items-start justify-between mb-3 gap-3">
                        <div class="flex-1">
                          <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2">Container Path</dt>
                          <dd class="text-xs font-mono font-semibold text-[rgb(var(--color-text-primary))] break-all">{mount.destination}</dd>
                        </div>
                        <div class="flex items-center gap-1.5">
                          {#if mount.mountType}
                            <Badge variant="default" size="xs">
                              {mount.mountType}
                            </Badge>
                          {/if}
                          <Badge variant={mount.mode === 'rw' ? 'default' : 'warning'} size="xs">
                            {mount.mode.toUpperCase()}
                          </Badge>
                        </div>
                      </div>
                      <div class="bg-[rgb(var(--color-bg-primary))] rounded p-3 border border-[rgb(var(--color-border-primary))]/30">
                        <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2">Host Path</dt>
                        <dd class="text-xs font-mono text-[rgb(var(--color-text-primary))] break-all">{mount.source}</dd>
                      </div>
                    </div>
                  {/each}
                </div>
              </div>
            {/if}

            <!-- Resource Limits -->
            {#if details.limits}
              <div class="bg-[rgb(var(--color-bg-secondary))] rounded-lg border border-[rgb(var(--color-border-primary))] overflow-hidden">
                <div class="px-5 py-3 border-b border-[rgb(var(--color-border-primary))] bg-[rgb(var(--color-bg-tertiary))]/30">
                  <h2 class="text-sm font-semibold text-[rgb(var(--color-text-primary))] flex items-center gap-2">
                    <Activity class="w-4 h-4" />
                    Resource Limits
                  </h2>
                </div>
                <div class="p-6">
                  <dl class="grid grid-cols-3 gap-4">
                    <div class="bg-gradient-to-br from-[rgb(var(--color-bg-tertiary))]/40 to-[rgb(var(--color-bg-tertiary))]/10 rounded-lg p-4 border border-[rgb(var(--color-border-primary))]">
                      <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-3 flex items-center gap-2">
                        <MemoryStick class="w-3.5 h-3.5" />
                        Memory Limit
                      </dt>
                      <dd class="text-lg font-bold text-[rgb(var(--color-text-primary))] tabular-nums">
                        {details.limits.memoryLimitBytes ? formatBytes(details.limits.memoryLimitBytes) : 'Unlimited'}
                      </dd>
                    </div>
                    <div class="bg-gradient-to-br from-[rgb(var(--color-bg-tertiary))]/40 to-[rgb(var(--color-bg-tertiary))]/10 rounded-lg p-4 border border-[rgb(var(--color-border-primary))]">
                      <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-3 flex items-center gap-2">
                        <Cpu class="w-3.5 h-3.5" />
                        CPU Limit
                      </dt>
                      <dd class="text-lg font-bold text-[rgb(var(--color-text-primary))] tabular-nums">
                        {details.limits.cpuLimit ? formatCpuLimit(details.limits.cpuLimit) : 'Unlimited'}
                      </dd>
                    </div>
                    {#if details.limits.pidsLimit}
                      <div class="bg-gradient-to-br from-[rgb(var(--color-bg-tertiary))]/40 to-[rgb(var(--color-bg-tertiary))]/10 rounded-lg p-4 border border-[rgb(var(--color-border-primary))]">
                        <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-3 flex items-center gap-2">
                          <Activity class="w-3.5 h-3.5" />
                          Process Limit
                        </dt>
                        <dd class="text-lg font-bold text-[rgb(var(--color-text-primary))] tabular-nums">
                          {details.limits.pidsLimit}
                        </dd>
                      </div>
                    {/if}
                  </dl>
                </div>
              </div>
            {/if}

            <!-- Healthcheck Configuration -->
            {#if details.healthcheck}
              <div class="bg-[rgb(var(--color-bg-secondary))] rounded-lg border border-[rgb(var(--color-border-primary))] overflow-hidden">
                <div class="px-5 py-3 border-b border-[rgb(var(--color-border-primary))] bg-[rgb(var(--color-bg-tertiary))]/30">
                  <h2 class="text-sm font-semibold text-[rgb(var(--color-text-primary))] flex items-center gap-2">
                    <Heart class="w-4 h-4" />
                    Healthcheck
                  </h2>
                </div>
                <div class="p-6 space-y-4">
                  {#if details.healthcheck.test.length > 0}
                    <div>
                      <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-3">Test Command</dt>
                      <dd class="bg-[#0D0E12] rounded-md px-4 py-3 font-mono text-xs text-[rgb(var(--color-text-primary))] border border-[rgb(var(--color-border-primary))] leading-relaxed">
                        {details.healthcheck.test.join(' ')}
                      </dd>
                    </div>
                  {/if}
                  <dl class="grid grid-cols-2 md:grid-cols-4 gap-4">
                    <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                      <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2">Interval</dt>
                      <dd class="text-xs font-mono font-semibold text-[rgb(var(--color-text-primary))]">{formatNsDuration(details.healthcheck.intervalNs)}</dd>
                    </div>
                    <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                      <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2">Timeout</dt>
                      <dd class="text-xs font-mono font-semibold text-[rgb(var(--color-text-primary))]">{formatNsDuration(details.healthcheck.timeoutNs)}</dd>
                    </div>
                    <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                      <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2">Retries</dt>
                      <dd class="text-xs font-mono font-semibold text-[rgb(var(--color-text-primary))]">{details.healthcheck.retries}</dd>
                    </div>
                    <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                      <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2">Start Period</dt>
                      <dd class="text-xs font-mono font-semibold text-[rgb(var(--color-text-primary))]">{formatNsDuration(details.healthcheck.startPeriodNs)}</dd>
                    </div>
                  </dl>
                </div>
              </div>
            {/if}
          {/if}
        {:else if activeTab === 'realtime'}
          <!-- Real-time Stats Component -->
          {#if container}
            <RealTimeStats containerId={container.id} agentId={container.agentId} />
          {/if}
        {:else if activeTab === 'logs'}
          <!-- Logs Tab - Full Space -->
          <div class="h-full -mx-8 -my-5 flex flex-col">
            <!-- Compact Controls Bar -->
            {#if showFilters}
              <div class="px-8 py-4 bg-[rgb(var(--color-bg-secondary))] border-b border-[rgb(var(--color-border-primary))]">
                <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
                  <!-- Tail Lines -->
                  <div>
                    <label class="block text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2">
                      Tail Lines
                    </label>
                    <input
                      type="number"
                      bind:value={tailLines}
                      min="1"
                      max="10000"
                      class="w-full px-3 py-2 text-xs bg-[rgb(var(--color-bg-primary))] text-[rgb(var(--color-text-primary))] rounded border border-[rgb(var(--color-border-primary))] focus:outline-none focus:ring-2 focus:ring-[rgb(var(--color-accent-blue))]"
                    />
                  </div>

                  <!-- Filter Pattern -->
                  <div>
                    <label class="block text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2">
                      Filter Pattern (regex)
                    </label>
                    <input
                      type="text"
                      bind:value={filterPattern}
                      placeholder="error|warn|fail"
                      class="w-full px-3 py-2 text-xs bg-[rgb(var(--color-bg-primary))] text-[rgb(var(--color-text-primary))] rounded border border-[rgb(var(--color-border-primary))] focus:outline-none focus:ring-2 focus:ring-[rgb(var(--color-accent-blue))]"
                    />
                  </div>

                  <!-- Filter Mode -->
                  <div>
                    <label class="block text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2">
                      Filter Mode
                    </label>
                    <select
                      bind:value={filterMode}
                      class="w-full px-3 py-2 text-xs bg-[rgb(var(--color-bg-primary))] text-[rgb(var(--color-text-primary))] rounded border border-[rgb(var(--color-border-primary))] focus:outline-none focus:ring-2 focus:ring-[rgb(var(--color-accent-blue))]"
                    >
                      <option value="NONE">No Filter</option>
                      <option value="INCLUDE">Include Only</option>
                      <option value="EXCLUDE">Exclude</option>
                    </select>
                  </div>

                  {#if viewMode === 'historical'}
                    <!-- Since Date -->
                    <div>
                      <label class="block text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2">
                        Since Date
                      </label>
                      <input
                        type="date"
                        bind:value={sinceDate}
                        class="w-full px-3 py-2 text-xs bg-[rgb(var(--color-bg-primary))] text-[rgb(var(--color-text-primary))] rounded border border-[rgb(var(--color-border-primary))] focus:outline-none focus:ring-2 focus:ring-[rgb(var(--color-accent-blue))]"
                      />
                    </div>

                    <!-- Since Time -->
                    <div>
                      <label class="block text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2">
                        Since Time
                      </label>
                      <input
                        type="time"
                        bind:value={sinceTime}
                        class="w-full px-3 py-2 text-xs bg-[rgb(var(--color-bg-primary))] text-[rgb(var(--color-text-primary))] rounded border border-[rgb(var(--color-border-primary))] focus:outline-none focus:ring-2 focus:ring-[rgb(var(--color-accent-blue))]"
                      />
                    </div>

                    <!-- Until Date -->
                    <div>
                      <label class="block text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2">
                        Until Date
                      </label>
                      <input
                        type="date"
                        bind:value={untilDate}
                        class="w-full px-3 py-2 text-xs bg-[rgb(var(--color-bg-primary))] text-[rgb(var(--color-text-primary))] rounded border border-[rgb(var(--color-border-primary))] focus:outline-none focus:ring-2 focus:ring-[rgb(var(--color-accent-blue))]"
                      />
                    </div>

                    <!-- Until Time -->
                    <div>
                      <label class="block text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2">
                        Until Time
                      </label>
                      <input
                        type="time"
                        bind:value={untilTime}
                        class="w-full px-3 py-2 text-xs bg-[rgb(var(--color-bg-primary))] text-[rgb(var(--color-text-primary))] rounded border border-[rgb(var(--color-border-primary))] focus:outline-none focus:ring-2 focus:ring-[rgb(var(--color-accent-blue))]"
                      />
                    </div>
                  {/if}
                </div>

                <!-- Apply Button -->
                <div class="mt-4 flex items-center gap-2">
                  {#if viewMode === 'historical'}
                    <Button variant="primary" size="sm" onclick={loadHistoricalLogs}>
                      <RefreshCw class="w-4 h-4" />
                      Apply Filters
                    </Button>
                  {/if}
                  <Button 
                    variant="ghost" 
                    size="sm" 
                    onclick={() => {
                      filterPattern = '';
                      filterMode = 'NONE';
                      tailLines = 100;
                      sinceDate = '';
                      sinceTime = '';
                      untilDate = '';
                      untilTime = '';
                      if (viewMode === 'historical') loadHistoricalLogs();
                    }}
                  >
                    Clear Filters
                  </Button>
                </div>
              </div>
            {/if}

            <!-- Log Container - Full Height -->
            <div class="flex-1 overflow-hidden bg-[rgb(var(--color-bg-primary))] flex flex-col">
              {#if logsError}
                <div class="h-full flex items-center justify-center">
                  <div class="text-center max-w-md">
                    <Terminal class="w-12 h-12 text-red-400 mx-auto mb-3 opacity-40" />
                    <p class="text-sm text-red-400 mb-2">Failed to load logs</p>
                    <p class="text-xs text-[rgb(var(--color-text-tertiary))]">{logsError.getUserMessage()}</p>
                  </div>
                </div>
              {:else if logs.length === 0}
                <div class="h-full flex items-center justify-center">
                  <div class="text-center">
                    <Terminal class="w-12 h-12 text-[rgb(var(--color-text-tertiary))] mx-auto mb-3 opacity-40" />
                    <p class="text-sm text-[rgb(var(--color-text-tertiary))]">
                      {#if viewMode === 'streaming'}
                        {#if isStreaming}
                          <span class="flex items-center justify-center gap-2">
                            <span class="inline-block w-2 h-2 bg-green-500 rounded-full animate-pulse"></span>
                            Connected - Waiting for logs...
                          </span>
                        {:else}
                          Connecting...
                        {/if}
                      {:else}
                        No logs found for the selected time range
                      {/if}
                    </p>
                    <p class="text-xs text-[rgb(var(--color-text-tertiary))] mt-2">
                      Container: {container?.name}
                    </p>
                  </div>
                </div>
              {:else}
                <!-- Table Container - Scrollable in both directions -->
                <div
                  bind:this={logContainer}
                  class="flex-1 overflow-auto"
                >
                  <div class="min-w-max">
                    <!-- Table Header - Sticky -->
                    <div class="sticky top-0 z-10 bg-[rgb(var(--color-bg-tertiary))] border-b-2 border-[rgb(var(--color-border-secondary))] shadow-sm">
                      <div class="flex px-4 py-2 font-mono text-xs font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider">
                        <div style="{columnWidths.timestamp === 'auto' ? 'width: auto; min-width: 140px;' : `width: ${columnWidths.timestamp}px; min-width: ${columnWidths.timestamp}px;`}" class="flex items-center justify-between group relative pr-2 border-r border-[rgb(var(--color-border-secondary))]/50">
                          <span>Timestamp</span>
                          <div 
                            class="absolute right-0 top-0 bottom-0 w-1.5 cursor-col-resize hover:bg-[rgb(var(--color-accent-blue))] opacity-0 group-hover:opacity-100 transition-opacity"
                            onmousedown={(e) => startResize('timestamp', e)}
                          ></div>
                        </div>
                        <div style="{columnWidths.container === 'auto' ? 'width: auto; min-width: 150px;' : `width: ${columnWidths.container}px; min-width: ${columnWidths.container}px;`}" class="flex items-center justify-between group relative px-2 border-r border-[rgb(var(--color-border-secondary))]/50">
                          <span>Container</span>
                          <div 
                            class="absolute right-0 top-0 bottom-0 w-1.5 cursor-col-resize hover:bg-[rgb(var(--color-accent-blue))] opacity-0 group-hover:opacity-100 transition-opacity"
                            onmousedown={(e) => startResize('container', e)}
                          ></div>
                        </div>
                        <div style="{columnWidths.level === 'auto' ? 'width: auto; min-width: 100px;' : `width: ${columnWidths.level}px; min-width: ${columnWidths.level}px;`}" class="flex items-center justify-between group relative px-2 border-r border-[rgb(var(--color-border-secondary))]/50">
                          <span>Level</span>
                          <div 
                            class="absolute right-0 top-0 bottom-0 w-1.5 cursor-col-resize hover:bg-[rgb(var(--color-accent-blue))] opacity-0 group-hover:opacity-100 transition-opacity"
                            onmousedown={(e) => startResize('level', e)}
                          ></div>
                        </div>
                        <div style="{columnWidths.logger === 'auto' ? 'width: auto; min-width: 120px;' : `width: ${columnWidths.logger}px; min-width: ${columnWidths.logger}px;`}" class="flex items-center justify-between group relative px-2 border-r border-[rgb(var(--color-border-secondary))]/50">
                          <span>Logger</span>
                          <div 
                            class="absolute right-0 top-0 bottom-0 w-1.5 cursor-col-resize hover:bg-[rgb(var(--color-accent-blue))] opacity-0 group-hover:opacity-100 transition-opacity"
                            onmousedown={(e) => startResize('logger', e)}
                          ></div>
                        </div>
                        <div style="{columnWidths.status === 'auto' ? 'width: auto; min-width: 80px;' : `width: ${columnWidths.status}px; min-width: ${columnWidths.status}px;`}" class="flex items-center justify-between group relative px-2 border-r border-[rgb(var(--color-border-secondary))]/50">
                          <span>Status</span>
                          <div 
                            class="absolute right-0 top-0 bottom-0 w-1.5 cursor-col-resize hover:bg-[rgb(var(--color-accent-blue))] opacity-0 group-hover:opacity-100 transition-opacity"
                            onmousedown={(e) => startResize('status', e)}
                          ></div>
                        </div>
                        <div class="flex-1 pl-2 min-w-[300px]">
                          <span>Message</span>
                        </div>
                      </div>
                    </div>

                    <!-- Table Body -->
                  {#each logs as log, index (log.sequence)}
                    <div 
                      class="flex px-4 py-2 text-xs font-mono border-b border-[rgb(var(--color-border-primary))] hover:bg-[rgb(var(--color-bg-tertiary))] transition-colors {index % 2 === 0 ? 'bg-[rgb(var(--color-bg-primary))]' : 'bg-[rgb(var(--color-bg-secondary))]'}"
                    >
                      <!-- Timestamp -->
                      <div style="{columnWidths.timestamp === 'auto' ? 'width: auto; min-width: 140px;' : `width: ${columnWidths.timestamp}px; min-width: ${columnWidths.timestamp}px;`}" class="text-[rgb(var(--color-text-tertiary))] self-start overflow-hidden text-ellipsis pr-2 border-r border-[rgb(var(--color-border-primary))]/30">
                        {new Date(log.timestamp).toLocaleString('en-US', { 
                          month: 'short', 
                          day: '2-digit', 
                          year: 'numeric', 
                          hour: '2-digit', 
                          minute: '2-digit', 
                          second: '2-digit', 
                          hour12: false 
                        })}
                      </div>

                      <!-- Container -->
                      <div style="{columnWidths.container === 'auto' ? 'width: auto; min-width: 150px;' : `width: ${columnWidths.container}px; min-width: ${columnWidths.container}px;`}" class="text-[rgb(var(--color-text-secondary))] self-start truncate px-2 border-r border-[rgb(var(--color-border-primary))]/30">
                        {container?.name || 'N/A'}
                      </div>

                      <!-- Level -->
                      <div style="{columnWidths.level === 'auto' ? 'width: auto; min-width: 100px;' : `width: ${columnWidths.level}px; min-width: ${columnWidths.level}px;`}" class="self-start px-2 border-r border-[rgb(var(--color-border-primary))]/30">
                        {#if log.parsed?.level}
                          <span class="px-2 py-0.5 rounded text-xs font-bold {getBadgeColor(log.parsed.level)}">
                            {log.parsed.level.toUpperCase()}
                          </span>
                        {:else if log.level === 'STDERR'}
                          <span class="px-2 py-0.5 rounded text-xs font-bold bg-red-900/50 text-red-300">
                            STDERR
                          </span>
                        {:else}
                          <span class="text-[rgb(var(--color-text-tertiary))]">-</span>
                        {/if}
                      </div>

                      <!-- Logger -->
                      <div style="{columnWidths.logger === 'auto' ? 'width: auto; min-width: 120px;' : `width: ${columnWidths.logger}px; min-width: ${columnWidths.logger}px;`}" class="text-[rgb(var(--color-text-secondary))] self-start truncate px-2 border-r border-[rgb(var(--color-border-primary))]/30">
                        {log.parsed?.logger || '-'}
                      </div>

                      <!-- Status (HTTP Status Code) -->
                      <div style="{columnWidths.status === 'auto' ? 'width: auto; min-width: 80px;' : `width: ${columnWidths.status}px; min-width: ${columnWidths.status}px;`}" class="self-start px-2 border-r border-[rgb(var(--color-border-primary))]/30">
                        {#if log.parsed?.request?.statusCode}
                          <span class="font-bold {getStatusColor(log.parsed.request.statusCode)}">
                            {log.parsed.request.statusCode}
                          </span>
                        {:else}
                          <span class="text-[rgb(var(--color-text-tertiary))]">-</span>
                        {/if}
                      </div>

                      <!-- Message -->
                      <div class="{getLogColor(log)} break-words leading-relaxed flex-1 pl-2 min-w-[300px]">
                        {#if log.parsed?.message}
                          {log.parsed.message}
                        {:else}
                          {log.content}
                        {/if}
                        
                        <!-- HTTP Request Info (inline) -->
                        {#if log.parsed?.request}
                          <span class="ml-2 text-blue-400">
                            {log.parsed.request.method} {log.parsed.request.path}
                          </span>
                          {#if log.parsed.request.durationMs}
                            <span class="ml-2 text-[rgb(var(--color-text-tertiary))]">
                              ({formatDuration(log.parsed.request.durationMs)})
                            </span>
                          {/if}
                        {/if}

                        <!-- Error indicator -->
                        {#if log.parsed?.error}
                          <span class="ml-2 text-red-400">
                            {log.parsed.error.errorType || 'Error'}
                            {#if log.parsed.error.errorMessage}
                              : {log.parsed.error.errorMessage}
                            {/if}
                          </span>
                        {/if}

                        <!-- Multiline indicator -->
                        {#if log.isGrouped}
                          <span class="ml-2 text-amber-400">
                            ({log.lineCount} lines)
                          </span>
                        {/if}
                      </div>
                    </div>
                  {/each}
                  </div>
                </div>
              {/if}
            </div>
          </div>
        {/if}
      </div>
    {/if}
  </div>
</div>
