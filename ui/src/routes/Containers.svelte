<script lang="ts">
  import ContainerListRow from "../lib/containers/ContainerListRow.svelte";
  import SearchInput from "../lib/common/SearchInput.svelte";
  import FilterButton from "../lib/common/FilterButton.svelte";
  import PageHeader from "../lib/common/PageHeader.svelte";
  import RefreshButton from "../lib/common/RefreshButton.svelte";
  import LoadingState from "../lib/common/LoadingState.svelte";
  import ErrorState from "../lib/common/ErrorState.svelte";
  import EmptyState from "../lib/common/EmptyState.svelte";
  import {
    SlidersHorizontal,
    Calendar,
    List,
    Layers,
    Box,
  } from "@lucide/svelte";
  import {
    fetchContainers,
    type ContainerWithAgent,
    GraphQLError,
    groupContainersByCompose,
  } from "../lib/api";
  import { logger } from "../lib/utils/logger";

  let allContainers = $state<ContainerWithAgent[]>([]);
  let isLoading = $state(true);
  let error = $state<GraphQLError | null>(null);
  let searchQuery = $state("");
  let filters = $state<{ agents: string[]; states: string[] }>({
    agents: [],
    states: [],
  });
  let sortBy = $state<"name" | "state" | "created">("name");
  let sortOrder = $state<"asc" | "desc">("asc");
  let viewMode = $state<"list" | "grouped">("list");

  // Use $effect to load containers on mount
  $effect(() => {
    loadContainers();
  });

  async function loadContainers() {
    try {
      isLoading = true;
      error = null;
      allContainers = await fetchContainers();
      logger.debug("[Containers] Loaded containers:", allContainers.length);
    } catch (err: any) {
      if (err instanceof GraphQLError) {
        error = err;
      } else {
        error = new GraphQLError(
          err.message || "Failed to load containers",
          "INTERNAL_SERVER_ERROR",
        );
      }
      logger.error("[Containers] Failed to load:", err);
    } finally {
      isLoading = false;
    }
  }

  async function handleRetry() {
    await loadContainers();
  }

  // Filter and sort containers
  const filteredContainers = $derived(() => {
    let result = allContainers;

    // Filter out infrastructure containers (agent and cluster)
    result = result.filter(c => 
      !c.name.startsWith('docktail-agent') && 
      !c.name.startsWith('docktail-cluster')
    );

    // Search filter
    if (searchQuery) {
      result = result.filter(
        (c) =>
          c.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
          c.image.toLowerCase().includes(searchQuery.toLowerCase()),
      );
    }

    // Agent filter
    if (filters.agents.length > 0) {
      result = result.filter((c) => filters.agents.includes(c.agentId));
    }

    // State filter
    if (filters.states.length > 0) {
      result = result.filter((c) =>
        filters.states.includes(c.state.toLowerCase()),
      );
    }

    // Sorting - create a copy before sorting to avoid mutating state
    result = [...result].sort((a, b) => {
      let comparison = 0;

      if (sortBy === "name") {
        comparison = a.name.localeCompare(b.name);
      } else if (sortBy === "state") {
        comparison = a.state.localeCompare(b.state);
      } else if (sortBy === "created") {
        comparison =
          new Date(a.createdAt).getTime() - new Date(b.createdAt).getTime();
      }

      return sortOrder === "asc" ? comparison : -comparison;
    });

    return result;
  });

  // Get unique agents and states for filter dropdown
  const agents = $derived(() => {
    const agentMap = new Map<
      string,
      { id: string; name: string; count: number }
    >();

    for (const container of allContainers) {
      const existing = agentMap.get(container.agentId);
      if (existing) {
        existing.count++;
      } else {
        agentMap.set(container.agentId, {
          id: container.agentId,
          name: container.agentName,
          count: 1,
        });
      }
    }

    return Array.from(agentMap.values());
  });

  const states = $derived(() => {
    const stateCounts = new Map<string, number>();

    for (const container of allContainers) {
      const state = container.state.toLowerCase();
      stateCounts.set(state, (stateCounts.get(state) || 0) + 1);
    }

    return [
      {
        id: "running",
        name: "Running",
        count: stateCounts.get("running") || 0,
      },
      { id: "exited", name: "Exited", count: stateCounts.get("exited") || 0 },
      {
        id: "created",
        name: "Created",
        count: stateCounts.get("created") || 0,
      },
      { id: "paused", name: "Paused", count: stateCounts.get("paused") || 0 },
    ].filter((s) => s.count > 0);
  });

  function toggleAgent(agentId: string) {
    if (filters.agents.includes(agentId)) {
      filters.agents = filters.agents.filter((id: string) => id !== agentId);
    } else {
      filters.agents = [...filters.agents, agentId];
    }
  }

  function toggleState(stateId: string) {
    if (filters.states.includes(stateId)) {
      filters.states = filters.states.filter((id: string) => id !== stateId);
    } else {
      filters.states = [...filters.states, stateId];
    }
  }

  // Group containers by compose project
  const groupedContainers = $derived(() => {
    if (viewMode === "list") return null;
    return groupContainersByCompose(filteredContainers());
  });

  const composeProjectCount = $derived(() => {
    if (!groupedContainers()) return 0;
    const groups = groupedContainers();
    return groups ? groups.size - (groups.has("__ungrouped__") ? 1 : 0) : 0;
  });
