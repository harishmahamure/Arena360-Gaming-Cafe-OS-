<script lang="ts">
  import { t, type MessageKey } from "../messages/index.js";

  type Tag = "new" | "call" | "time-up" | "request";

  const TAGS: Record<Tag, MessageKey> = {
    new: "ui.alert.tag.new",
    call: "ui.alert.tag.call",
    "time-up": "ui.alert.tag.time_up",
    request: "ui.alert.tag.request",
  };

  /** Pass `count` for a counter (changes are announced politely) or `tag` for a short tag. */
  let { count, tag }: { count: number; tag?: undefined } | { count?: undefined; tag: Tag } =
    $props();

  const n = $derived(count ?? 0);
</script>

<span class="alert" data-component="alert-badge">
  {#if tag}
    <span class="pill type-hub-micro">{t(TAGS[tag])}</span>
  {:else}
    {#if n > 0}
      <span class="pill type-hub-micro tabular-nums" aria-hidden="true">{n}</span>
    {/if}
    <!-- The live region stays mounted (empty at zero) so a change from 0 is still announced. -->
    <span class="visually-hidden" role="status" aria-live="polite"
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
