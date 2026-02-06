<script lang="ts">
  let { 
    variant = 'default' as 'default' | 'primary' | 'secondary' | 'danger' | 'success' | 'ghost',
    size = 'md' as 'xs' | 'sm' | 'md' | 'lg',
    disabled = false,
    type = 'button' as 'button' | 'submit' | 'reset',
    onclick = undefined,
    children
  } = $props();

  const variants = {
    default: 'bg-[rgb(var(--color-bg-secondary))] hover:bg-[rgb(var(--color-bg-tertiary))] text-[rgb(var(--color-text-primary))] border border-[rgb(var(--color-border-secondary))] shadow-[0_1px_3px_0_rgba(0,0,0,0.08)] hover:shadow-[0_3px_8px_0_rgba(0,0,0,0.12)]',
    primary: 'bg-[rgb(var(--color-accent-blue))] hover:bg-[rgb(var(--color-accent-blue))] text-white border border-transparent',
    secondary: 'bg-[rgb(var(--color-bg-tertiary))] hover:bg-zinc-700 text-[rgb(var(--color-text-primary))] border border-[rgb(var(--color-border-secondary))]',
    danger: 'bg-[rgb(var(--color-accent-red))] hover:bg-red-600 text-white border border-transparent',
    success: 'bg-[rgb(var(--color-accent-green))] hover:bg-green-600 text-white border border-transparent',
    ghost: 'bg-transparent hover:bg-[rgb(var(--color-bg-tertiary))] text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-text-primary))] border border-transparent',
  };

  const sizes = {
    xs: 'px-2 py-1 text-xs gap-1.5',
    sm: 'px-3 py-1.5 text-sm gap-2',
    md: 'px-4 py-2 text-sm gap-2',
    lg: 'px-5 py-2.5 text-base gap-2.5',
  };
</script>

<button
  {type}
  {disabled}
  {onclick}
  class="inline-flex items-center justify-center rounded-lg font-medium transition-all duration-200 ease-in-out focus:outline-none disabled:opacity-50 disabled:cursor-not-allowed disabled:pointer-events-none cursor-pointer {variants[variant]} {sizes[size]}"
>
  {@render children()}
</button>
