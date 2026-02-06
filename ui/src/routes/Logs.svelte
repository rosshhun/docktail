<script lang="ts">
  import { link } from 'svelte-spa-router';
  import { ArrowLeft, Play, Pause, Download, Copy, RefreshCw, Filter, Calendar, Clock, Activity, Terminal, Sparkles, Zap } from '@lucide/svelte';
  import Button from '../lib/common/Button.svelte';
  import Badge from '../lib/common/Badge.svelte';
  import FilterButton from '../lib/common/FilterButton.svelte';
  import { 
    fetchContainer, 
    subscribeToLogs, 
    fetchHistoricalLogs,
    type LogEvent, 
    type Container, 
    type Agent, 
    query, 
    GraphQLError 
  } from '../lib/api';

  // Get container ID from route params (Svelte 5 syntax)
  let { params = { id: '' } }: { params?: { id: string } } = $props();

  let container: Container | null = $state(null);
  let agentName: string = $state('');
  let logs: LogEvent[] = $state([]);
  let isStreaming = $state(true);
  let isPaused = $state(false);
  let error: GraphQLError | null = $state(null);
  let isRetrying = $state(false);
  let logContainer = $state<HTMLDivElement>();
  let unsubscribe: (() => void) | null = null;

  // Advanced filtering options
  let viewMode = $state<'streaming' | 'historical'>('streaming');
  let filterPattern = $state('');
  let filterMode = $state<'NONE' | 'INCLUDE' | 'EXCLUDE'>('NONE');
  let tailLines = $state(100);
  let sinceDate = $state('');
  let sinceTime = $state('');
  let untilDate = $state('');
  let untilTime = $state('');
  let showFilters = $state(false);

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

  // Auto-scroll to bottom when new logs arrive
  $effect(() => {
    if (!isPaused && logContainer && logs.length > 0) {
      requestAnimationFrame(() => {
        if (logContainer) {
          logContainer.scrollTop = logContainer.scrollHeight;
        }
      });
    }
  });

  // Use $effect to load container and manage log subscription
  $effect(() => {
    if (!params.id) {
      error = new GraphQLError('No container ID provided', 'BAD_REQUEST');
      return;
    }

    loadContainer();
    
    // Cleanup function for unsubscribing
    return () => {
      unsubscribe?.();
    };
  });

  async function loadContainer() {
    try {
      error = null;
      isRetrying = true;

      // Fetch container details
      container = await fetchContainer(params.id);

      // Fetch agent name
      if (container?.agentId) {
        const result = await query<{ agent: Agent }>(`
          query GetAgent($id: ID!) {
            agent(id: $id) {
              id
              name
            }
          }
        `, { id: container.agentId });
        agentName = result.agent?.name || container.agentId;
      }

      if (viewMode === 'streaming') {
        // Start streaming logs
        if (container?.agentId) {
          unsubscribe = subscribeToLogs(
            params.id,
            container.agentId,
            (logEvent) => {
              logs = [...logs, logEvent];
            },
            (err) => {
              if (err instanceof GraphQLError) {
                error = err;
              } else {
                error = new GraphQLError(err.message || 'Stream error', 'INTERNAL_SERVER_ERROR');
              }
              isStreaming = false;
            }
          );
        }
      } else {
        // Load historical logs
        await loadHistoricalLogs();
      }
    } catch (err: any) {
      if (err instanceof GraphQLError) {
        error = err;
      } else {
        error = new GraphQLError(err.message || 'Failed to load container', 'INTERNAL_SERVER_ERROR');
      }
    } finally {
      isRetrying = false;
    }
  }

  async function loadHistoricalLogs() {
    if (!container?.agentId) return;

    try {
      error = null;
      isRetrying = true;

      // Build date-time strings for since/until
      const sinceTimestamp = sinceDate && sinceTime ? `${sinceDate}T${sinceTime}:00Z` : undefined;
      const untilTimestamp = untilDate && untilTime ? `${untilDate}T${untilTime}:00Z` : undefined;

      const historicalLogs = await fetchHistoricalLogs(
        params.id,
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
        error = err;
      } else {
        error = new GraphQLError(err.message || 'Failed to load historical logs', 'INTERNAL_SERVER_ERROR');
      }
    } finally {
      isRetrying = false;
    }
  }

  function switchMode(mode: 'streaming' | 'historical') {
    unsubscribe?.();
    logs = [];
    viewMode = mode;
    
    if (mode === 'streaming') {
      isStreaming = true;
      loadContainer();
    } else {
      isStreaming = false;
      loadHistoricalLogs();
    }
  }

  async function handleRetry() {
    unsubscribe?.();
    logs = [];
    isStreaming = true;
    await loadContainer();
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
</script>

<svelte:window 
  onmousemove={handleMouseMove} 
  onmouseup={stopResize}
/>

<div class="h-full flex flex-col bg-[rgb(var(--color-bg-primary))]" class:select-none={isResizing} class:cursor-col-resize={isResizing}>
  <!-- Header - Compact Style matching ContainerDetails -->
  <div class="border-b border-[rgb(var(--color-border-primary))] px-8 py-2 sticky top-0 bg-[rgb(var(--color-bg-primary))] z-10 shadow-sm">
    {#if container}
      <!-- Top Row: Breadcrumb & Status -->
      <div class="flex items-center justify-between mb-2 mt-2">
        <!-- Left: Breadcrumb -->
        <div class="flex items-center gap-3 flex-1">
          <a
            use:link
            href="/containers"
            class="flex items-center justify-center w-7 h-7 rounded hover:bg-[rgb(var(--color-bg-secondary))] border border-[rgb(var(--color-border-primary))] text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))] transition-all"
            title="Back to Containers"
          >
            <ArrowLeft class="w-3.5 h-3.5" />
          </a>
          
          <div class="flex items-center gap-2 text-sm text-[rgb(var(--color-text-secondary))]">
            <a use:link href="/" class="hover:text-[rgb(var(--color-text-primary))] transition-colors">Home</a>
            <span>/</span>
            <a use:link href="/containers" class="hover:text-[rgb(var(--color-text-primary))] transition-colors">Containers</a>
            <span>/</span>
            <a use:link href="/containers/{container.id}" class="hover:text-[rgb(var(--color-text-primary))] transition-colors">{container.name}</a>
            <span>/</span>
            <span class="text-[rgb(var(--color-text-primary))] font-semibold">Logs</span>
          </div>

          <Badge variant={container.state === 'RUNNING' ? 'success' : 'default'} size="sm">
            {container.state}
          </Badge>
        </div>

        <!-- Right: Stream Status & Controls -->
        <div class="flex items-center gap-3">
          <!-- Stream Status Indicator -->
          <div class="flex items-center gap-2 px-2.5 py-1 bg-[rgb(var(--color-bg-secondary))] rounded-md border border-[rgb(var(--color-border-primary))]">
            <div class="w-1.5 h-1.5 rounded-full {isStreaming ? 'bg-green-500 animate-pulse' : 'bg-gray-400'}"></div>
            <span class="text-xs font-medium text-[rgb(var(--color-text-secondary))]">
              {isStreaming ? 'Live' : 'Disconnected'}
            </span>
            <span class="text-xs text-[rgb(var(--color-text-tertiary))]">•</span>
            <span class="text-xs font-bold text-[rgb(var(--color-text-primary))] tabular-nums">{logs.length}</span>
          </div>

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
      </div>

      <!-- Filter Panel -->
      {#if showFilters}
        <div class="mb-3 p-4 bg-[rgb(var(--color-bg-secondary))] rounded-lg border border-[rgb(var(--color-border-primary))]">
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
    {/if}
  </div>

  <!-- Log Content -->
  <div class="flex-1 overflow-hidden px-8 py-5">
    {#if error}
      <div class="h-full flex items-center justify-center">
        <div class="text-center max-w-md">
          <div class="inline-flex items-center justify-center w-16 h-16 rounded-full mb-4 bg-red-50">
            <svg class="w-8 h-8 text-red-600" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
            </svg>
          </div>
          <h3 class="text-lg font-semibold text-[rgb(var(--color-text-primary))] mb-2">
            {#if error.isNotFound()}
              Container Not Found
            {:else if error.isUnavailable()}
              Service Unavailable
            {:else}
              Error Loading Logs
            {/if}
          </h3>
          <p class="text-sm text-[rgb(var(--color-text-secondary))] mb-4">{error.getUserMessage()}</p>
          <div class="flex gap-2 justify-center">
            {#if error.isRetryable()}
              <Button variant="primary" size="md" disabled={isRetrying} onclick={handleRetry}>
                {#if isRetrying}
                  <RefreshCw class="w-4 h-4 animate-spin" />
                  Retrying...
                {:else}
                  <RefreshCw class="w-4 h-4" />
                  Retry
                {/if}
              </Button>
            {/if}
            <a use:link href="/containers">
              <Button variant="default" size="md">Back to Containers</Button>
            </a>
          </div>
        </div>
      </div>
    {:else if !container}
      <div class="h-full flex items-center justify-center">
        <div class="text-center">
          <div class="inline-block animate-spin rounded-full h-8 w-8 border-b-2 border-[rgb(var(--color-accent-blue))]"></div>
          <p class="text-sm text-[rgb(var(--color-text-secondary))] mt-3">Loading container logs...</p>
        </div>
      </div>
    {:else}
      <!-- Logs Card -->
      <div class="h-full bg-[rgb(var(--color-bg-secondary))] rounded-lg border border-[rgb(var(--color-border-primary))] overflow-hidden flex flex-col">
        <!-- Card Header -->
        <div class="px-5 py-3 border-b border-[rgb(var(--color-border-primary))] bg-[rgb(var(--color-bg-tertiary))]/30 flex items-center justify-between">
          <div class="flex items-center gap-2">
            <Terminal class="w-4 h-4 text-[rgb(var(--color-text-secondary))]" />
            <h2 class="text-sm font-semibold text-[rgb(var(--color-text-primary))]">
              {viewMode === 'streaming' ? 'Live Stream' : 'Historical Logs'}
            </h2>
          </div>
          {#if isPaused}
            <Badge variant="warning" size="xs">
              Paused
            </Badge>
          {/if}
        </div>

        <!-- Log Container -->
        <div
          bind:this={logContainer}
          class="flex-1 overflow-auto bg-[rgb(var(--color-bg-primary))]"
        >
          {#if logs.length === 0}
            <div class="h-full flex items-center justify-center">
              <div class="text-center">
                <Terminal class="w-12 h-12 text-[rgb(var(--color-text-tertiary))] mx-auto mb-3 opacity-40" />
                <p class="text-sm text-[rgb(var(--color-text-tertiary))]">
                  {viewMode === 'streaming' ? 'Waiting for logs...' : 'No logs found for the selected time range'}
                </p>
              </div>
            </div>
          {:else}
            <!-- Table View -->
            <div class="min-w-max">
              <!-- Table Header -->
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
          {/if}
        </div>
      </div>
    {/if}
  </div>
</div>
