<script lang="ts">
  import Header from "./Header.svelte";
  import Footer from "./Footer.svelte";
  import Sidebar from "../Sidebar.svelte";
</script>

<div
  class="flex h-screen bg-[rgb(var(--color-bg-primary))] overflow-hidden font-sans text-[rgb(var(--color-text-primary))]"
>
  <!-- Sidebar -->
  <Sidebar />

  <!-- Main Content Area -->
  <div class="flex-1 flex flex-col min-w-0">
    <!-- Page Content -->
    <main class="flex-1 overflow-auto">
      <slot />
    </main>
    
    <!-- Footer -->
    <Footer />
  </div>
</div>
