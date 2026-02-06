<script lang="ts">
  import { AlertCircle, RefreshCw } from '@lucide/svelte';
  import Button from './Button.svelte';
  import type { GraphQLError } from '../api';

  interface Props {
    error: GraphQLError;
    onRetry?: () => void;
    title?: string;
    children?: any;
  }

  let { error, onRetry, title = 'Error', children }: Props = $props();

  const isRetryable = $derived(error.isUnavailable() || error.isRetryable());
</script>

<div class="flex items-center justify-center py-16">
  <div class="text-center max-w-md">
    <div class="inline-flex items-center justify-center w-12 h-12 rounded-full mb-3 {isRetryable ? 'bg-yellow-50' : 'bg-red-50'}">
      <AlertCircle class="w-6 h-6 {isRetryable ? 'text-yellow-600' : 'text-red-600'}" />
    </div>
    <p class="text-sm font-medium text-[rgb(var(--color-text-primary))] mb-1">
      {isRetryable ? 'Service Unavailable' : title}
    </p>
    <p class="text-xs text-[rgb(var(--color-text-secondary))] mb-1">
      {error.getUserMessage()}
    </p>
    {#if error.code}
      <p class="text-xs text-[rgb(var(--color-text-secondary))] font-mono mb-4">
        Code: {error.code}
      </p>
    {/if}
    {#if onRetry}
      <Button variant="primary" size="md" onclick={onRetry}>
        <RefreshCw class="w-4 h-4" />
        Retry
      </Button>
    {/if}
    {#if children}
      <div class="mt-4">
        {@render children()}
      </div>
    {/if}
  </div>
</div>