</script>

<div class="flex flex-col h-full bg-[rgb(var(--color-bg-primary))]">
  <!-- Page Header -->
  <PageHeader title="Containers">
    <!-- Search and Controls -->
    <div class="flex items-center gap-3 flex-wrap">
      <div class="flex-1 max-w-md">
        <SearchInput
          placeholder="Search containers..."
          bind:value={searchQuery}
        />
      </div>

      <!-- View Mode Toggle -->
      <div
        class="flex items-center gap-1 bg-[rgb(var(--color-bg-secondary))] border-b-2 border-[rgb(var(--color-border-secondary))] rounded-lg border p-1"
      >
        <button
          onclick={() => (viewMode = "list")}
          class="inline-flex items-center gap-1.5 px-3 py-1.5 rounded text-xs font-medium transition-all cursor-pointer focus:outline-none focus:ring-0 {viewMode ===
          'list'
            ? 'bg-[rgb(var(--color-bg-tertiary))] text-[rgb(var(--color-text-primary))] shadow-sm border-b-2 border-[rgb(var(--color-border-secondary))]'
            : 'text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))] hover:bg-[rgb(var(--color-bg-tertiary))]'}"
          title="List view"
        >
          <List class="w-3.5 h-3.5" strokeWidth={2} />
          List
        </button>
        <button
          onclick={() => (viewMode = "grouped")}
          class="inline-flex items-center gap-1.5 px-3 py-1.5 rounded text-xs font-medium transition-all cursor-pointer focus:outline-none focus:ring-0 {viewMode ===
          'grouped'
            ? 'bg-[rgb(var(--color-bg-tertiary))] text-[rgb(var(--color-text-primary))] shadow-sm border-b-2 border-[rgb(var(--color-border-secondary))]'
            : 'text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))] hover:bg-[rgb(var(--color-bg-tertiary))]'}"
          title="Grouped by Compose project"
        >
          <Layers class="w-3.5 h-3.5" strokeWidth={2} />
          Group
        </button>
      </div>

      <FilterButton
        icon={SlidersHorizontal}
        label="Filters"
        active={filters.agents.length > 0 || filters.states.length > 0}
        count={filters.agents.length + filters.states.length}
        dropdownId="filters-dropdown"
      >
        <div class="min-w-[220px] max-h-[450px] overflow-y-auto">
          <!-- Agent Filters -->
          <div class="py-3">
            <div class="px-3 mb-2">
              <h4
                class="text-[10px] font-bold text-[rgb(var(--color-text-tertiary))] uppercase tracking-wider"
              >
                Agent
              </h4>
            </div>
            <div class="space-y-0.5 px-2">
              {#each agents() as agent}
                <label
                  class="group flex items-center gap-2.5 cursor-pointer hover:bg-[rgb(var(--color-bg-secondary))] px-2.5 py-2 rounded-lg transition-all duration-150"
                >
                  <input
                    type="checkbox"
                    checked={filters.agents.includes(agent.id)}
                    onchange={() => toggleAgent(agent.id)}
                    class="w-4 h-4 rounded border-2 border-[rgb(var(--color-border-secondary))] bg-[rgb(var(--color-bg-primary))] text-[rgb(var(--color-accent-blue))] focus:ring-0 focus:ring-offset-0 cursor-pointer transition-all hover:border-[rgb(var(--color-accent-blue))]"
                  />
                  <span
                    class="text-xs text-[rgb(var(--color-text-primary))] flex-1 font-medium group-hover:text-[rgb(var(--color-text-primary))] transition-colors truncate"
                    >{agent.name}</span
                  >
                  <span
                    class="text-[10px] text-[rgb(var(--color-text-secondary))] font-bold bg-[rgb(var(--color-bg-tertiary))] px-2 py-0.5 rounded-full min-w-[24px] text-center"
                    >{agent.count}</span
                  >
                </label>
              {/each}
            </div>
          </div>

          <!-- State Filters -->
          <div
            class="py-3 border-t-2 border-[rgb(var(--color-border-primary))]"
          >
            <div class="px-3 mb-2">
              <h4
                class="text-[10px] font-bold text-[rgb(var(--color-text-tertiary))] uppercase tracking-wider"
              >
                State
              </h4>
            </div>
            <div class="space-y-0.5 px-2">
              {#each states() as state}
                <label
                  class="group flex items-center gap-2.5 cursor-pointer hover:bg-[rgb(var(--color-bg-secondary))] px-2.5 py-2 rounded-lg transition-all duration-150"
                >
                  <input
                    type="checkbox"
                    checked={filters.states.includes(state.id)}
                    onchange={() => toggleState(state.id)}
                    class="w-4 h-4 rounded border-2 border-[rgb(var(--color-border-secondary))] bg-[rgb(var(--color-bg-primary))] text-[rgb(var(--color-accent-blue))] focus:ring-0 focus:ring-offset-0 cursor-pointer transition-all hover:border-[rgb(var(--color-accent-blue))]"
                  />
                  <span
                    class="text-xs text-[rgb(var(--color-text-primary))] flex-1 font-medium group-hover:text-[rgb(var(--color-text-primary))] transition-colors"
                    >{state.name}</span
                  >
                  <span
                    class="text-[10px] text-[rgb(var(--color-text-secondary))] font-bold bg-[rgb(var(--color-bg-tertiary))] px-2 py-0.5 rounded-full min-w-[24px] text-center"
                    >{state.count}</span
                  >
                </label>
              {/each}
            </div>
          </div>

          <!-- Clear Filters -->
          <div
            class="py-3 px-2 border-t-2 border-[rgb(var(--color-border-primary))]"
          >
            <button
              onclick={() => (filters = { agents: [], states: [] })}
              disabled={filters.agents.length === 0 &&
                filters.states.length === 0}
              class="w-full text-xs font-semibold transition-all duration-150 px-3 py-2.5 rounded-lg border-2
                {filters.agents.length > 0 || filters.states.length > 0
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
        label="Sort: {sortBy === 'name'
          ? 'Name'
          : sortBy === 'state'
            ? 'Status'
            : 'Created'}"
        dropdownId="sort-dropdown"
      >
        <div class="min-w-[200px]">
          <!-- Sort By Section -->
          <div class="py-3">
            <div class="px-3 mb-2">
              <h4
                class="text-[10px] font-bold text-[rgb(var(--color-text-tertiary))] uppercase tracking-wider"
              >
                Sort By
              </h4>
            </div>
            <div class="space-y-0.5 px-2">
              <button
                onclick={() => (sortBy = "name")}
                class="group w-full px-2.5 py-2 text-left text-xs hover:bg-[rgb(var(--color-bg-secondary))] text-[rgb(var(--color-text-primary))] font-medium transition-all duration-150 rounded-lg {sortBy ===
                'name'
                  ? 'bg-[rgb(var(--color-bg-tertiary))] text-[rgb(var(--color-text-primary))] border-2 border-[rgb(var(--color-border-secondary))]'
                  : 'border-2 border-transparent'}"
              >
                <span class="flex items-center gap-2">
                  {#if sortBy === "name"}
                    <span
                      class="w-2 h-2 rounded-full bg-[rgb(var(--color-accent-blue))]"
                    ></span>
                  {:else}
                    <span
                      class="w-2 h-2 rounded-full border-2 border-[#D4D2D7] group-hover:border-[rgb(var(--color-accent-blue))]"
                    ></span>
                  {/if}
                  Name
                </span>
              </button>
              <button
                onclick={() => (sortBy = "state")}
                class="group w-full px-2.5 py-2 text-left text-xs hover:bg-[rgb(var(--color-bg-secondary))] text-[rgb(var(--color-text-primary))] font-medium transition-all duration-150 rounded-lg {sortBy ===
                'state'
                  ? 'bg-[rgb(var(--color-bg-tertiary))] text-[rgb(var(--color-accent-blue))] border-2 border-[rgb(var(--color-border-secondary))]'
                  : 'border-2 border-transparent'}"
              >
                <span class="flex items-center gap-2">
                  {#if sortBy === "state"}
                    <span
                      class="w-2 h-2 rounded-full bg-[rgb(var(--color-accent-blue))]"
                    ></span>
                  {:else}
                    <span
                      class="w-2 h-2 rounded-full border-2 border-[#D4D2D7] group-hover:border-[rgb(var(--color-accent-blue))]"
                    ></span>
                  {/if}
                  Status
                </span>
              </button>
              <button
                onclick={() => (sortBy = "created")}
                class="group w-full px-2.5 py-2 text-left text-xs hover:bg-[rgb(var(--color-bg-secondary))] text-[rgb(var(--color-text-primary))] font-medium transition-all duration-150 rounded-lg {sortBy ===
                'created'
                  ? 'bg-[rgb(var(--color-bg-tertiary))] text-[rgb(var(--color-accent-blue))] border-2 border-[rgb(var(--color-border-secondary))]'
                  : 'border-2 border-transparent'}"
              >
                <span class="flex items-center gap-2">
                  {#if sortBy === "created"}
                    <span
                      class="w-2 h-2 rounded-full bg-[rgb(var(--color-accent-blue))]"
                    ></span>
                  {:else}
                    <span
                      class="w-2 h-2 rounded-full border-2 border-[#D4D2D7] group-hover:border-[rgb(var(--color-accent-blue))]"
                    ></span>
                  {/if}
                  Created
                </span>
              </button>
            </div>
          </div>

          <!-- Sort Order Section -->
          <div
            class="py-3 px-2 border-t-2 border-[rgb(var(--color-border-primary))]"
          >
            <div class="px-1 mb-2">
              <h4
                class="text-[10px] font-bold text-[rgb(var(--color-text-tertiary))] uppercase tracking-wider"
              >
                Order
              </h4>
            </div>
            <button
              onclick={() => (sortOrder = sortOrder === "asc" ? "desc" : "asc")}
              class="w-full px-2.5 py-2 text-left text-xs bg-[rgb(var(--color-bg-tertiary))] hover:bg-purple-100 text-[rgb(var(--color-accent-blue))] font-medium transition-all duration-150 rounded-lg border-2 border-[rgb(var(--color-border-secondary))]"
            >
              <span class="flex items-center gap-2">
                <span class="text-sm font-bold"
                  >{sortOrder === "asc" ? "↑" : "↓"}</span
                >
                {sortOrder === "asc" ? "Ascending" : "Descending"}
              </span>
            </button>
          </div>
        </div>
      </FilterButton>

      <RefreshButton onclick={loadContainers} disabled={isLoading} />
    </div>
  </PageHeader>

  <!-- Container Table -->
  <div class="flex-1 overflow-auto px-8 py-4">
    {#if isLoading}
      <LoadingState message="Loading containers..." />
    {:else if error}
      <ErrorState error={error} onRetry={handleRetry} title="Failed to load containers" />
    {:else if filteredContainers().length === 0}
      <EmptyState 
        icon={Box}
        title="No containers found"
        message={searchQuery || filters.agents.length > 0 || filters.states.length > 0
          ? "Try adjusting your search or filters"
          : "No containers are currently available"}
      />
    {:else}
      <!-- List View -->
      {#if viewMode === "list"}
        <div
          class="bg-[rgb(var(--color-bg-secondary))] rounded-lg border border-[rgb(var(--color-border-primary))] shadow-sm overflow-hidden"
        >
          <table class="w-full">
            <thead
              class="bg-[rgb(var(--color-bg-secondary))] border-b border-[rgb(var(--color-border-primary))]"
            >
              <tr>
                <th
                  class="text-left px-4 py-3 text-xs font-semibold text-[rgb(var(--color-text-secondary))] uppercase tracking-wide w-16"
                >
                  Status
                </th>
                <th
                  class="text-left px-4 py-3 text-xs font-semibold text-[rgb(var(--color-text-secondary))] uppercase tracking-wide"
                >
                  Name
                </th>
                <th
                  class="text-left px-4 py-3 text-xs font-semibold text-[rgb(var(--color-text-secondary))] uppercase tracking-wide w-40"
                >
                  Resources
                </th>
                <th
                  class="text-left px-4 py-3 text-xs font-semibold text-[rgb(var(--color-text-secondary))] uppercase tracking-wide w-32"
                >
                  Ports
                </th>
                <th
                  class="text-left px-4 py-3 text-xs font-semibold text-[rgb(var(--color-text-secondary))] uppercase tracking-wide w-28"
                >
                  Uptime
                </th>
                <th
                  class="text-left px-4 py-3 text-xs font-semibold text-[rgb(var(--color-text-secondary))] uppercase tracking-wide w-40"
                >
                  Agent
                </th>
                <th
                  class="text-left px-4 py-3 text-xs font-semibold text-[rgb(var(--color-text-secondary))] uppercase tracking-wide w-24"
                >
                  Actions
                </th>
              </tr>
            </thead>
            <tbody
              class="bg-[rgb(var(--color-bg-secondary))] divide-y divide-[rgb(var(--color-border-primary))]"
            >
              {#each filteredContainers() as container (container.id)}
                <ContainerListRow {container} />
              {/each}
            </tbody>
          </table>
        </div>
      {:else}
        <!-- Grouped View by Docker Compose Project -->
        <div class="space-y-4">
          {#if groupedContainers()}
            {#each Array.from(groupedContainers() || new Map()) as [projectName, containers]}
              {@const isUngrouped = projectName === "__ungrouped__"}
              <div
                class="border border-[rgb(var(--color-border-primary))] rounded-lg overflow-hidden bg-[rgb(var(--color-bg-secondary))] shadow-sm"
              >
                <!-- Group Header -->
                <div
                  class="bg-[rgb(var(--color-bg-secondary))] px-4 py-3 border-b border-[rgb(var(--color-border-primary))]"
                >
                  <div class="flex items-center justify-between">
                    <div class="flex items-center gap-3">
                      {#if !isUngrouped}
                        <div
                          class="w-10 h-10 rounded-lg bg-blue-100 flex items-center justify-center"
                        >
                          <Layers
                            class="w-5 h-5 text-blue-600"
                            strokeWidth={2}
                          />
                        </div>
                      {/if}
                      <div>
                        <h3
                          class="text-sm font-semibold text-[rgb(var(--color-text-primary))]"
                        >
                          {isUngrouped ? "Standalone Containers" : projectName}
                        </h3>
                        <p
                          class="text-xs text-[rgb(var(--color-text-secondary))] mt-0.5"
                        >
                          {containers.length} container{containers.length !== 1
                            ? "s"
                            : ""}
                          {#if !isUngrouped}
                            · Docker Compose Project
                          {/if}
                        </p>
                      </div>
                    </div>
                    <div class="flex items-center gap-2">
                      {#if !isUngrouped}
                        <span
                          class="px-2 py-1 bg-blue-50 text-blue-700 rounded text-xs font-medium border border-blue-200"
                        >
                          Compose
                        </span>
                      {/if}
                      <span
                        class="text-xs text-[rgb(var(--color-text-secondary))]"
                      >
                        {containers.filter(
                          (c: ContainerWithAgent) => c.state === "RUNNING",
                        ).length} running
                      </span>
                    </div>
                  </div>
                </div>

                <!-- Group Containers Table -->
                <table class="w-full">
                  <tbody
                    class="bg-[rgb(var(--color-bg-secondary))] divide-y divide-[rgb(var(--color-border-primary))]"
                  >
                    {#each containers as container (container.id)}
                      <ContainerListRow {container} />
                    {/each}
                  </tbody>
                </table>
              </div>
            {/each}
          {/if}
        </div>
      {/if}
    {/if}
  </div>
</div>
