<script lang="ts">
  import { t } from "@arena/ui";
  import type { LauncherId } from "./games";

  let {
    id,
    name,
    running = false,
    onactivate,
  }: {
    /** `steam`, `epic` or `riot`. */
    id: LauncherId;
    /** The launcher's name on the card. */
    name: string;
    /** The launcher is open now: activating it brings its window to the front. */
    running?: boolean;
    /** Enter, Space or a click. */
    onactivate: () => void;
  } = $props();

  // The launcher's mark: its initial on a solid square (the brand artwork is not bundled).
  const mark = $derived(name.trim().charAt(0).toUpperCase());
</script>

<!-- A `launcher-card`: a `surface` card with the launcher's mark, its name and "Use your own
     account" (its accessible name is that text, plus "Open" when it is open). It opens the launcher on its login screen; the player signs in with their own
     account and the session end signs it out again. -->
<button
  type="button"
  class="launcher-card"
  class:running
  data-testid="launcher-card"
  data-launcher={id}
  data-running={running ? "true" : "false"}
  onclick={onactivate}
>
  <span class="mark type-kiosk-title" aria-hidden="true" data-testid="launcher-card-mark">
    {mark}
  </span>
  <span class="copy">
    <span class="name type-kiosk-tile-name" data-testid="launcher-card-name">{name}</span>
    <span class="caption type-kiosk-label" data-testid="launcher-card-caption">
      {t("player.games.launcher_caption")}
    </span>
  </span>
  {#if running}
    <span class="badge type-kiosk-label" data-testid="launcher-card-open">
      {t("player.games.launcher_open")}
    </span>
  {/if}
</button>

<style>
  .launcher-card {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    box-sizing: border-box;
    width: 280px;
    min-height: 88px;
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    background-color: var(--surface);
    color: var(--text);
    cursor: pointer;
    font: inherit;
    text-align: left;
  }

  .mark {
    display: grid;
    flex: none;
    place-content: center;
    width: 48px;
    height: 48px;
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background-color: var(--bg);
    color: var(--text);
  }

  .copy {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
    gap: 2px;
  }

  .name {
    overflow-wrap: anywhere;
  }

  .caption {
    color: var(--text-muted);
  }

  .badge {
    flex: none;
    padding: 2px var(--space-2);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-full);
    background-color: var(--bg);
    color: var(--text);
  }

  .launcher-card:hover {
    border-color: var(--border-strong);
  }

  .launcher-card:focus-visible,
  .launcher-card:focus {
    outline: 3px solid var(--focus-ring);
    outline-offset: 3px;
  }
</style>
