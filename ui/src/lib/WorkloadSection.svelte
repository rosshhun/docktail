<script lang="ts">
  import { Box, ExternalLink } from '@lucide/svelte';
  import type { ComponentType } from 'svelte';

  interface Resource {
    id: string;
    name: string;
    namespace?: string;
    created?: string;
    size?: string;
    lastEvent?: string;
    lastEventType?: 'normal' | 'error' | 'success'; 
  }

  let { 
    title, 
    count = 0, 
    icon = Box, 
    resources = [] 
  } = $props<{
    title: string;
    count?: number;
    icon?: ComponentType;
    resources?: Resource[];
  }>();
  
  // Derived component reference (capitalized for usage)
  const Icon = $derived(icon);
</script>

<div class="mb-8">
  <div class="flex items-center gap-2 mb-4">
    <Icon class="w-6 h-6 text-blue-500" />
    <h3 class="text-lg font-semibold text-gray-200">{title}</h3>
    <span class="inline-flex items-center justify-center px-2 py-0.5 text-xs font-medium bg-gray-700 text-[rgb(var(--color-text-primary))] rounded">{count}</span>
  </div>

  <div class="overflow-x-auto border border-gray-700 rounded-lg bg-gray-800">
    <table class="min-w-full divide-y divide-[rgb(var(--color-border-primary))]">
      <!-- head -->
      <thead>
        <tr class="bg-gray-800 text-[rgb(var(--color-text-secondary))] border-b border-gray-700">
          <th class="w-10 px-4 py-3 text-left">
            <label>
              <input type="checkbox" class="w-4 h-4 rounded border-gray-600 text-blue-600 focus:ring-blue-500 focus:ring-2 bg-gray-700" />
            </label>
          </th>
          <th class="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider">Name <span class="text-xs opacity-50">▼</span></th>
          <th class="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider">Namespace</th>
          <th class="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider">Created</th>
          <th class="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider">Size</th>
          <th class="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider">Last Event</th>
        </tr>
      </thead>
      <tbody class="divide-y divide-[rgb(var(--color-border-primary))]">
        {#if resources.length === 0}
           <tr>
             <td colspan="6" class="py-8 text-center text-[rgb(var(--color-text-tertiary))]">
               <div class="flex flex-col items-center gap-2">
                 <Box class="w-8 h-8 opacity-20" />
                 <span class="text-sm">No resources found</span>
               </div>
             </td>
           </tr>
        {:else}
            {#each resources as resource}
            <tr class="hover:bg-[rgb(var(--color-bg-tertiary))]/50 transition-colors group">
              <td class="px-4 py-3">
                <label>
                  <input type="checkbox" class="w-4 h-4 rounded border-gray-600 text-blue-600 focus:ring-blue-500 focus:ring-2 bg-gray-700" />
                </label>
              </td>
              <td class="px-4 py-3">
                <div class="font-medium text-[rgb(var(--color-text-primary))]">{resource.name}</div>
              </td>
              <td class="px-4 py-3 text-[rgb(var(--color-text-secondary))]">{resource.namespace}</td>
              <td class="px-4 py-3 text-[rgb(var(--color-text-tertiary))] text-sm">{resource.created}</td>
              <td class="px-4 py-3 text-[rgb(var(--color-text-secondary))] text-sm">{resource.size}</td>
              <td class="px-4 py-3">
                <div class="flex items-center justify-between w-full">
                  {#if resource.lastEvent === 'just now'}
                     <span class="bg-green-500/20 text-green-400 text-xs px-2 py-0.5 rounded flex items-center gap-1 font-medium">
                        just now
                     </span>
                  {:else}
                     <span class="text-[rgb(var(--color-text-secondary))] text-sm">{resource.lastEvent}</span>
                  {/if}
                  
                  <a href="#{resource.name}" class="text-blue-500 text-sm hover:underline flex items-center gap-1 ml-4 opacity-0 group-hover:opacity-100 table-cell-action">
                    view
                    <ExternalLink class="w-3 h-3" />
                  </a>
                </div>
              </td>
            </tr>
            {/each}
        {/if}
      </tbody>
    </table>
  </div>
</div>

<style>
    /* Show action on row hover */
    tr:hover .table-cell-action {
        opacity: 1;
    }
</style>
