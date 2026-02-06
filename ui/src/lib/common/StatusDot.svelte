<script lang="ts">
  let { 
    status = 'running' as 'running' | 'stopped' | 'error' | 'warning' | 'healthy' | 'unhealthy' | 'degraded',
    animated = false,
    size = 'sm' as 'sm' | 'md'
  } = $props();

  const colors = {
    running: 'bg-green-500',
    healthy: 'bg-green-500',
    stopped: 'bg-red-500',
    exited: 'bg-red-500',
    dead: 'bg-red-500',
    error: 'bg-red-500',
    unhealthy: 'bg-red-500',
    warning: 'bg-amber-500',
    degraded: 'bg-amber-500',
    paused: 'bg-amber-500',
    restarting: 'bg-amber-500',
    created: 'bg-[rgb(var(--color-text-tertiary))]',
  };

  const sizes = {
    sm: 'w-2 h-2',
    md: 'w-2.5 h-2.5',
  };
</script>

<span class="inline-block rounded-full {sizes[size]} {colors[status]} {animated ? 'animate-pulse' : ''}"></span>
