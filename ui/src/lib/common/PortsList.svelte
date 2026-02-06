<script lang="ts">
  import type { PortMapping } from '../api';

  let { 
    ports,
    maxDisplay = 3
  } = $props<{
    ports?: PortMapping[];
    maxDisplay?: number;
  }>();

  // Format and deduplicate port mappings for display
  const portStrings = $derived.by(() => {
    const formatted = (ports || []).map((p: PortMapping) => {
      if (p.hostPort && p.hostIp) {
        // For mapped ports, prefer IPv4 and omit 0.0.0.0 for brevity
        // Skip IPv6 (:::) if we already have the same port mapping
        const isIpv6 = p.hostIp.includes(':');
        const ip = p.hostIp === '0.0.0.0' ? '' : `${p.hostIp}:`;
        return {
          display: `${ip}${p.hostPort}:${p.containerPort}`,
          key: `${p.hostPort}:${p.containerPort}`,
          isIpv6
        };
      } else {
        // Exposed only: "80"
        return {
          display: `${p.containerPort}`,
          key: `${p.containerPort}`,
          isIpv6: false
        };
      }
    });
    
    // Deduplicate: prefer IPv4 over IPv6 for the same port mapping
    const seen = new Map<string, { display: string; isIpv6: boolean }>();
    for (const item of formatted) {
      const existing = seen.get(item.key);
      if (!existing || (existing.isIpv6 && !item.isIpv6)) {
        // Keep this one if: 1) it's new, or 2) we had IPv6 but this is IPv4
        seen.set(item.key, item);
      }
    }
    
    return Array.from(seen.values()).map(item => item.display);
  });

  const displayPorts = $derived(portStrings.slice(0, maxDisplay));
  const remainingCount = $derived(portStrings.length - displayPorts.length);
</script>

{#if portStrings.length > 0}
  <div class="text-xs text-[rgb(var(--color-text-secondary))]">
    {#if displayPorts.length > 0}
      <span class="font-mono">{displayPorts.join(' ')}</span>
      {#if remainingCount > 0}
        <span class="text-[rgb(var(--color-text-tertiary))]"> +{remainingCount}</span>
      {/if}
    {:else}
      <span class="text-[rgb(var(--color-text-tertiary))]">{portStrings.length} exposed</span>
    {/if}
  </div>
{:else}
  <span class="text-xs text-[rgb(var(--color-text-tertiary))]">-</span>
{/if}
