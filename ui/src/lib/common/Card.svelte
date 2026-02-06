<script lang="ts">
  let { padding = 'p-4', children } = $props();
</script>

<div class="card-elevated rounded-lg {padding}">
  {@render children()}
</div>
