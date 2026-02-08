<script lang="ts">
  import { location } from "svelte-spa-router";
  import { Server, Box, Settings, Network, Layers, GitBranch, KeyRound } from "@lucide/svelte";

  // Helper function to check if a route is active
  function isRouteActive(path: string, exact = false): boolean {
    if (exact) {
      return $location === path;
    }

    // Get container name from localStorage if viewing a container details page
    const containerName =
      typeof window !== "undefined" && $location.startsWith("/containers/")
        ? localStorage.getItem("currentContainerName") || ""
        : "";

    // Special handling for cluster containers viewed from container details page
    if (path === "/cluster") {
      // If viewing cluster page or a cluster container details page
      if ($location === "/cluster") return true;
      // Check if we're viewing a cluster container
      if (containerName && containerName.startsWith("docktail-cluster")) {
        return true;
      }
      return false;
    }

    // Special handling for agents - include agent container details pages
    if (path === "/agents") {
      if ($location === "/agents" || $location.startsWith("/agents/"))
        return true;
      // Check if we're viewing an agent container
      if (containerName && containerName.startsWith("docktail-agent")) {
        return true;
      }
      return false;
    }

    // Special handling for swarm routes
    if (path === "/swarm") {
      return $location === "/swarm" || $location.startsWith("/swarm/");
    }

    // For non-exact matches, check if location starts with the path
    // Special case for root/containers
    if (path === "/containers" || path === "/") {
      if ($location === "/" || $location === "/containers") return true;
      if ($location.startsWith("/containers/")) {
        // Only highlight containers if it's NOT a cluster or agent container
        return (
          !containerName.startsWith("docktail-cluster") &&
          !containerName.startsWith("docktail-agent")
        );
      }
      return false;
    }

    return $location === path || $location.startsWith(path + "/");
  }

  let swarmExpanded = $state(false);

  // Auto-expand swarm section when on a swarm route
  $effect(() => {
    if ($location.startsWith('/swarm')) {
      swarmExpanded = true;
    }
  });
</script>

<aside
  class="w-19 border-r border-[rgb(var(--color-border-primary))] bg-[rgb(var(--color-sidebar))] flex flex-col"
