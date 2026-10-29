<script lang="ts">
  import { GameMedia, isHubMediaUrl, t } from "@arena/ui";

  let {
    name,
    cover = null,
    running = false,
    gameRunning = false,
    rank = null,
    featured = false,
    onactivate,
  }: {
    name: string;
    /** The cover (a Hub `/media/<sha256>` URL); without one the tile is the `surface` fallback. */
    cover?: string | null;
    /** This tile's game is running now. */
    running?: boolean;
    /** Any game is running: the media inside stops its video (Story 6.6). */
    gameRunning?: boolean;
    /** The tile's place in the "Most played here" carousel (1 is first); `null` elsewhere. */
    rank?: number | null;
    /**
     * The Home row's featured tile (Story 6.29): 190 × 260. Its magenta outline is drawn by the
     * row, 3px offset 3px; the focus ring sits outside it so the two never read as one.
     */
    featured?: boolean;
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
  class:featured
  aria-label={rank !== null
    ? undefined
    : featured
      ? t("player.home.featured_tile", { name })
      : running
        ? t("player.games.tile_running", { name })
        : name}
  data-testid={featured ? "game-tile-featured" : "game-tile"}
  data-running={running ? "true" : "false"}
  data-game-running={gameRunning ? "true" : "false"}
  onclick={onactivate}
>
  {#if rank !== null}
    <!-- The rank: `bg` fill, `text` ink, top-left (the running badge moves to the top-right). A
         ranked tile has no `aria-label`: its text, "#1 Name", is its accessible name. -->
    <span class="badge rank type-kiosk-label" data-testid="game-tile-rank">
      {t("player.games.most_played_rank", { rank })}
    </span>
  {/if}
  <GameMedia class="media" scrim="tile" {cover} {gameRunning} data-testid="game-tile-media">
    {#if hasCover}
      <span class="name type-kiosk-tile-name" data-testid="game-tile-name">{name}</span>
    {/if}
  </GameMedia>
  {#if !hasCover}
    <span class="name fallback type-kiosk-tile-name" data-testid="game-tile-name">{name}</span>
  {/if}
  {#if running}
    <span
      class="badge type-kiosk-label"
      class:beside-rank={rank !== null}
      data-testid="game-tile-running"
      aria-hidden={rank === null ? "true" : undefined}
    >
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

  .badge.beside-rank {
    right: var(--space-2);
    left: auto;
  }

  .game-tile.featured {
    width: 190px;
    height: 260px;
  }

  .game-tile:hover {
    border-color: var(--border-strong);
  }

  .game-tile:focus-visible,
  .game-tile:focus {
    outline: 3px solid var(--focus-ring);
    outline-offset: 3px;
  }

  /* Past the featured outline (3px line, 3px gap): the ring stays distinct from it. */
  .game-tile.featured:focus-visible,
  .game-tile.featured:focus {
    outline-offset: 9px;
  }
</style>
