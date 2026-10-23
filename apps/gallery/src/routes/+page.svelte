<script lang="ts">
  import { t } from "@arena/ui";
  import ThemeSection from "./ThemeSection.svelte";

  const THEMES = [
    { id: "dark", title: "gallery.theme.dark" },
    { id: "hc", title: "gallery.theme.hc" },
    { id: "light", title: "gallery.theme.light" },
  ] as const;
</script>

<svelte:head>
  <title>{t("gallery.title")}</title>
</svelte:head>

<header class="page-header">
  <h1>{t("gallery.heading")}</h1>
  <p>{t("gallery.intro")}</p>
</header>

<main>
  {#each THEMES as theme (theme.id)}
    <ThemeSection theme={theme.id} title={theme.title} />
  {/each}
</main>

<style>
  .page-header {
    padding: var(--space-6) var(--space-web-gutter-phone);
  }

  h1 {
    margin: 0 0 var(--space-2);
    font-size: var(--type-web-display-size);
    font-weight: var(--type-web-display-weight);
    line-height: var(--type-web-display-line-height);
  }

  p {
    margin: 0;
    color: var(--text-muted);
  }

  main {
    display: flex;
    flex-direction: column;
  }
</style>
