<script lang="ts">
  import { Moon, Sun } from '@lucide/svelte';
  import { onMount } from 'svelte';

  let isDark = $state(true);

  onMount(() => {
    const savedTheme = localStorage.getItem("theme");
    if (savedTheme === "light") {
      isDark = false;
      document.documentElement.setAttribute("data-theme", "light");
    } else {
      isDark = true;
      document.documentElement.setAttribute("data-theme", "dark");
    }
  });

  function toggleTheme() {
    isDark = !isDark;
    if (isDark) {
      document.documentElement.setAttribute("data-theme", "dark");
      localStorage.setItem("theme", "dark");
    } else {
      document.documentElement.setAttribute("data-theme", "light");
      localStorage.setItem("theme", "light");
    }
  }
</script>

<button 
  onclick={toggleTheme}
  class="flex items-center gap-2 px-4 py-2 text-sm rounded-lg bg-[rgb(var(--color-bg-secondary))] hover:bg-[rgb(var(--color-bg-tertiary))] text-[rgb(var(--color-text-primary))] border border-[rgb(var(--color-border-primary))] transition-colors w-full"
>
  {#if isDark}
    <Sun class="w-5 h-5" />
    <span>Light Mode</span>
  {:else}
    <Moon class="w-5 h-5" />
    <span>Dark Mode</span>
  {/if}
</button>
