<script lang="ts">
  import Breadcrumbs from './Breadcrumbs.svelte';
  
  interface Props {
    title: string;
    subtitle?: string;
    breadcrumbs?: Array<{ label: string; href?: string }>;
    actions?: any;
    children?: any;
  }
  
  let { title, subtitle, breadcrumbs, actions, children }: Props = $props();
</script>

<div class="px-8 pt-6 pb-5 border-b border-[rgb(var(--color-border-primary))]">
  <div class="flex items-center justify-between mb-5">
    <div>
      {#if breadcrumbs && breadcrumbs.length > 0}
        <div class="mb-2">
          <Breadcrumbs items={breadcrumbs} />
        </div>
      {/if}
      <h1 class="text-2xl font-bold text-[rgb(var(--color-text-primary))]">{title}</h1>
      {#if subtitle}
        <p class="text-sm text-[rgb(var(--color-text-secondary))] mt-1">{subtitle}</p>
      {/if}
    </div>
    {#if actions}
      <div class="flex items-center gap-2">
        {@render actions()}
      </div>
    {/if}
  </div>
  {#if children}
    {@render children()}
  {/if}
</div>
