<script lang="ts">
  interface Props {
    title: string;
    icon?: any;
    value?: string | number;
    subtitle?: string;
    variant?: 'default' | 'success' | 'warning' | 'error';
    children?: any;
  }

  let { title, icon: Icon, value, subtitle, variant = 'default', children }: Props = $props();

  const variants = {
    default: 'bg-[rgb(var(--color-bg-tertiary))]',
    success: 'bg-green-50 border-green-200',
    warning: 'bg-yellow-50 border-yellow-200',
    error: 'bg-red-50 border-red-200'
  };

  const textVariants = {
    default: 'text-[rgb(var(--color-text-primary))]',
    success: 'text-green-800',
    warning: 'text-yellow-800',
    error: 'text-red-800'
  };
</script>

<div class="bg-[rgb(var(--color-bg-secondary))] rounded-lg border border-[rgb(var(--color-border-primary))] p-6">
  <h2 class="text-sm font-semibold text-[rgb(var(--color-text-primary))] mb-4 flex items-center gap-2">
    {#if Icon}
      <Icon class="w-4 h-4" />
    {/if}
    {title}
  </h2>
  {#if children}
    {@render children()}
  {:else if value !== undefined}
    <div class="text-center">
      <div class="text-3xl font-bold {textVariants[variant]}">
        {value}
      </div>
      {#if subtitle}
        <p class="text-xs text-[rgb(var(--color-text-secondary))] mt-1">{subtitle}</p>
      {/if}
    </div>
  {/if}
</div>
