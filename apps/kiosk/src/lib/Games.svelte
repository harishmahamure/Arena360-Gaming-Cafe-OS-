<script lang="ts">
  import { t } from "@arena/ui";
  import GameTile from "./GameTile.svelte";
  import type { GameCard } from "./games";

  let {
    games,
    gameRunning = false,
    onlaunch,
  }: {
    /** The games this PC lists, sorted by name; `null` until the service has answered. */
    games: GameCard[] | null;
    /** Any game is running (the shell's signal, wired into every tile's media). */
    gameRunning?: boolean;
    /** A tile was activated. */
    onlaunch: (game: GameCard) => void;
  } = $props();
</script>

<section class="games" aria-label={t("player.games.grid_label")} data-testid="games">
  {#if games === null}
    <!-- Not answered yet: nothing to say, so no flash of the empty line. -->
  {:else if games.length === 0}
    <p class="empty type-kiosk-body" data-testid="games-empty">{t("player.shell.empty")}</p>
  {:else}
    <ul class="grid" data-testid="games-grid">
      {#each games as game (game.id)}
        <li>
          <GameTile
            name={game.name}
            running={game.running}
            {gameRunning}
            onactivate={() => onlaunch(game)}
          />
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .games {
    width: 100%;
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, 140px);
    justify-content: center;
    gap: var(--space-5);
    margin: 0;
    padding: var(--space-3);
    list-style: none;
  }

  .empty {
    margin: 0;
    color: var(--text-muted);
    text-align: center;
  }
</style>
