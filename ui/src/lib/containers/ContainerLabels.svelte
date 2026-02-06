<script lang="ts">
  import { Tag } from '@lucide/svelte';

  let { 
    labels,
    compact = false,
    maxVisible = 3
  } = $props<{ 
    labels: Array<{ key: string; value: string }>;
    compact?: boolean;
    maxVisible?: number;
  }>();

  let showAll = $state(false);

  const visibleLabels = $derived(
    showAll ? labels : labels.slice(0, maxVisible)
  );

  const hasMore = $derived(labels.length > maxVisible);

  // Common label categories for color coding
  function getLabelCategory(key: string): 'compose' | 'traefik' | 'custom' {
    if (key.startsWith('com.docker.compose.')) return 'compose';
    if (key.startsWith('traefik.')) return 'traefik';
    return 'custom';
  }

  function getCategoryColor(category: string): string {
    switch (category) {
      case 'compose': return 'bg-blue-50 text-blue-700 border-blue-200';
      case 'traefik': return 'bg-[rgb(var(--color-bg-tertiary))] text-purple-700 border-[rgb(var(--color-border-secondary))]';
      default: return 'bg-[rgb(var(--color-bg-tertiary))] text-[rgb(var(--color-text-secondary))] border-[rgb(var(--color-border-primary))]';
    }
  }
</script>

{#if labels.length > 0}
  <div class="flex flex-wrap gap-2">
    {#each visibleLabels as label}
      {@const category = getLabelCategory(label.key)}
      {@const colorClass = getCategoryColor(category)}
      <div 
        class="inline-flex items-start gap-1.5 px-2.5 py-1.5 rounded-lg border text-[10px] font-medium {colorClass} shadow-sm"
        title="{label.key}={label.value}"
      >
        {#if !compact}
          <Tag class="w-3 h-3 shrink-0 mt-0.5" strokeWidth={2.5} />
        {/if}
        <div class="flex flex-col gap-0.5 min-w-0">
          <span class="font-semibold truncate max-w-[150px]">{label.key}</span>
          {#if !compact && label.value}
            <span class="font-mono text-[9px] opacity-70 truncate max-w-[150px]">{label.value}</span>
          {/if}
        </div>
      </div>
    {/each}
    
    {#if hasMore && !showAll}
      <button 
        onclick={() => showAll = true}
        class="inline-flex items-center px-2.5 py-1.5 rounded-lg border-2 border-dashed border-[rgb(var(--color-border-primary))] text-[10px] font-semibold text-[rgb(var(--color-text-secondary))] hover:bg-[rgb(var(--color-bg-tertiary))] hover:border-gray-400 transition-all"
      >
        +{labels.length - maxVisible} more
      </button>
    {/if}
    
    {#if showAll && hasMore}
      <button 
        onclick={() => showAll = false}
        class="inline-flex items-center px-2.5 py-1.5 rounded-lg border-2 border-dashed border-[rgb(var(--color-border-primary))] text-[10px] font-semibold text-[rgb(var(--color-text-secondary))] hover:bg-[rgb(var(--color-bg-tertiary))] hover:border-gray-400 transition-all"
      >
        Show less
      </button>
    {/if}
  </div>
{:else}
  <span class="text-xs text-[rgb(var(--color-text-tertiary))] italic">No labels</span>
{/if}
