<script lang="ts">
  type LogEntry = {
    timestamp: string;
    level: 'info' | 'warn' | 'error' | 'debug';
    message: string;
  };

  let { log } = $props<{ log: LogEntry }>();

  const colors = {
    info: 'text-blue-400',
    warn: 'text-amber-400 bg-amber-500/10',
    error: 'text-red-400 bg-red-500/10',
    debug: 'text-[rgb(var(--color-text-tertiary))]',
  };

  const badges = {
    info: 'bg-blue-500/20 text-blue-400',
    warn: 'bg-amber-500/20 text-amber-400',
    error: 'bg-red-500/20 text-red-400',
    debug: 'bg-[rgb(var(--color-bg-tertiary))]0/20 text-[rgb(var(--color-text-secondary))]',
  };
</script>

<div class="flex gap-3 px-4 py-2 hover:bg-gray-800/50 font-mono text-sm {colors[log.level]}">
  <span class="text-[rgb(var(--color-text-tertiary))] text-xs select-none shrink-0">{log.timestamp}</span>
  <span class="px-2 py-0.5 rounded text-xs font-medium uppercase shrink-0 {badges[log.level]}">{log.level}</span>
  <span class="flex-1">{log.message}</span>
</div>
