<script lang="ts">
  import "@arena/ui/base.css";
  import { ThemeToggle, t } from "@arena/ui";
  import { page } from "$app/state";
  import type { Snippet } from "svelte";

  let { children }: { children: Snippet } = $props();
</script>

<div class="shell">
  <header class="header">
    <a class="brand" href="/">{t("app.name")}</a>
  </header>
  <main class="main" class:wide={page.url.pathname.startsWith("/setup")}>
    {@render children()}
  </main>
  <footer class="footer">
    <ThemeToggle />
  </footer>
</div>

<style>
  :global(body) {
    margin: 0;
  }

  .shell {
    display: flex;
    flex-direction: column;
    min-height: 100vh;
    min-height: 100dvh;
    box-sizing: border-box;
    max-width: 1200px;
    margin: 0 auto;
    padding: 0 var(--space-4);
  }

  .header,
  .footer {
    display: flex;
    align-items: center;
    padding: var(--space-4) 0;
  }

  .brand {
    display: inline-flex;
    align-items: center;
    min-height: var(--space-touch-min);
    min-width: var(--space-touch-min);
    font-size: var(--type-hub-title-size);
    font-weight: var(--type-hub-title-weight);
  }

  .main {
    flex: 1;
    width: 100%;
    max-width: 36rem;
    padding-bottom: var(--space-6);
  }

  .main.wide {
    max-width: none;
  }

  .footer {
    border-top: 1px solid var(--line);
  }
</style>
