<script lang="ts">
  export type Tab = {
    id: string;
    label: string;
    icon?: any;
  };

  interface Props {
    tabs: Tab[];
    activeTab: string;
    onTabChange: (tabId: string) => void;
  }

  let { tabs, activeTab, onTabChange }: Props = $props();
</script>

<div class="flex items-center gap-1">
  {#each tabs as tab}
    <button
      onclick={() => onTabChange(tab.id)}
      class="px-3 py-1.5 cursor-pointer text-sm font-medium transition-colors {activeTab === tab.id ? 'text-[rgb(var(--color-accent-blue))] border-b-2 border-[rgb(var(--color-accent-blue))]' : 'text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))]'}"
    >
      {#if tab.icon}
        <svelte:component this={tab.icon} class="w-3.5 h-3.5 inline-block mr-1.5" />
      {/if}
      {tab.label}
    </button>
  {/each}
</div>
