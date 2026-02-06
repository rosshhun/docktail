<script lang="ts">
  let { size = 'md' as 'sm' | 'md' | 'lg' } = $props();

  const sizes = {
    sm: 'w-4 h-4 border-2',
    md: 'w-8 h-8 border-2',
    lg: 'w-12 h-12 border-4',
  };
</script>

<div class="inline-block {sizes[size]} border-blue-500 border-t-transparent rounded-full animate-spin"></div>