>
  <!-- Logo Section -->
  <div class="py-4 flex items-center justify-center">
    <div class="w-8 h-8 flex items-center justify-center">
      <img src="/logo.svg" alt="Docktail" class="w-8 h-8" />
    </div>
  </div>

  <!-- Navigation -->
  <nav class="flex-1 py-2">
    <ul class="space-y-0">
      <li>
        <a
          href="#/containers"
          class="group flex flex-col items-center gap-1 px-2 py-2.5 text-xs font-medium transition-all duration-200 relative {isRouteActive(
            '/containers',
          )
            ? 'text-[rgb(var(--color-text-primary))]'
            : 'text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))]'}"
        >
          <div
            class="w-10 h-10 rounded-md flex items-center justify-center transition-colors {isRouteActive(
              '/containers',
            )
              ? 'bg-[rgb(var(--color-bg-tertiary))]'
              : 'hover:bg-[rgb(var(--color-bg-tertiary))]'}"
          >
            <Box class="w-[22px] h-[24px]" strokeWidth={2} />
          </div>
          <span class="text-[10px] font-semibold mt-0.5">Containers</span>
        </a>
      </li>
      <li>
        <a
          href="#/agents"
          class="group flex flex-col items-center gap-1 px-2 py-2.5 text-xs font-medium transition-all duration-200 relative {isRouteActive(
            '/agents',
          )
            ? 'text-[rgb(var(--color-text-primary))]'
            : 'text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))]'}"
        >
          <div
            class="w-10 h-10 rounded-md flex items-center justify-center transition-colors {isRouteActive(
              '/agents',
            )
              ? 'bg-[rgb(var(--color-bg-tertiary))]'
              : 'hover:bg-[rgb(var(--color-bg-tertiary))]'}"
          >
            <Server class="w-[21px] h-[24px]" strokeWidth={2} />
          </div>
          <span class="text-[10px] font-semibold mt-0.5">Agents</span>
        </a>
      </li>
      <li>
        <a
          href="#/cluster"
          class="group flex flex-col items-center gap-1 px-2 py-2.5 text-xs font-medium transition-all duration-200 relative {isRouteActive(
            '/cluster',
          )
            ? 'text-[rgb(var(--color-text-primary))]'
            : 'text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))]'}"
        >
          <div
            class="w-10 h-10 rounded-md flex items-center justify-center transition-colors {isRouteActive(
              '/cluster',
            )
              ? 'bg-[rgb(var(--color-bg-tertiary))]'
              : 'hover:bg-[rgb(var(--color-bg-tertiary))]'}"
          >
            <Network class="w-[21px] h-[24px]" strokeWidth={2} />
          </div>
          <span class="text-[10px] font-semibold mt-0.5">Cluster</span>
        </a>
      </li>
      <li>
        <button
          onclick={() => swarmExpanded = !swarmExpanded}
          class="w-full group flex flex-col items-center gap-1 px-2 py-2.5 text-xs font-medium transition-all duration-200 relative cursor-pointer {isRouteActive(
            '/swarm',
          )
            ? 'text-[rgb(var(--color-text-primary))]'
            : 'text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))]'}"
        >
          <div
            class="w-10 h-10 rounded-md flex items-center justify-center transition-colors {isRouteActive(
              '/swarm',
            )
              ? 'bg-[rgb(var(--color-bg-tertiary))]'
              : 'hover:bg-[rgb(var(--color-bg-tertiary))]'}"
          >
            <Layers class="w-[21px] h-[24px]" strokeWidth={2} />
          </div>
          <span class="text-[10px] font-semibold mt-0.5">Swarm</span>
        </button>
        {#if swarmExpanded}
          <div class="flex flex-col items-center gap-0.5 pb-1">
            <a href="#/swarm" class="text-[9px] font-medium py-1 px-2 rounded transition-colors {$location === '/swarm' ? 'text-[rgb(var(--color-accent-blue))]' : 'text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))]'}">Overview</a>
            <a href="#/swarm/services" class="text-[9px] font-medium py-1 px-2 rounded transition-colors {$location.startsWith('/swarm/services') ? 'text-[rgb(var(--color-accent-blue))]' : 'text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))]'}">Services</a>
            <a href="#/swarm/nodes" class="text-[9px] font-medium py-1 px-2 rounded transition-colors {$location.startsWith('/swarm/nodes') ? 'text-[rgb(var(--color-accent-blue))]' : 'text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))]'}">Nodes</a>
            <a href="#/swarm/stacks" class="text-[9px] font-medium py-1 px-2 rounded transition-colors {$location.startsWith('/swarm/stacks') ? 'text-[rgb(var(--color-accent-blue))]' : 'text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))]'}">Stacks</a>
            <a href="#/swarm/networks" class="text-[9px] font-medium py-1 px-2 rounded transition-colors {$location === '/swarm/networks' ? 'text-[rgb(var(--color-accent-blue))]' : 'text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))]'}">Networks</a>
            <a href="#/swarm/secrets" class="text-[9px] font-medium py-1 px-2 rounded transition-colors {$location === '/swarm/secrets' ? 'text-[rgb(var(--color-accent-blue))]' : 'text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))]'}">Secrets</a>
          </div>
        {/if}
      </li>

      <!-- Separator -->
      <li class="px-2 py-2">
        <div class="border-t border-[rgb(var(--color-border-primary))]"></div>
      </li>

      <li>
        <a
          href="#/settings"
          class="group flex flex-col items-center gap-1 px-2 py-2.5 text-xs font-medium transition-all duration-200 relative {isRouteActive(
            '/settings',
            true,
          )
            ? 'text-[rgb(var(--color-text-primary))]'
            : 'text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))]'}"
        >
          <div
            class="w-10 h-10 rounded-md flex items-center justify-center transition-colors {isRouteActive(
              '/settings',
              true,
            )
              ? 'bg-[rgb(var(--color-bg-tertiary))]'
              : 'hover:bg-[rgb(var(--color-bg-tertiary))]'}"
          >
            <Settings class="w-[21px] h-[24px]" strokeWidth={2} />
          </div>
          <span class="text-[10px] font-semibold mt-0.5">Settings</span>
        </a>
      </li>
    </ul>
  </nav>
</aside>
