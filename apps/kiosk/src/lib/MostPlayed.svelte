<script lang="ts">
  import { t } from "@arena/ui";
  import GameTile from "./GameTile.svelte";
  import type { GameCard, RankedGame } from "./games";

  let {
    tiles,
    branch = "",
    gameRunning = false,
    onlaunch,
  }: {
    /** The tiles to show, already ranked and limited to the games this PC has. */
    tiles: RankedGame[];
    /** The branch's name for the caption. */
    branch?: string;
    /** Any game is running (the shell's signal, wired into every tile's media). */
    gameRunning?: boolean;
    /** A tile was activated: it launches the game like any other tile. */
    onlaunch: (game: GameCard) => void;
  } = $props();

  let rowEl: HTMLElement | undefined = $state();

  // Left and Right move focus from tile to tile and scroll the new one into view, on top of Tab.
  function move(e: KeyboardEvent) {
    if ((e.key !== "ArrowLeft" && e.key !== "ArrowRight") || !rowEl) return;
    const items = [...rowEl.querySelectorAll<HTMLElement>("button.game-tile")];
    const at = items.indexOf(document.activeElement as HTMLElement);
    if (at < 0) return;
    const to = at + (e.key === "ArrowLeft" ? -1 : 1);
    if (to < 0 || to >= items.length) return;
    e.preventDefault();
    items[to].focus();
    items[to].scrollIntoView({ block: "nearest", inline: "nearest" });
  }
</script>

<!-- The `most-played-carousel`: the branch's top games by play hours over the last 7 days, ranked
     "#1" onward. Not shown at all when the window has no plays. -->
<section class="most-played" aria-labelledby="most-played-title" data-testid="most-played-carousel">
  <h2 id="most-played-title" class="type-kiosk-title">{t("player.games.most_played_title")}</h2>
  <p class="caption type-kiosk-label" data-testid="most-played-caption">
    {branch === ""
      ? t("player.games.most_played_caption_no_branch")
      : t("player.games.most_played_caption", { branch })}
  </p>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <ul class="row" data-testid="most-played-row" bind:this={rowEl} onkeydown={move}>
    {#each tiles as tile (tile.game.id)}
      <li>
        <GameTile
          name={tile.game.name}
          running={tile.game.running}
          {gameRunning}
          rank={tile.rank}
          onactivate={() => onlaunch(tile.game)}
        />
      </li>
    {/each}
  </ul>
</section>

<style>
  .most-played {
    padding: 0 var(--space-3) var(--space-5);
  }

  .most-played h2 {
    margin: 0;
  }

  .caption {
    margin: 0 0 var(--space-3);
    color: var(--text-muted);
  }

  /* One row that scrolls sideways: the tiles keep their size. */
  .row {
    display: flex;
    gap: var(--space-5);
    margin: 0;
    padding: var(--space-3);
    overflow-x: auto;
    list-style: none;
    scroll-padding: var(--space-3);
  }

  .row li {
    flex: none;
  }
</style>
