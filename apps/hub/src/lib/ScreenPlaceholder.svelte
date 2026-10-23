<script lang="ts">
  import { t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";

  /** A screen that is not built yet: its title and a note, never a hidden destination. */
  let { title }: { title: MessageKey } = $props();
</script>

<svelte:head>
  <title>{t("hub.page_title", { screen: t(title) })}</title>
</svelte:head>

<section class="placeholder" data-screen-placeholder>
  <h1 class="type-hub-numeric">{t(title)}</h1>
  <p class="type-hub-body">{t("hub.placeholder.body")}</p>
</section>

<style>
  .placeholder {
    display: grid;
    gap: var(--space-3);
    max-width: 60ch;
  }

  h1,
  p {
    margin: 0;
  }

  p {
    color: var(--text-muted);
  }
</style>
