<script lang="ts">
  import { t, type MessageKey } from "../messages/index.js";

  /** A link that looks like a `Button` (navigation, not an action): the marketing "Get started". */
  let {
    href,
    label,
    variant = "primary",
  }: { href: string; label: MessageKey; variant?: "primary" | "secondary" } = $props();
</script>

<a class="ui-interactive link-button link-button--{variant}" {href} data-component="link-button"
  >{t(label)}</a
>

<style>
  .link-button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-height: var(--space-touch-min);
    min-width: var(--space-touch-min);
    max-width: 100%;
    box-sizing: border-box;
    padding: var(--space-2) var(--space-5);
    border: 1px solid transparent;
    border-radius: var(--radius-full);
    font-family: var(--type-kiosk-label-family), system-ui, sans-serif;
    font-size: var(--type-hub-title-size);
    font-weight: var(--type-kiosk-cta-weight);
    line-height: var(--type-kiosk-cta-line-height);
    text-align: center;
    text-decoration-line: none;
    overflow-wrap: anywhere;
    cursor: pointer;
  }

  .link-button--primary {
    --ui-bg: var(--button-primary-background);
    color: var(--button-primary-foreground);
  }

  .link-button--secondary {
    --ui-bg: var(--button-secondary-background);
    color: var(--button-secondary-foreground);
    border-color: var(--border-strong);
  }
</style>
