<script lang="ts">
  import { t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { Snippet } from "svelte";

  /**
   * A screen that is not built yet: its title and a note, never a hidden destination. A
   * placeholder may carry a demo control (`children`).
   */
  let { title, children }: { title: MessageKey; children?: Snippet } = $props();
</script>

<svelte:head>
  <title>{t("hub.page_title", { screen: t(title) })}</title>
</svelte:head>

<section class="placeholder" data-screen-placeholder>
  <h1 class="type-hub-numeric">{t(title)}</h1>
  <p class="type-hub-body">{t("hub.placeholder.body")}</p>
  {@render children?.()}
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
