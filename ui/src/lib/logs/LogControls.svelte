<script lang="ts">
  import Button from '../common/Button.svelte';
  import { Play, Pause, Trash2, Download, Copy } from '@lucide/svelte';

  let { 
    isFollowing = $bindable(true),
    isPaused = $bindable(false),
    onClear,
    onDownload,
    onCopy
  } = $props();
</script>

<div class="flex items-center justify-between px-6 py-4 bg-gray-800 border-b border-gray-700">
  <div class="flex items-center gap-3">
    <Button 
      variant={isFollowing ? 'primary' : 'secondary'}
      size="sm"
      onclick={() => isFollowing = !isFollowing}
    >
      {isFollowing ? '● Follow' : '○ Follow'}
    </Button>
    
    <Button 
      variant="secondary"
      size="sm"
      onclick={() => isPaused = !isPaused}
    >
      {#if isPaused}
        <Play class="w-3 h-3 mr-1" />
        Resume
      {:else}
        <Pause class="w-3 h-3 mr-1" />
        Pause
      {/if}
    </Button>

    <div class="h-6 w-px bg-gray-700"></div>

    <Button variant="secondary" size="sm" onclick={onClear}>
      <Trash2 class="w-3 h-3 mr-1" />
      Clear
    </Button>

    <Button variant="secondary" size="sm" onclick={onCopy}>
      <Copy class="w-3 h-3 mr-1" />
      Copy
    </Button>

    <Button variant="secondary" size="sm" onclick={onDownload}>
      <Download class="w-3 h-3 mr-1" />
      Download
    </Button>
  </div>

  <div class="flex items-center gap-4">
    <select class="px-3 py-1.5 text-sm bg-gray-700 border border-gray-600 rounded text-[rgb(var(--color-text-primary))] focus:outline-none focus:ring-2 focus:ring-blue-500">
      <option value="all">All Levels</option>
      <option value="info">INFO</option>
      <option value="warn">WARN</option>
      <option value="error">ERROR</option>
      <option value="debug">DEBUG</option>
    </select>

    <select class="px-3 py-1.5 text-sm bg-gray-700 border border-gray-600 rounded text-[rgb(var(--color-text-primary))] focus:outline-none focus:ring-2 focus:ring-blue-500">
      <option value="1000">1000 lines</option>
      <option value="5000">5000 lines</option>
      <option value="10000">10000 lines</option>
      <option value="all">All lines</option>
    </select>
  </div>
</div>
