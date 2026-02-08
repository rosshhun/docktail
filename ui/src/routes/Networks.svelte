<script lang="ts">
  import { Network, Search, Globe, Lock, Plug } from '@lucide/svelte';
  import PageHeader from '../lib/common/PageHeader.svelte';
  import DataTable from '../lib/common/DataTable.svelte';
  import Badge from '../lib/common/Badge.svelte';
  import LoadingState from '../lib/common/LoadingState.svelte';
  import EmptyState from '../lib/common/EmptyState.svelte';
  import SearchInput from '../lib/common/SearchInput.svelte';
  import RefreshButton from '../lib/common/RefreshButton.svelte';
  import {
    fetchAgents,
    fetchSwarmInfo,
    fetchSwarmNetworks,
    type SwarmNetworkView,
    GraphQLError,
  } from '../lib/api';

  let networks = $state<SwarmNetworkView[]>([]);
  let isLoading = $state(true);
  let error = $state<GraphQLError | null>(null);
  let searchQuery = $state('');
  let expandedId = $state('');

  $effect(() => { loadNetworks(); });

  async function loadNetworks() {
    try {
      isLoading = true;
      error = null;
      const agents = await fetchAgents();
      let swarmAgentId = '';
      for (const agent of agents) {
        try {
          const info = await fetchSwarmInfo(agent.id);
          if (info?.isSwarmMode) { swarmAgentId = agent.id; break; }
        } catch { /* next */ }
      }
      if (swarmAgentId) {
        networks = await fetchSwarmNetworks(swarmAgentId);
      }
    } catch (err: any) {
      error = err instanceof GraphQLError ? err : new GraphQLError(err.message, 'INTERNAL_SERVER_ERROR');
    } finally {
      isLoading = false;
    }
  }

  const filteredNetworks = $derived(
    networks.filter(n => n.name.toLowerCase().includes(searchQuery.toLowerCase()) || n.driver.toLowerCase().includes(searchQuery.toLowerCase()))
  );
</script>

