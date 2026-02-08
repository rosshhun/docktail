<script lang="ts">
  import { KeyRound, FileCode, Search } from '@lucide/svelte';
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
    fetchSwarmSecrets,
    fetchSwarmConfigs,
    type SwarmSecretView,
    type SwarmConfigView,
    GraphQLError,
  } from '../lib/api';

  let secrets = $state<SwarmSecretView[]>([]);
  let configs = $state<SwarmConfigView[]>([]);
  let isLoading = $state(true);
  let error = $state<GraphQLError | null>(null);
  let searchQuery = $state('');
  let activeTab = $state<'secrets' | 'configs'>('secrets');

  $effect(() => { loadData(); });

  async function loadData() {
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
        const [s, c] = await Promise.all([
          fetchSwarmSecrets(swarmAgentId),
          fetchSwarmConfigs(swarmAgentId),
        ]);
        secrets = s;
        configs = c;
      }
    } catch (err: any) {
      error = err instanceof GraphQLError ? err : new GraphQLError(err.message, 'INTERNAL_SERVER_ERROR');
    } finally {
      isLoading = false;
    }
  }

  const filteredSecrets = $derived(
    secrets.filter(s => s.name.toLowerCase().includes(searchQuery.toLowerCase()))
  );
  const filteredConfigs = $derived(
    configs.filter(c => c.name.toLowerCase().includes(searchQuery.toLowerCase()))
  );
</script>

<div class="h-full flex flex-col bg-[rgb(var(--color-bg-primary))]">
  <div class="px-8 pt-3">
    <PageHeader title="Secrets & Configs" subtitle="Docker Swarm secrets and configuration objects" icon={KeyRound}>
      {#snippet actions()}
        <SearchInput bind:value={searchQuery} placeholder="Search..." />
        <RefreshButton onclick={loadData} />
      {/snippet}
    </PageHeader>
  </div>

  <!-- Tabs -->
  <div class="px-8">
    <div class="flex items-center gap-1 border-b border-[rgb(var(--color-border-primary))]">
      <button onclick={() => activeTab = 'secrets'} class="px-3 py-2 cursor-pointer text-sm font-medium transition-colors {activeTab === 'secrets' ? 'text-[rgb(var(--color-accent-blue))] border-b-2 border-[rgb(var(--color-accent-blue))]' : 'text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))]'}">
        <KeyRound class="w-3.5 h-3.5 inline-block mr-1.5" />Secrets
        <Badge variant="default" size="xs">{secrets.length}</Badge>
      </button>
      <button onclick={() => activeTab = 'configs'} class="px-3 py-2 cursor-pointer text-sm font-medium transition-colors {activeTab === 'configs' ? 'text-[rgb(var(--color-accent-blue))] border-b-2 border-[rgb(var(--color-accent-blue))]' : 'text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))]'}">
        <FileCode class="w-3.5 h-3.5 inline-block mr-1.5" />Configs
        <Badge variant="default" size="xs">{configs.length}</Badge>
      </button>
    </div>
  </div>

  <div class="flex-1 overflow-auto px-8 py-4 pb-8">
    {#if isLoading}
      <LoadingState message="Loading secrets & configs..." />
    {:else if error}
      <div class="p-8 text-center"><p class="text-sm text-red-400">{error.getUserMessage()}</p></div>
    {:else if activeTab === 'secrets'}
      {#if filteredSecrets.length === 0}
        <EmptyState icon={KeyRound} title="No secrets found" message={searchQuery ? 'No secrets match your search.' : 'No secrets in this swarm.'} />
      {:else}
        <DataTable columns={[
          { key: 'name', label: 'Name' },
          { key: 'id', label: 'ID', width: 'w-40' },
          { key: 'driver', label: 'Driver', width: 'w-28' },
          { key: 'created', label: 'Created', width: 'w-44' },
          { key: 'updated', label: 'Updated', width: 'w-44' },
          { key: 'labels', label: 'Labels', width: 'w-32' },
        ]}>
          {#each filteredSecrets as secret (secret.id)}
            <tr class="hover:bg-[rgb(var(--color-bg-tertiary))]/50">
              <td class="px-4 py-3 text-sm font-medium text-[rgb(var(--color-text-primary))]">
                <div class="flex items-center gap-2"><KeyRound class="w-3.5 h-3.5 text-amber-400 shrink-0" />{secret.name}</div>
              </td>
              <td class="px-4 py-3 text-xs font-mono text-[rgb(var(--color-text-secondary))] truncate">{secret.id.slice(0, 12)}</td>
              <td class="px-4 py-3 text-xs text-[rgb(var(--color-text-secondary))]">{secret.driver || '—'}</td>
              <td class="px-4 py-3 text-xs text-[rgb(var(--color-text-secondary))]">{new Date(secret.createdAt).toLocaleString()}</td>
              <td class="px-4 py-3 text-xs text-[rgb(var(--color-text-secondary))]">{new Date(secret.updatedAt).toLocaleString()}</td>
              <td class="px-4 py-3">
                {#if secret.labels.length > 0}
                  <Badge variant="default" size="xs">{secret.labels.length}</Badge>
                {:else}
                  <span class="text-xs text-[rgb(var(--color-text-secondary))]">—</span>
                {/if}
              </td>
            </tr>
          {/each}
        </DataTable>
      {/if}
    {:else}
      {#if filteredConfigs.length === 0}
        <EmptyState icon={FileCode} title="No configs found" message={searchQuery ? 'No configs match your search.' : 'No configs in this swarm.'} />
      {:else}
        <DataTable columns={[
          { key: 'name', label: 'Name' },
          { key: 'id', label: 'ID', width: 'w-40' },
          { key: 'created', label: 'Created', width: 'w-44' },
          { key: 'updated', label: 'Updated', width: 'w-44' },
          { key: 'labels', label: 'Labels', width: 'w-32' },
        ]}>
          {#each filteredConfigs as config (config.id)}
            <tr class="hover:bg-[rgb(var(--color-bg-tertiary))]/50">
              <td class="px-4 py-3 text-sm font-medium text-[rgb(var(--color-text-primary))]">
                <div class="flex items-center gap-2"><FileCode class="w-3.5 h-3.5 text-blue-400 shrink-0" />{config.name}</div>
              </td>
              <td class="px-4 py-3 text-xs font-mono text-[rgb(var(--color-text-secondary))] truncate">{config.id.slice(0, 12)}</td>
              <td class="px-4 py-3 text-xs text-[rgb(var(--color-text-secondary))]">{new Date(config.createdAt).toLocaleString()}</td>
              <td class="px-4 py-3 text-xs text-[rgb(var(--color-text-secondary))]">{new Date(config.updatedAt).toLocaleString()}</td>
              <td class="px-4 py-3">
                {#if config.labels.length > 0}
                  <Badge variant="default" size="xs">{config.labels.length}</Badge>
                {:else}
                  <span class="text-xs text-[rgb(var(--color-text-secondary))]">—</span>
                {/if}
              </td>
            </tr>
          {/each}
        </DataTable>
      {/if}
    {/if}
  </div>
</div>
