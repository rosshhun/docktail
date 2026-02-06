<script lang="ts">
  let { 
    variant = 'default' as 'success' | 'warning' | 'error' | 'info' | 'default',
    size = 'sm' as 'xs' | 'sm',
    children
  } = $props();

  const variants = {
    success: 'bg-green-500/10 text-green-400 border-green-500/20',
    warning: 'bg-amber-500/10 text-amber-400 border-amber-500/20',
    error: 'bg-red-500/10 text-red-400 border-red-500/20',
    info: 'bg-blue-500/10 text-blue-400 border-blue-500/20',
    default: 'bg-[rgb(var(--color-bg-tertiary))] text-[rgb(var(--color-text-secondary))] border-[rgb(var(--color-border-primary))]',
  };

  const sizes = {
    xs: 'px-1.5 py-0.5 text-[10px]',
    sm: 'px-2 py-0.5 text-[11px]',
  };
</script>

<span class="inline-flex items-center rounded border font-medium {variants[variant]} {sizes[size]}">
  {@render children()}
</span>