<div class="h-full flex flex-col bg-[rgb(var(--color-bg-primary))]">
  <div class="px-8 pt-3">
    <PageHeader title="Networks" subtitle="Docker Swarm networks" icon={Network}>
      {#snippet actions()}
        <SearchInput bind:value={searchQuery} placeholder="Search networks..." />
        <RefreshButton onclick={loadNetworks} />
      {/snippet}
    </PageHeader>
  </div>

  <div class="flex-1 overflow-auto px-8 pb-8">
    {#if isLoading}
      <LoadingState message="Loading networks..." />
    {:else if error}
      <div class="p-8 text-center"><p class="text-sm text-red-400">{error.getUserMessage()}</p></div>
    {:else if filteredNetworks.length === 0}
      <EmptyState icon={Network} title="No networks found" message={searchQuery ? 'No networks match your search.' : 'No swarm networks found.'} />
    {:else}
      <div class="space-y-3">
        {#each filteredNetworks as net (net.id)}
          <div class="bg-[rgb(var(--color-bg-secondary))] rounded-lg border border-[rgb(var(--color-border-primary))] overflow-hidden">
            <!-- Network row -->
            <button class="w-full flex items-center gap-4 px-5 py-3 text-left hover:bg-[rgb(var(--color-bg-tertiary))]/50 cursor-pointer transition-colors" onclick={() => expandedId = expandedId === net.id ? '' : net.id}>
              <Network class="w-4 h-4 text-[rgb(var(--color-text-secondary))] shrink-0" />
              <div class="flex-1 min-w-0">
                <span class="text-sm font-medium text-[rgb(var(--color-text-primary))]">{net.name}</span>
                <span class="ml-2 text-xs font-mono text-[rgb(var(--color-text-secondary))] truncate">{net.id.slice(0, 12)}</span>
              </div>
              <div class="flex items-center gap-2">
                <Badge variant="default" size="xs">{net.driver}</Badge>
                <Badge variant="info" size="xs">{net.scope}</Badge>
                {#if net.isIngress}<Badge variant="warning" size="xs">Ingress</Badge>{/if}
                {#if net.isInternal}<Badge variant="default" size="xs"><Lock class="w-3 h-3 inline-block mr-0.5" />Internal</Badge>{/if}
                {#if net.isAttachable}<Badge variant="success" size="xs"><Plug class="w-3 h-3 inline-block mr-0.5" />Attachable</Badge>{/if}
                {#if net.enableIpv6}<Badge variant="info" size="xs"><Globe class="w-3 h-3 inline-block mr-0.5" />IPv6</Badge>{/if}
                {#if net.serviceAttachments.length > 0}
                  <Badge variant="default" size="xs">{net.serviceAttachments.length} svc{net.serviceAttachments.length !== 1 ? 's' : ''}</Badge>
                {/if}
              </div>
            </button>

            <!-- Expanded details -->
            {#if expandedId === net.id}
              <div class="border-t border-[rgb(var(--color-border-primary))] px-5 py-4 space-y-4 bg-[rgb(var(--color-bg-tertiary))]/20">
                <!-- IPAM Configs -->
                {#if net.ipamConfigs.length > 0}
                  <div>
                    <h4 class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase mb-2">IPAM Configuration</h4>
                    <div class="grid grid-cols-1 md:grid-cols-3 gap-3">
                      {#each net.ipamConfigs as ipam}
                        <div class="bg-[rgb(var(--color-bg-secondary))] rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
                          {#if ipam.subnet}<div class="text-xs"><span class="text-[rgb(var(--color-text-secondary))]">Subnet:</span> <span class="font-mono text-[rgb(var(--color-text-primary))]">{ipam.subnet}</span></div>{/if}
                          {#if ipam.gateway}<div class="text-xs"><span class="text-[rgb(var(--color-text-secondary))]">Gateway:</span> <span class="font-mono text-[rgb(var(--color-text-primary))]">{ipam.gateway}</span></div>{/if}
                          {#if ipam.ipRange}<div class="text-xs"><span class="text-[rgb(var(--color-text-secondary))]">Range:</span> <span class="font-mono text-[rgb(var(--color-text-primary))]">{ipam.ipRange}</span></div>{/if}
                        </div>
                      {/each}
                    </div>
                  </div>
                {/if}

                <!-- Service Attachments -->
                {#if net.serviceAttachments.length > 0}
                  <div>
                    <h4 class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase mb-2">Attached Services</h4>
                    <div class="grid grid-cols-1 md:grid-cols-2 gap-2">
                      {#each net.serviceAttachments as svc}
                        <div class="flex items-center justify-between bg-[rgb(var(--color-bg-secondary))] rounded-md px-3 py-2 border border-[rgb(var(--color-border-primary))]/50">
                          <a href="/#/swarm/services/{svc.serviceId}" class="text-xs font-medium text-[rgb(var(--color-accent-blue))] hover:underline">{svc.serviceName}</a>
                          <span class="text-xs font-mono text-[rgb(var(--color-text-secondary))]">{svc.virtualIp}</span>
                        </div>
                      {/each}
                    </div>
                  </div>
                {/if}

                <!-- Peers -->
                {#if net.peers.length > 0}
                  <div>
                    <h4 class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase mb-2">Peers</h4>
                    <div class="flex flex-wrap gap-2">
                      {#each net.peers as peer}
                        <div class="bg-[rgb(var(--color-bg-secondary))] rounded-md px-3 py-1.5 border border-[rgb(var(--color-border-primary))]/50 text-xs">
                          <span class="text-[rgb(var(--color-text-primary))]">{peer.name}</span>
                          <span class="text-[rgb(var(--color-text-secondary))] ml-1.5 font-mono">{peer.ip}</span>
                        </div>
                      {/each}
                    </div>
                  </div>
                {/if}

                <!-- Labels -->
                {#if net.labels.length > 0}
                  <div>
                    <h4 class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase mb-2">Labels</h4>
                    <div class="flex flex-wrap gap-1.5">
                      {#each net.labels as label}
                        <span class="text-xs font-mono bg-[rgb(var(--color-bg-secondary))] border border-[rgb(var(--color-border-primary))]/50 rounded px-2 py-0.5 text-[rgb(var(--color-text-secondary))]">{label.key}={label.value}</span>
                      {/each}
                    </div>
                  </div>
                {/if}

                <!-- Options -->
                {#if net.options.length > 0}
                  <div>
                    <h4 class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase mb-2">Options</h4>
                    <div class="flex flex-wrap gap-1.5">
                      {#each net.options as opt}
                        <span class="text-xs font-mono bg-[rgb(var(--color-bg-secondary))] border border-[rgb(var(--color-border-primary))]/50 rounded px-2 py-0.5 text-[rgb(var(--color-text-secondary))]">{opt.key}={opt.value}</span>
                      {/each}
                    </div>
                  </div>
                {/if}

                <!-- Created -->
                <div class="text-xs text-[rgb(var(--color-text-secondary))]">Created: {new Date(net.createdAt).toLocaleString()}</div>
              </div>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>
