<script lang="ts">
  let { 
    message = 'Loading...',
    size = 'md' as 'sm' | 'md' | 'lg'
  } = $props();

  const sizes = {
    sm: 'h-6 w-6 border',
    md: 'h-8 w-8 border-b-2',
    lg: 'h-12 w-12 border-b-2'
  };

  const textSizes = {
    sm: 'text-xs',
    md: 'text-sm',
    lg: 'text-base'
  };
</script>

<div class="flex items-center justify-center py-16">
  <div class="text-center">
    <div class="inline-block animate-spin rounded-full {sizes[size]} border-[rgb(var(--color-accent-blue))]"></div>
    {#if message}
      <p class="{textSizes[size]} text-[rgb(var(--color-text-secondary))] mt-3">{message}</p>
    {/if}
  </div>
</div>
