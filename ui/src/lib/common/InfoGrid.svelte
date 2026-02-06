<script lang="ts">
  export type InfoItem = {
    label: string;
    value: string | number;
    variant?: 'default' | 'code' | 'badge';
    badge?: any;
  };

  interface Props {
    items: InfoItem[];
    columns?: 1 | 2 | 3;
  }

  let { items, columns = 2 }: Props = $props();

  const colClasses = {
    1: 'grid-cols-1',
    2: 'grid-cols-1 md:grid-cols-2',
    3: 'grid-cols-1 md:grid-cols-3'
  };
</script>

<dl class="grid {colClasses[columns]} gap-4">
  {#each items as item}
    <div class="bg-[rgb(var(--color-bg-tertiary))]/20 rounded-md p-3 border border-[rgb(var(--color-border-primary))]/50">
      <dt class="text-[10px] font-bold text-[rgb(var(--color-text-secondary))] uppercase tracking-wider mb-2">
        {item.label}
      </dt>
      <dd class="text-xs {item.variant === 'code' ? 'font-mono' : ''} text-[rgb(var(--color-text-primary))] {item.variant === 'code' ? 'break-all' : ''}">
        {#if item.badge}
          {@render item.badge()}
        {:else}
          {item.value}
        {/if}
      </dd>
    </div>
  {/each}
</dl>
