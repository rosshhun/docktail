<script lang="ts">
  import { Search, X } from '@lucide/svelte';
  import { onMount } from 'svelte';

  let { 
    placeholder = 'Search...',
    value = $bindable(''),
    oninput = undefined,
    autofocus = false
  } = $props();

  let inputElement = $state<HTMLInputElement>();
  let isFocused = $state(false);

  onMount(() => {
    // Global keyboard shortcut handler
    const handleKeydown = (e: KeyboardEvent) => {
      // Check for Cmd+K (Mac) or Ctrl+K (Windows/Linux)
      if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
        e.preventDefault();
        inputElement?.focus();
      }
      // Escape to clear search when focused
      if (e.key === 'Escape' && isFocused) {
        value = '';
        inputElement?.blur();
      }
    };

    window.addEventListener('keydown', handleKeydown);
    
    if (autofocus) {
      inputElement?.focus();
    }

    return () => {
      window.removeEventListener('keydown', handleKeydown);
    };
  });

  function clearSearch() {
    value = '';
    inputElement?.focus();
  }

  // Detect if user is on Mac
  const isMac = typeof navigator !== 'undefined' && navigator.platform.toUpperCase().indexOf('MAC') >= 0;
  const shortcutKey = isMac ? '⌘' : 'Ctrl';
</script>

<div class="relative group">
  <!-- Search Icon -->
  <div class="absolute left-3.5 top-1/2 -translate-y-1/2 pointer-events-none transition-colors duration-150">
    <Search 
      class="w-4 h-4 transition-colors duration-150 {isFocused ? 'text-[rgb(var(--color-text-primary))]' : 'text-[rgb(var(--color-text-secondary))]'}" 
      strokeWidth={1.5} 
    />
  </div>

  <!-- Input Field -->
  <input 
    bind:this={inputElement}
    type="text" 
    {placeholder}
    bind:value
    {oninput}
    onfocus={() => isFocused = true}
    onblur={() => isFocused = false}
    onmouseenter={(e) => {
      if (!isFocused) {
        e.currentTarget.style.borderBottom = '2px solid rgb(var(--color-border-secondary))';
      }
    }}
    onmouseleave={(e) => {
      if (!isFocused) {
        e.currentTarget.style.borderBottom = '2px solid rgb(var(--color-border-secondary))';
      }
    }}
    class="
      w-full pl-10 pr-20 py-1.5
      text-sm font-medium leading-6
      bg-[rgb(var(--color-bg-secondary))]
      rounded-md
      transition-all duration-150 ease-in-out
      placeholder:text-[rgb(var(--color-text-tertiary))] placeholder:font-normal
      text-[rgb(var(--color-text-primary))]
      focus:outline-none focus:ring-0
      cursor-text appearance-none
      {isFocused ? 'bg-[rgb(var(--color-bg-tertiary))]' : 'hover:bg-[rgb(var(--color-bg-tertiary))]'}
    "
    style="box-shadow: 0px 0px 0px 1px rgb(var(--color-border-secondary)); letter-spacing: -0.28px; border: none; border-bottom: 2px solid {isFocused ? 'rgb(var(--color-border-secondary))' : 'rgb(var(--color-border-secondary))'}; transition: all 0.15s ease-in-out;"
  />

  <!-- Right Side: Clear Button & Keyboard Shortcut -->
  <div class="absolute right-2 top-1/2 -translate-y-1/2 flex items-center gap-1.5">
    <!-- Clear Button (shown when there's text) -->
    {#if value}
      <button
        onclick={clearSearch}
        type="button"
        class="
          flex items-center justify-center
          w-5 h-5 rounded
          text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))]
          hover:bg-[rgb(var(--color-border-secondary))]
          transition-all duration-150
          focus:outline-none focus:ring-0
        "
        title="Clear search (Esc)"
      >
        <X class="w-3.5 h-3.5" strokeWidth={2} />
      </button>
    {/if}

    <!-- Keyboard Shortcut Hint (hidden when focused or has value) -->
    {#if !isFocused && !value}
      <div 
        class="
          flex items-center gap-0.5
          px-1.5 py-0.5 
          bg-[rgb(var(--color-bg-secondary))]
          border-2 border-[rgb(var(--color-border-secondary))]
          rounded-sm
          transition-opacity duration-150
          opacity-70 group-hover:opacity-100
        "
      >
        <span class="text-[12px] font-semibold text-[rgb(var(--color-text-secondary))] leading-none">
          {shortcutKey}
        </span>
        <span class="text-[12px] font-semibold text-[rgb(var(--color-text-secondary))] leading-none">K</span>
      </div>
    {/if}
  </div>
</div>
