<script lang="ts">
  import { t } from "@arena/ui";
</script>

<!--
  The quiet "Reconnecting…" pill (Story 6.36): shown while the Hub is unreachable, left of the
  volume control on the shell and on the expanded HUD. A solid surface and a static icon: it never
  animates, never takes focus and never blocks a click.
-->
<span class="pill type-kiosk-label" role="status" data-testid="reconnecting-indicator">
  <svg class="icon" viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
    <circle cx="8" cy="8" r="6" fill="none" stroke="currentColor" stroke-width="1.5" />
    <path d="M8 4.5V8.5L10.5 10" fill="none" stroke="currentColor" stroke-width="1.5" />
  </svg>
  {t("player.hub.reconnecting")}
</span>

<style>
  .pill {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-4);
    border-radius: var(--radius-full);
    background-color: var(--reconnecting-indicator-background);
    color: var(--reconnecting-indicator-foreground);
    white-space: nowrap;
    pointer-events: none;
    justify-self: start;
  }

  .icon {
    flex: none;
  }
</style>
