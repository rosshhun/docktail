<script lang="ts">
  import { getResourceColor } from '../utils/formatting';

  let { 
    label,
    value,
    percentage,
    showValue = true,
    size = 'sm'
  } = $props<{
    label: string;
    value?: string;
    percentage: number;
    showValue?: boolean;
    size?: 'xs' | 'sm' | 'md';
  }>();

  // Use $derived for reactive class computation based on size prop
  const heightClass = $derived(size === 'xs' ? 'h-1' : size === 'sm' ? 'h-1.5' : 'h-2');
  const labelSize = $derived(size === 'xs' ? 'text-[9px]' : 'text-[10px]');
  const valueSize = $derived(size === 'xs' ? 'text-[10px]' : 'text-[11px]');
  
  // Clamp percentage to 0-100 range
  const displayPercentage = $derived(Math.max(0, Math.min(percentage, 100)));
</script>

<div class="space-y-1 group" title="{label}: {value || percentage.toFixed(1) + '%'}">
  <div class="flex items-center justify-between">
    <span class="{labelSize} font-bold text-[rgb(var(--color-text-tertiary))] uppercase tracking-wider">
      {label}
    </span>
    {#if showValue && value}
      <span class="{valueSize} font-mono font-semibold text-[rgb(var(--color-text-primary))] tabular-nums">
        {value}
      </span>
    {/if}
  </div>
  <div class="{heightClass} bg-[rgb(var(--color-bg-tertiary))] rounded-full overflow-hidden shadow-inner">
    <div 
      class="h-full transition-all duration-500 ease-out {getResourceColor(percentage)} shadow-sm"
      style="width: {displayPercentage}%"
    ></div>
  </div>
</div>
