<script lang="ts">
  import Badge from './Badge.svelte';

  interface Props {
    title: string;
    icon?: any;
    badge?: string | number;
    children: any;
  }

  let { title, icon: Icon, badge, children }: Props = $props();
</script>

<div class="bg-[rgb(var(--color-bg-secondary))] rounded-lg border border-[rgb(var(--color-border-primary))] overflow-hidden">
  <div class="px-5 py-3 border-b border-[rgb(var(--color-border-primary))] bg-[rgb(var(--color-bg-tertiary))]/30">
    <h2 class="text-sm font-semibold text-[rgb(var(--color-text-primary))] flex items-center gap-2">
      {#if Icon}
        <Icon class="w-4 h-4" />
      {/if}
      {title}
      {#if badge !== undefined}
        <Badge variant="default" size="xs">
          {badge}
        </Badge>
      {/if}
    </h2>
  </div>
  <div class="p-6">
    {@render children()}
  </div>
</div>
