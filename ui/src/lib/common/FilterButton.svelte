<script lang="ts">
  import { ChevronDown } from '@lucide/svelte';
  import { onMount } from 'svelte';

  let { 
    icon = undefined,
    label = '',
    active = false,
    count = undefined,
    showChevron = true,
    onclick = undefined,
    dropdownId = Math.random().toString(36),
    children
  } = $props<{
    icon?: any;
    label: string;
    active?: boolean;
    count?: number;
    showChevron?: boolean;
    onclick?: () => void;
    dropdownId?: string;
    children?: any;
  }>();

  let showDropdown = $state(false);
  let containerRef = $state<HTMLDivElement>();

  function toggleDropdown(e: MouseEvent) {
    e.stopPropagation();
    
    // Close other dropdowns
    if (!showDropdown) {
      window.dispatchEvent(new CustomEvent('closeAllDropdowns', { detail: { except: dropdownId } }));
    }
    
    showDropdown = !showDropdown;
    if (onclick) onclick();
  }

  function handleClickOutside(e: MouseEvent) {
    if (showDropdown && containerRef && !containerRef?.contains(e.target as Node)) {
      showDropdown = false;
    }
  }

  function handleCloseOthers(e: CustomEvent) {
    if (e.detail.except !== dropdownId) {
      showDropdown = false;
    }
  }

  onMount(() => {
    document.addEventListener('click', handleClickOutside);
    window.addEventListener('closeAllDropdowns', handleCloseOthers as EventListener);

    return () => {
      document.removeEventListener('click', handleClickOutside);
      window.removeEventListener('closeAllDropdowns', handleCloseOthers as EventListener);
    };
  });
</script>

<div class="relative inline-block" bind:this={containerRef}>
  <button
    onclick={toggleDropdown}
    type="button"
    class="inline-flex items-center gap-2 px-3.5 py-1.5 bg-[rgb(var(--color-bg-secondary))] text-[rgb(var(--color-text-primary))] rounded-md text-sm font-medium leading-6 transition-all duration-150 ease-in-out focus:outline-none focus:ring-0 cursor-pointer appearance-none hover:bg-[rgb(var(--color-bg-tertiary))]"
    style="box-shadow: 0px 0px 0px 1px rgb(var(--color-border-secondary)); letter-spacing: -0.28px; border: none; border-bottom: 2px solid {active ? 'rgb(var(--color-border-secondary))' : 'rgb(var(--color-border-secondary))'}; transition: all 0.15s ease-in-out;"
    onmouseenter={(e) => { if (!active) e.currentTarget.style.borderBottom = '2px solid rgb(var(--color-border-secondary))'; }}
    onmouseleave={(e) => { if (!active) e.currentTarget.style.borderBottom = '2px solid rgb(var(--color-border-secondary))'; }}
  >
    {#if icon}
      {@const Icon = icon}
      <Icon class="shrink-0 text-[rgb(var(--color-text-secondary))]" strokeWidth={1.5} size={16} />
    {/if}
    
    <span class="flex-1 whitespace-nowrap">{label}</span>
    
    {#if count !== undefined && count > 0}
      <span class="px-1.5 py-0.5 bg-[rgb(var(--color-accent-blue))] text-white rounded text-[10px] font-semibold leading-none">
        {count}
      </span>
    {/if}
    
    {#if showChevron}
      <ChevronDown class="shrink-0 text-[rgb(var(--color-text-secondary))] transition-transform duration-200 {showDropdown ? 'rotate-180' : ''}" strokeWidth={2} size={12} />
    {/if}
  </button>
  
    {#if showDropdown && children}
    <div class="absolute top-full left-0 mt-2 min-w-full bg-[rgb(var(--color-bg-secondary))] rounded-lg shadow-xl border border-[rgb(var(--color-border-secondary))] z-50 py-1 animate-in fade-in slide-in-from-top-2 duration-200">
      {@render children()}
    </div>
  {/if}
</div>
