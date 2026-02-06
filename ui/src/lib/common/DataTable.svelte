<script lang="ts">
  export type Column = {
    key: string;
    label: string;
    width?: string;
  };

  interface Props {
    columns: Column[];
    children: any;
  }

  let { columns, children }: Props = $props();
</script>

<div class="bg-[rgb(var(--color-bg-secondary))] rounded-lg border border-[rgb(var(--color-border-primary))] shadow-sm overflow-hidden">
  <table class="w-full">
    <thead class="bg-[rgb(var(--color-bg-secondary))] border-b border-[rgb(var(--color-border-primary))]">
      <tr>
        {#each columns as column}
          <th class="text-left px-4 py-3 text-xs font-semibold text-[rgb(var(--color-text-secondary))] uppercase tracking-wide {column.width || ''}">
            {column.label}
          </th>
        {/each}
      </tr>
    </thead>
    <tbody class="bg-[rgb(var(--color-bg-secondary))] divide-y divide-[rgb(var(--color-border-primary))]">
      {@render children()}
    </tbody>
  </table>
</div>
