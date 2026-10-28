<script lang="ts" module>
  import type { MessageKey } from "../messages/index.js";

  export type AlertTag = "new" | "call" | "time-up" | "request" | "controllers" | "locked-out";

  export const ALERT_TAGS: Record<AlertTag, MessageKey> = {
    new: "ui.alert.tag.new",
    call: "ui.alert.tag.call",
    "time-up": "ui.alert.tag.time_up",
    request: "ui.alert.tag.request",
    controllers: "ui.alert.tag.controllers",
    "locked-out": "ui.alert.tag.locked_out",
  };
</script>

<script lang="ts">
  import { t } from "../messages/index.js";

  /**
   * Pass `count` for a counter (changes are announced politely unless `live` is off, for a
   * caller that announces them itself) or `tag` for a short tag.
   */
  let {
    count,
    tag,
    live = true,
  }:
    | { count: number; tag?: undefined; live?: boolean }
    | { count?: undefined; tag: AlertTag; live?: boolean } = $props();

  const n = $derived(count ?? 0);
</script>

<span class="alert" data-component="alert-badge">
  {#if tag}
    <span class="pill type-hub-micro">{t(ALERT_TAGS[tag])}</span>
  {:else}
    {#if n > 0}
      <span class="pill type-hub-micro tabular-nums" aria-hidden="true">{n}</span>
    {/if}
    <!-- The live region stays mounted (empty at zero) so a change from 0 is still announced. -->
    <span
      class="visually-hidden"
      role={live ? "status" : undefined}
      aria-live={live ? "polite" : undefined}
      >{n > 0 ? t(n === 1 ? "ui.alert.count_one" : "ui.alert.count_other", { count: n }) : ""}</span
    >
  {/if}
</span>

<style>
  .alert {
    display: inline-flex;
    max-width: 100%;
  }

  .pill {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: var(--space-5);
    box-sizing: border-box;
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius-full);
    background-color: var(--alert-badge-background);
    color: var(--alert-badge-foreground);
    overflow-wrap: anywhere;
  }
</style>
