<script lang="ts">
  import { GameMedia, isHubMediaUrl, t } from "@arena/ui";

  let {
    name,
    cover = null,
    running = false,
    gameRunning = false,
    onactivate,
  }: {
    name: string;
    /** The cover (a Hub `/media/<sha256>` URL); without one the tile is the `surface` fallback. */
    cover?: string | null;
    /** This tile's game is running now. */
    running?: boolean;
    /** Any game is running: the media inside stops its video (Story 6.6). */
    gameRunning?: boolean;
    /** Enter, Space or a click. */
    onactivate: () => void;
  } = $props();

  const hasCover = $derived(isHubMediaUrl(cover));
</script>

<!-- A `game-tile`: a portrait cover, 140 × 196, the name bottom-left on the tile scrim, or the
     name centred on `surface` when there is no cover. -->
<button
  type="button"
  class="game-tile"
  class:running
  aria-label={running ? t("player.games.tile_running", { name }) : name}
  data-testid="game-tile"
  data-running={running ? "true" : "false"}
  data-game-running={gameRunning ? "true" : "false"}
  onclick={onactivate}
>
  <GameMedia class="media" scrim="tile" {cover} {gameRunning} data-testid="game-tile-media">
    {#if hasCover}
      <span class="name type-kiosk-tile-name" data-testid="game-tile-name">{name}</span>
    {/if}
  </GameMedia>
  {#if !hasCover}
    <span class="name fallback type-kiosk-tile-name" data-testid="game-tile-name">{name}</span>
  {/if}
  {#if running}
    <span class="badge type-kiosk-label" data-testid="game-tile-running" aria-hidden="true">
      {t("player.games.running")}
    </span>
  {/if}
</button>

<style>
  .game-tile {
    position: relative;
    box-sizing: border-box;
    width: 140px;
    height: 196px;
    padding: 0;
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    background-color: var(--surface);
    color: var(--text);
    box-shadow: var(--elevation-game-tile);
    overflow: hidden;
    cursor: pointer;
    font: inherit;
    text-align: left;
  }

  .game-tile :global(.media) {
    position: absolute;
    inset: 0;
    background: var(--surface);
  }

  .name {
    display: block;
    overflow: hidden;
    overflow-wrap: anywhere;
    text-overflow: ellipsis;
    /* The tile's copy zone holds two lines of the 20px name. */
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
  }

  .name.fallback {
    position: absolute;
    inset: 0;
    display: grid;
    place-content: center;
    padding: var(--space-3);
    text-align: center;
  }

  .badge {
    position: absolute;
    top: var(--space-2);
    left: var(--space-2);
    padding: 2px var(--space-2);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-full);
    background-color: var(--bg);
    color: var(--text);
  }

  .game-tile:hover {
    border-color: var(--border-strong);
  }

  .game-tile:focus-visible,
  .game-tile:focus {
    outline: 3px solid var(--focus-ring);
    outline-offset: 3px;
  }
</style>
