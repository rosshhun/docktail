<script lang="ts">
  import { RefreshCw } from '@lucide/svelte';
  import type { GraphQLError } from '../api';

  interface Props {
    error: GraphQLError;
    onRetry?: () => void | Promise<void>;
    isRetrying?: boolean;
    showBackButton?: boolean;
    backHref?: string;
    backLabel?: string;
  }

  let { 
    error, 
    onRetry, 
    isRetrying = false,
    showBackButton = false,
    backHref = '/',
    backLabel = 'Go Back'
  }: Props = $props();

  async function handleRetry() {
    if (onRetry) {
      await onRetry();
    }
  }
</script>

<div class="flex items-center justify-center py-16">
  <div class="text-center max-w-md px-4">
    <!-- Error Icon -->
    <div class="inline-flex items-center justify-center w-16 h-16 rounded-full mb-4
                {error.isUnavailable() || error.isRetryable() ? 'bg-yellow-50' : 'bg-red-50'}">
      <svg class="w-8 h-8 {error.isUnavailable() || error.isRetryable() ? 'text-yellow-600' : 'text-red-600'}" 
           fill="none" viewBox="0 0 24 24" stroke="currentColor">
        {#if error.isNotFound()}
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" 
                d="M9.172 16.172a4 4 0 015.656 0M9 10h.01M15 10h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
        {:else if error.isUnavailable()}
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" 
                d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
        {:else if error.isUnauthorized()}
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" 
                d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z" />
        {:else}
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" 
                d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
        {/if}
      </svg>
    </div>

    <!-- Error Title -->
    <h3 class="text-lg font-semibold text-[rgb(var(--color-text-primary))] mb-2">
      {#if error.isNotFound()}
        Not Found
      {:else if error.isUnavailable()}
        Temporarily Unavailable
      {:else if error.isUnauthorized()}
        Authentication Required
      {:else if error.isForbidden()}
        Access Denied
      {:else}
        Error
      {/if}
    </h3>

    <!-- Error Message -->
    <p class="text-sm text-[rgb(var(--color-text-secondary))] mb-1">
      {error.getUserMessage()}
    </p>
    
    <!-- Error Code (for debugging) -->
    {#if error.code}
      <p class="text-xs text-[rgb(var(--color-text-secondary))] font-mono mb-4">
        Code: {error.code}
      </p>
    {/if}

    <!-- Actions -->
    <div class="flex gap-2 justify-center">
      {#if error.isRetryable() && onRetry}
        <button
          onclick={handleRetry}
          disabled={isRetrying}
          class="px-4 py-2 bg-[rgb(var(--color-accent-blue))] text-white rounded-lg hover:bg-[#7C3BAD] 
                 transition-colors text-sm font-medium flex items-center gap-2
                 disabled:opacity-50 disabled:cursor-not-allowed"
        >
          {#if isRetrying}
            <RefreshCw class="w-4 h-4 animate-spin" />
            Retrying...
          {:else}
            <RefreshCw class="w-4 h-4" />
            Retry
          {/if}
        </button>
      {/if}
      
      {#if showBackButton}
        <a
          href={backHref}
          class="px-4 py-2 border border-[rgb(var(--color-border-primary))] rounded-lg 
                 hover:bg-[rgb(var(--color-bg-secondary))] transition-colors text-sm font-medium
                 text-[rgb(var(--color-text-primary))]"
        >
          {backLabel}
        </a>
      {/if}
    </div>
  </div>
</div>
