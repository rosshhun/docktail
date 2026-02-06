<script lang="ts">
  import { RefreshCw } from '@lucide/svelte';

  let {
    onclick,
    disabled = false,
    label = 'Refresh'
  }: {
    onclick: () => void;
    disabled?: boolean;
    label?: string;
  } = $props();
</script>

<button
  {onclick}
  {disabled}
  type="button"
  class="inline-flex items-center gap-2 px-3.5 py-1.5 bg-[rgb(var(--color-bg-secondary))] text-[rgb(var(--color-text-primary))] rounded-md text-sm font-medium leading-6 transition-all duration-150 ease-in-out focus:outline-none focus:ring-0 cursor-pointer appearance-none hover:bg-[rgb(var(--color-bg-tertiary))] disabled:opacity-50 disabled:cursor-not-allowed"
  style="box-shadow: 0px 0px 0px 1px rgb(var(--color-border-secondary)); letter-spacing: -0.28px; border: none; border-bottom: 2px solid rgb(var(--color-border-secondary)); transition: all 0.15s ease-in-out;"
  onmouseenter={(e) => {
    if (!disabled) e.currentTarget.style.borderBottom = "2px solid rgb(var(--color-border-secondary))";
  }}
  onmouseleave={(e) => {
    if (!disabled) e.currentTarget.style.borderBottom = "2px solid rgb(var(--color-border-secondary))";
  }}
>
  <RefreshCw 
    class="shrink-0 text-[rgb(var(--color-text-secondary))] {disabled ? 'animate-spin' : ''}" 
    strokeWidth={1.5}
    size={16}
  />
  <span class="whitespace-nowrap">{label}</span>
</button>
