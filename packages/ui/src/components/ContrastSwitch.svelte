<script lang="ts">
  import { t } from "../messages/index.js";
  import { chooseContrast, siteTheme, useSiteTheme } from "../site-theme.svelte.js";
  import { onMount } from "svelte";

  /** The web surfaces' High contrast on/off switch, beside System · Light · Dark. */
  onMount(useSiteTheme);

  const on = $derived((siteTheme.contrast ?? siteTheme.prefersMore) === true);
</script>

<button
  type="button"
  role="switch"
  class="ui-interactive contrast"
  aria-checked={on}
  data-component="contrast-switch"
  onclick={() => chooseContrast(!on)}
>
  <span>{t("site.theme.contrast")}</span>
  <span class="state" aria-hidden="true"
    >{on ? t("site.theme.contrast_on") : t("site.theme.contrast_off")}</span
  >
</button>

<style>
  .contrast {
    --ui-bg: var(--surface-2);
    display: inline-flex;
    align-items: center;
    gap: var(--space-3);
    min-height: var(--space-touch-min);
    min-width: var(--space-touch-min);
    max-width: 100%;
    box-sizing: border-box;
    padding: 0 var(--space-5);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-full);
    color: var(--text);
    font: inherit;
    font-size: var(--type-hub-body-size);
    font-weight: var(--type-hub-title-weight);
    cursor: pointer;
  }

  .state {
    padding: 0 var(--space-2);
    border-radius: var(--radius-full);
    background-color: var(--text);
    color: var(--bg);
    font-size: var(--type-hub-label-size);
  }

  .contrast[aria-checked="false"] .state {
    background-color: transparent;
    color: var(--text-muted);
  }
</style>
