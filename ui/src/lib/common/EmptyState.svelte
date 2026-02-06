<script lang="ts">
  interface Props {
    icon: any;
    title: string;
    message?: string;
    children?: any;
  }
  
  let { icon: Icon, title, message, children }: Props = $props();
</script>

<div class="flex items-center justify-center py-16">
  <div class="text-center">
    <div class="inline-flex items-center justify-center w-12 h-12 rounded-full bg-[rgb(var(--color-bg-tertiary))] mb-3">
      <Icon class="w-6 h-6 text-[rgb(var(--color-text-secondary))]" strokeWidth={2} />
    </div>
    <p class="text-sm font-medium text-[rgb(var(--color-text-primary))] mb-1">{title}</p>
    {#if message}
      <p class="text-xs text-[rgb(var(--color-text-secondary))]">{message}</p>
    {/if}
    {#if children}
      <div class="mt-4">
        {@render children()}
      </div>
    {/if}
  </div>
</div>
