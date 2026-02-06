<script lang="ts">
  import { link } from 'svelte-spa-router';

  let {
    items
  }: {
    items: Array<{ label: string; href?: string }>;
  } = $props();
</script>

<nav class="flex items-center gap-2 text-sm text-[rgb(var(--color-text-secondary))]">
  {#each items as item, index}
    {#if index > 0}
      <span>/</span>
    {/if}
    {#if item.href}
      <a 
        use:link 
        href={item.href} 
        class="hover:text-[rgb(var(--color-text-primary))] transition-colors"
      >
        {item.label}
      </a>
    {:else}
      <span class="text-[rgb(var(--color-text-primary))] font-semibold">{item.label}</span>
    {/if}
  {/each}
</nav>
