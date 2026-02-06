<script lang="ts">
  // Get current year dynamically
  const currentYear = new Date().getFullYear();
  const version = "0.1.0"; // Could be imported from a config file

  const links = [
    { label: "Docs", href: "https://docktail.dev/docs" },
    { label: "GitHub", href: "https://github.com/rosshhun/docktail" },
    { label: "Issues", href: "https://github.com/rosshhun/docktail/issues" },
  ];
</script>

<footer>
  <div class="px-8 py-2.5">
    <div class="flex items-center justify-between">
      <!-- Left Section: Copyright & Version -->
      <div class="flex items-center">
        <div class="docktail-logo-wrapper">
          <img src="/docktail.svg" alt="Docktail" class="w-17 filter invert" />
        </div>
      </div>

      <!-- Right Section: Quick Links -->
      <div class="flex items-center gap-4">
        <span
          class="px-2 py-0.5 text-xs font-semibold text-[rgb(var(--color-text-secondary))] rounded border border-[rgb(var(--color-border-primary))]"
        >
          v{version}
        </span>
        {#each links as link}
          <a
            href={link.href}
            class="text-xs text-[rgb(var(--color-text-secondary))] hover:text-[rgb(var(--color-accent-blue))] transition-colors duration-200"
          >
            {link.label}
          </a>
        {/each}
      </div>
    </div>
  </div>
</footer>

<style>
  :global(.docktail-logo-wrapper) {
    transition: background-color 0.4s ease-in-out;
    padding: 2px 6px;
    cursor: pointer;
  }
  :global(.docktail-logo-wrapper:hover) {
    background-color: #635bff;
  }
</style>
