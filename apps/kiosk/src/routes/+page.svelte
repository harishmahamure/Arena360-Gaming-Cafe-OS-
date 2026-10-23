<script lang="ts">
  import { t } from "@arena/ui";
</script>

<svelte:head>
  <title>{t("app.title", { surface: t("surface.kiosk") })}</title>
</svelte:head>

<!-- No session yet: the locked placeholder (login comes with the session stories). -->
<main class="locked" data-testid="locked">
  <p class="brand type-kiosk-eyebrow">{t("app.name")}</p>
  <h1 class="type-kiosk-headline">{t("player.locked.ready")}</h1>
  <p class="hint type-kiosk-body">{t("player.locked.hint")}</p>
</main>

<style>
  .locked {
    display: grid;
    place-content: center;
    gap: var(--space-6);
    min-height: 100vh;
    padding: var(--space-kiosk-margin);
    color: var(--text);
    text-align: center;
  }

  h1 {
    max-width: 24ch;
    margin: 0 auto;
  }

  .brand,
  .hint {
    margin: 0;
    color: var(--text-muted);
  }
</style>
