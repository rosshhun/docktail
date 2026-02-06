<script lang="ts">
  import { link } from 'svelte-spa-router';
  import { Server, Box, Network, Activity } from '@lucide/svelte';
  import StatusDot from '../lib/common/StatusDot.svelte';
  import ContainerListRow from '../lib/containers/ContainerListRow.svelte';
  import PageHeader from '../lib/common/PageHeader.svelte';
  import RefreshButton from '../lib/common/RefreshButton.svelte';
  import LoadingState from '../lib/common/LoadingState.svelte';
  import ErrorState from '../lib/common/ErrorState.svelte';
  import StatCard from '../lib/common/StatCard.svelte';
  import DataTable from '../lib/common/DataTable.svelte';
  import EmptyState from '../lib/common/EmptyState.svelte';
  import { 
    fetchContainers, 
    fetchHealth,
    type ContainerWithAgent, 
    type HealthStatus,
    GraphQLError 
  } from '../lib/api';
  import { logger } from '../lib/utils/logger';

  let allContainers = $state<ContainerWithAgent[]>([]);
  let clusterContainer = $state<ContainerWithAgent | null>(null);
  let healthStatus = $state<HealthStatus | null>(null);
  let isLoading = $state(true);
  let error = $state<GraphQLError | null>(null);

  // Use $effect to load data on mount
  $effect(() => {
    loadClusterData();
  });

  async function loadClusterData() {
    try {
      isLoading = true;
      error = null;
      
      // Fetch containers and health in parallel
      const [containers, health] = await Promise.all([
        fetchContainers(),
        fetchHealth().catch(() => null)
      ]);
      
      allContainers = containers;
      healthStatus = health;
      
      // Find the cluster container
      clusterContainer = containers.find(c => c.name.startsWith('docktail-cluster')) || null;
      
      logger.debug('[Cluster] Loaded cluster container:', clusterContainer?.name);
    } catch (err: any) {
      if (err instanceof GraphQLError) {
        error = err;
      } else {
        error = new GraphQLError(err.message || 'Failed to load cluster data', 'INTERNAL_SERVER_ERROR');
      }
      logger.error('[Cluster] Failed to load:', err);
    } finally {
      isLoading = false;
    }
  }

  async function handleRetry() {
    await loadClusterData();
  }

  const totalAgents = $derived(() => {
    const uniqueAgents = new Set(allContainers.map(c => c.agentId));
    return uniqueAgents.size;
  });

  const totalContainers = $derived(() => allContainers.length);
  const runningContainers = $derived(() => allContainers.filter(c => c.state === 'RUNNING').length);
</script>

<div class="flex flex-col h-full bg-[rgb(var(--color-bg-primary))]">
  <!-- Page Header -->
  <PageHeader 
    title="Cluster" 
    subtitle="Cluster infrastructure and health monitoring"
  >
    {#snippet actions()}
      <RefreshButton onclick={loadClusterData} disabled={isLoading} />
    {/snippet}
  </PageHeader>

  <!-- Content -->
  <div class="flex-1 overflow-auto px-8 py-4">
    {#if isLoading}
      <LoadingState message="Loading cluster data..." />
    {:else if error}
      <ErrorState error={error} onRetry={handleRetry} title="Failed to load cluster data" />
    {:else}
      <div class="space-y-6">
        <!-- Overview Cards -->
        <div class="grid grid-cols-1 md:grid-cols-4 gap-4">
          <!-- Cluster Health -->
          <StatCard title="Health" icon={Activity}>
            {#if healthStatus?.status === 'healthy'}
              <div class="text-center">
                <div class="w-12 h-12 mx-auto mb-2 rounded-full bg-green-100 flex items-center justify-center">
                  <svg class="w-6 h-6 text-green-600" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7" />
                  </svg>
                </div>
                <p class="text-sm font-medium text-green-700">Operational</p>
              </div>
            {:else}
              <div class="text-center">
                <div class="w-12 h-12 mx-auto mb-2 rounded-full bg-yellow-100 flex items-center justify-center">
                  <Activity class="w-6 h-6 text-yellow-600" />
                </div>
                <p class="text-sm font-medium text-yellow-700">Unknown</p>
              </div>
            {/if}
          </StatCard>

          <!-- Total Agents -->
          <StatCard title="Agents" icon={Server} value={totalAgents()} subtitle="Connected" />

          <!-- Total Containers -->
          <StatCard title="Containers" icon={Box} value={totalContainers()}>
            <div class="text-center">
              <div class="text-3xl font-bold text-[rgb(var(--color-text-primary))]">
                {totalContainers()}
              </div>
              <p class="text-xs text-[rgb(var(--color-text-secondary))] mt-1">
                <span class="text-green-600 font-semibold">{runningContainers()}</span> running
              </p>
            </div>
          </StatCard>

          <!-- Cluster Status -->
          <StatCard title="Cluster Node" icon={Network}>
            <div class="text-center">
              {#if clusterContainer}
                <div class="flex flex-col items-center">
                  <StatusDot status={clusterContainer.state === 'RUNNING' ? 'running' : 'stopped'} animated={clusterContainer.state === 'RUNNING'} size="md" />
                  <p class="text-xs text-[rgb(var(--color-text-secondary))] mt-2">
                    {clusterContainer.state}
                  </p>
                </div>
              {:else}
                <div class="text-xs text-[rgb(var(--color-text-tertiary))]">Not found</div>
              {/if}
            </div>
          </StatCard>
        </div>

        <!-- Cluster Container Details -->
        {#if clusterContainer}
          <div class="bg-[rgb(var(--color-bg-secondary))] rounded-lg border border-[rgb(var(--color-border-primary))] overflow-hidden">
            <div class="px-6 py-4 border-b border-[rgb(var(--color-border-primary))]">
              <h2 class="text-lg font-semibold text-[rgb(var(--color-text-primary))]">
                Cluster Container
              </h2>
            </div>
            <DataTable columns={[
              { key: 'status', label: 'Status', width: 'w-16' },
              { key: 'name', label: 'Name' },
              { key: 'resources', label: 'Resources', width: 'w-40' },
              { key: 'ports', label: 'Ports', width: 'w-32' },
              { key: 'uptime', label: 'Uptime', width: 'w-28' },
              { key: 'actions', label: 'Actions', width: 'w-24' }
            ]}>
              <ContainerListRow container={clusterContainer} showAgentColumn={false} />
            </DataTable>
          </div>
        {:else}
          <EmptyState 
            icon={Network}
            title="No Cluster Container Found"
            message="The cluster container (docktail-cluster) is not running or cannot be found."
          />
        {/if}
      </div>
    {/if}
  </div>
</div>
