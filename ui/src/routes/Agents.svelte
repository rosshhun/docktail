<script lang="ts">
  import AgentListRow from '../lib/agents/AgentListRow.svelte';
  import SearchInput from '../lib/common/SearchInput.svelte';
  import FilterButton from '../lib/common/FilterButton.svelte';
  import PageHeader from '../lib/common/PageHeader.svelte';
  import RefreshButton from '../lib/common/RefreshButton.svelte';
  import LoadingState from '../lib/common/LoadingState.svelte';
  import ErrorState from '../lib/common/ErrorState.svelte';
  import EmptyState from '../lib/common/EmptyState.svelte';
  import DataTable from '../lib/common/DataTable.svelte';
  import { Server, SlidersHorizontal, Calendar } from '@lucide/svelte';
  import { fetchAgents, fetchContainers, type Agent, type ContainerWithAgent, GraphQLError } from '../lib/api';
  import { logger } from '../lib/utils/logger';

  let allAgents = $state<Agent[]>([]);
  let allContainers = $state<ContainerWithAgent[]>([]);
  let isLoading = $state(true);
  let error = $state<GraphQLError | null>(null);
  let searchQuery = $state('');
  let statusFilter = $state<string[]>([]);
  let sortBy = $state<'name' | 'status' | 'containers'>('name');
  let sortOrder = $state<'asc' | 'desc'>('asc');

  // Use $effect to load agents on mount
  $effect(() => {
    loadAgents();
  });

  async function loadAgents() {
    try {
      isLoading = true;
      error = null;
      // Fetch both agents and containers in parallel
      const [agents, containers] = await Promise.all([
        fetchAgents(),
        fetchContainers()
      ]);
      allAgents = agents;
      allContainers = containers;
      logger.debug('[Agents] Loaded agents:', allAgents.length, 'containers:', allContainers.length);
    } catch (err: any) {
      if (err instanceof GraphQLError) {
        error = err;
      } else {
        error = new GraphQLError(err.message || 'Failed to load agents', 'INTERNAL_SERVER_ERROR');
      }
      logger.error('[Agents] Failed to load:', err);
    } finally {
      isLoading = false;
    }
  }

  async function handleRetry() {
    await loadAgents();
  }

  const filteredAgents = $derived(() => {
    let result = allAgents;

    // Search filter
    if (searchQuery) {
      result = result.filter(a => 
        a.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
        a.id.toLowerCase().includes(searchQuery.toLowerCase())
      );
    }

    // Status filter
    if (statusFilter.length > 0) {
      result = result.filter(a => statusFilter.includes(a.status.toLowerCase()));
    }

    // Sorting
    result = [...result].sort((a, b) => {
      let comparison = 0;
      
      if (sortBy === 'name') {
        comparison = a.name.localeCompare(b.name);
      } else if (sortBy === 'status') {
        comparison = a.status.localeCompare(b.status);
      } else if (sortBy === 'containers') {
        const aCount = containerCounts().get(a.id)?.total || 0;
        const bCount = containerCounts().get(b.id)?.total || 0;
        comparison = aCount - bCount;
      }
      
      return sortOrder === 'asc' ? comparison : -comparison;
    });

    return result;
  });

  const statusCounts = $derived(() => {
    const counts = new Map<string, number>();
    for (const agent of allAgents) {
      const status = agent.status.toLowerCase();
      counts.set(status, (counts.get(status) || 0) + 1);
    }
    return [
      { id: 'healthy', name: 'Healthy', count: counts.get('healthy') || 0 },
      { id: 'degraded', name: 'Degraded', count: counts.get('degraded') || 0 },
      { id: 'unhealthy', name: 'Unhealthy', count: counts.get('unhealthy') || 0 },
    ].filter(s => s.count > 0);
  });

  function toggleStatus(status: string) {
    if (statusFilter.includes(status)) {
      statusFilter = statusFilter.filter((s: string) => s !== status);
    } else {
      statusFilter = [...statusFilter, status];
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
    const months = Math.floor(days / 30);
    if (months < 12) return `${months}mo ago`;
    const years = Math.floor(months / 12);
    return `${years}y ago`;
  }

  // Calculate container counts per agent
  const containerCounts = $derived(() => {
    const counts = new Map<string, { total: number; running: number }>();
    for (const container of allContainers) {
      const existing = counts.get(container.agentId) || { total: 0, running: 0 };
      existing.total++;
      if (container.state === 'RUNNING') {
        existing.running++;
      }
      counts.set(container.agentId, existing);
    }
    return counts;
  });

  // Convert agent data to display format
  const displayAgents = $derived(() => {
    return filteredAgents().map(agent => {
      const counts = containerCounts().get(agent.id) || { total: 0, running: 0 };
      return {
        id: agent.id,
        name: agent.name,
        status: agent.status.toLowerCase() as 'healthy' | 'degraded' | 'unhealthy' | 'unknown',
        endpoint: agent.address,
        version: agent.version || 'N/A',
        containerCount: counts.total,
        runningCount: counts.running,
      };
    });
  });
</script>

<div class="flex flex-col h-full bg-[rgb(var(--color-bg-primary))]">
  <!-- Page Header -->
  <PageHeader title="Agents">
    <!-- Search and Controls -->
    <div class="flex items-center gap-3 flex-wrap">
      <div class="flex-1 max-w-md">
        <SearchInput placeholder="Search agents..." bind:value={searchQuery} />
      </div>
      
      <FilterButton 
        icon={SlidersHorizontal}
        label="Filters"
        active={statusFilter.length > 0}
        count={statusFilter.length}
        dropdownId="filters-dropdown"
      >
        <div class="min-w-[220px] max-h-[450px] overflow-y-auto">
          <!-- Status Filters -->
          <div class="py-3">
            <div class="px-3 mb-2">
              <h4 class="text-[10px] font-bold text-[rgb(var(--color-text-tertiary))] uppercase tracking-wider">Status</h4>
            </div>
            <div class="space-y-0.5 px-2">
              {#each statusCounts() as status}
                <label class="group flex items-center gap-2.5 cursor-pointer hover:bg-[rgb(var(--color-bg-secondary))] px-2.5 py-2 rounded-lg transition-all duration-150">
                  <input 
                    type="checkbox" 
                    checked={statusFilter.includes(status.id)}
                    onchange={() => toggleStatus(status.id)}
                    class="w-4 h-4 rounded border-2 border-[rgb(var(--color-border-secondary))] bg-[rgb(var(--color-bg-primary))] text-[rgb(var(--color-accent-blue))] focus:ring-0 focus:ring-offset-0 cursor-pointer transition-all hover:border-[rgb(var(--color-accent-blue))]"
                  />
                  <span class="text-xs text-[rgb(var(--color-text-primary))] flex-1 font-medium group-hover:text-[rgb(var(--color-text-primary))] transition-colors">{status.name}</span>
                  <span class="text-[10px] text-[rgb(var(--color-text-secondary))] font-bold bg-[rgb(var(--color-bg-tertiary))] px-2 py-0.5 rounded-full min-w-[24px] text-center">{status.count}</span>
                </label>
              {/each}
            </div>
          </div>
          
          <!-- Clear Filters -->
          <div class="py-3 px-2 border-t-2 border-[rgb(var(--color-border-primary))]">
            <button 
              onclick={() => statusFilter = []}
              disabled={statusFilter.length === 0}
              class="w-full text-xs font-semibold transition-all duration-150 px-3 py-2.5 rounded-lg border-2
                {statusFilter.length > 0
                ? 'text-[rgb(var(--color-text-primary))] hover:text-[rgb(var(--color-text-primary))] hover:bg-[rgb(var(--color-bg-tertiary))] border-[rgb(var(--color-border-primary))] hover:border-[rgb(var(--color-border-secondary))]'
                : 'text-[rgb(var(--color-text-tertiary))] border-[rgb(var(--color-border-primary))] cursor-not-allowed opacity-50'}"
            >
              Clear
            </button>
          </div>
        </div>
      </FilterButton>

      <FilterButton 
        icon={Calendar}
        label="Sort: {sortBy === 'name' ? 'Name' : sortBy === 'status' ? 'Status' : 'Containers'}"
        dropdownId="sort-dropdown"
      >
        <div class="min-w-[200px]">
          <!-- Sort By Section -->
          <div class="py-3">
            <div class="px-3 mb-2">
              <h4 class="text-[10px] font-bold text-[rgb(var(--color-text-tertiary))] uppercase tracking-wider">Sort By</h4>
            </div>
            <div class="space-y-0.5 px-2">
              <button 
                onclick={() => sortBy = 'name'}
                class="group w-full px-2.5 py-2 text-left text-xs hover:bg-[rgb(var(--color-bg-secondary))] text-[rgb(var(--color-text-primary))] font-medium transition-all duration-150 rounded-lg {sortBy === 'name' ? 'bg-[rgb(var(--color-bg-tertiary))] text-[rgb(var(--color-text-primary))] border-2 border-[rgb(var(--color-border-secondary))]' : 'border-2 border-transparent'}"
              >
                <span class="flex items-center gap-2">
                  {#if sortBy === 'name'}
                    <span class="w-2 h-2 rounded-full bg-[rgb(var(--color-accent-blue))]"></span>
                  {:else}
                    <span class="w-2 h-2 rounded-full border-2 border-[#D4D2D7] group-hover:border-[rgb(var(--color-accent-blue))]"></span>
                  {/if}
                  Name
                </span>
              </button>
              <button 
                onclick={() => sortBy = 'status'}
                class="group w-full px-2.5 py-2 text-left text-xs hover:bg-[rgb(var(--color-bg-secondary))] text-[rgb(var(--color-text-primary))] font-medium transition-all duration-150 rounded-lg {sortBy === 'status' ? 'bg-[rgb(var(--color-bg-tertiary))] text-[rgb(var(--color-accent-blue))] border-2 border-[rgb(var(--color-border-secondary))]' : 'border-2 border-transparent'}"
              >
                <span class="flex items-center gap-2">
                  {#if sortBy === 'status'}
                    <span class="w-2 h-2 rounded-full bg-[rgb(var(--color-accent-blue))]"></span>
                  {:else}
                    <span class="w-2 h-2 rounded-full border-2 border-[#D4D2D7] group-hover:border-[rgb(var(--color-accent-blue))]"></span>
                  {/if}
                  Status
                </span>
              </button>
              <button 
                onclick={() => sortBy = 'containers'}
                class="group w-full px-2.5 py-2 text-left text-xs hover:bg-[rgb(var(--color-bg-secondary))] text-[rgb(var(--color-text-primary))] font-medium transition-all duration-150 rounded-lg {sortBy === 'containers' ? 'bg-[rgb(var(--color-bg-tertiary))] text-[rgb(var(--color-accent-blue))] border-2 border-[rgb(var(--color-border-secondary))]' : 'border-2 border-transparent'}"
              >
                <span class="flex items-center gap-2">
                  {#if sortBy === 'containers'}
                    <span class="w-2 h-2 rounded-full bg-[rgb(var(--color-accent-blue))]"></span>
                  {:else}
                    <span class="w-2 h-2 rounded-full border-2 border-[#D4D2D7] group-hover:border-[rgb(var(--color-accent-blue))]"></span>
                  {/if}
                  Containers
                </span>
              </button>
            </div>
          </div>

          <!-- Sort Order Section -->
          <div class="py-3 px-2 border-t-2 border-[rgb(var(--color-border-primary))]">
            <div class="px-1 mb-2">
              <h4 class="text-[10px] font-bold text-[rgb(var(--color-text-tertiary))] uppercase tracking-wider">Order</h4>
            </div>
            <button 
              onclick={() => sortOrder = sortOrder === 'asc' ? 'desc' : 'asc'}
              class="w-full px-2.5 py-2 text-left text-xs bg-[rgb(var(--color-bg-tertiary))] hover:bg-purple-100 text-[rgb(var(--color-accent-blue))] font-medium transition-all duration-150 rounded-lg border-2 border-[rgb(var(--color-border-secondary))]"
            >
              <span class="flex items-center gap-2">
                <span class="text-sm font-bold">{sortOrder === 'asc' ? '↑' : '↓'}</span>
                {sortOrder === 'asc' ? 'Ascending' : 'Descending'}
              </span>
            </button>
          </div>
        </div>
      </FilterButton>

      <RefreshButton onclick={loadAgents} disabled={isLoading} />
    </div>
  </PageHeader>

  <!-- Agent Table -->
  <div class="flex-1 overflow-auto px-8 py-4">
    {#if isLoading}
      <LoadingState message="Loading agents..." />
    {:else if error}
      <ErrorState error={error} onRetry={handleRetry} title="Failed to load agents" />
    {:else if filteredAgents().length === 0}
      <EmptyState 
        icon={Server}
        title="No agents found"
        message={searchQuery || statusFilter.length > 0 
          ? 'Try adjusting your search or filters' 
          : 'No agents are currently connected'}
      />
    {:else}
      <DataTable columns={[
        { key: 'status', label: 'Status', width: 'w-16' },
        { key: 'agent', label: 'Agent' },
        { key: 'containers', label: 'Containers', width: 'w-32' },
        { key: 'version', label: 'Version', width: 'w-24' },
        { key: 'actions', label: 'Actions', width: 'w-24' }
      ]}>
        {#each displayAgents() as agent (agent.id)}
          <AgentListRow agent={agent} />
        {/each}
      </DataTable>
    {/if}
  </div>
</div>
