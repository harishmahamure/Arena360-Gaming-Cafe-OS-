<script lang="ts">
  import { t } from "@arena/ui";
  import GameTile from "./GameTile.svelte";
  import { railLabels, visibleGames, type GameCard } from "./games";

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

  // The filter rail and the search field (Story 6.24). One rail item is active at a time and
  // the grid follows at once; an item the library no longer has counts as All.
  let picked: string | null = $state(null);
  let query = $state("");

  const labels = $derived(games === null ? [] : railLabels(games));
  const label = $derived(
    picked !== null && labels.some((l) => l.toLowerCase() === picked!.toLowerCase())
      ? picked
      : null,
  );
  const shown = $derived(games === null ? [] : visibleGames(games, label, query));

  let railEl: HTMLElement | undefined = $state();
  let gridEl: HTMLElement | undefined = $state();
  let searchEl: HTMLInputElement | undefined = $state();

  function clearFilters() {
    picked = null;
    query = "";
    searchEl?.focus();
  }

  // Arrow keys step through the rail (Up and Down) and the grid (all four) on top of Tab.
  function move(e: KeyboardEvent, root: HTMLElement | undefined, selector: string, cols: boolean) {
    const keys = cols
      ? ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"]
      : ["ArrowUp", "ArrowDown"];
    if (!keys.includes(e.key) || !root) return;
    const items = [...root.querySelectorAll<HTMLElement>(selector)];
    const at = items.indexOf(document.activeElement as HTMLElement);
    if (at < 0) return;
    let to = at;
    if (e.key === "ArrowLeft" || (!cols && e.key === "ArrowUp")) to = at - 1;
    else if (e.key === "ArrowRight" || (!cols && e.key === "ArrowDown")) to = at + 1;
    else {
      // Up and Down in the grid go to the tile above or below: the nearest one in that column.
      const here = items[at].getBoundingClientRect();
      const dir = e.key === "ArrowUp" ? -1 : 1;
      let best = -1;
      let bestScore = Infinity;
      items.forEach((el, i) => {
        const r = el.getBoundingClientRect();
        const dy = (r.top - here.top) * dir;
        if (dy <= 1) return;
        const score = dy * 1000 + Math.abs(r.left - here.left);
        if (score < bestScore) {
          bestScore = score;
          best = i;
        }
      });
      to = best;
    }
    if (to < 0 || to >= items.length) return;
    e.preventDefault();
    items[to].focus();
  }
</script>

<section class="games" aria-label={t("player.games.grid_label")} data-testid="games">
  {#if games === null}
    <!-- Not answered yet: nothing to say, so no flash of the empty line. -->
  {:else if games.length === 0}
    <p class="empty type-kiosk-body" data-testid="games-empty">{t("player.shell.empty")}</p>
  {:else}
    <div class="layout">
      <!-- The `filter-rail`: All, then every category and tag; the active item is in `text`. -->
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <nav
        class="rail"
        aria-label={t("player.games.filter_label")}
        data-testid="filter-rail"
        bind:this={railEl}
        onkeydown={(e) => move(e, railEl, "button", false)}
      >
        <ul>
          {#each [null, ...labels] as item (item ?? "")}
            {@const active = item === null ? label === null : item === label}
            <li>
              <button
                type="button"
                class="rail-item type-kiosk-label"
                class:active
                aria-pressed={active}
                data-testid="filter-item"
                onclick={() => (picked = item)}
              >
                {item ?? t("player.games.filter_all")}
              </button>
            </li>
          {/each}
        </ul>
      </nav>

      <div class="content">
        <div class="title-row">
          <h2 class="type-kiosk-title">{t("player.games.title")}</h2>
          <input
            bind:this={searchEl}
            bind:value={query}
            class="search type-kiosk-body"
            type="search"
            name="game-search"
            autocomplete="off"
            autocapitalize="off"
            spellcheck="false"
            maxlength={64}
            aria-label={t("player.games.search_label")}
            placeholder={t("player.games.search_label")}
            data-testid="games-search"
          />
        </div>

        {#if shown.length === 0}
          <p class="empty type-kiosk-body" data-testid="games-no-match">
            {t("player.games.no_match")}
            <button
              type="button"
              class="clear type-kiosk-body"
              data-testid="games-clear"
              onclick={clearFilters}
            >
              {t("player.games.clear_filters")}
            </button>
          </p>
        {:else}
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <ul
            class="grid"
            data-testid="games-grid"
            bind:this={gridEl}
            onkeydown={(e) => move(e, gridEl, "button.game-tile", true)}
          >
            {#each shown as game (game.id)}
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
      </div>
    </div>
  {/if}
</section>

<style>
  .games {
    width: 100%;
  }

  .layout {
    display: grid;
    grid-template-columns: 200px minmax(0, 1fr);
    gap: var(--space-5);
    align-items: start;
  }

  .rail {
    padding: var(--space-3);
    border-radius: var(--radius-lg);
    background-color: var(--surface);
  }

  .rail ul {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .rail-item {
    box-sizing: border-box;
    width: 100%;
    min-height: var(--space-touch-min);
    padding: 0 var(--space-3);
    border: 0;
    border-left: 3px solid transparent;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
    font: inherit;
    text-align: left;
    overflow-wrap: anywhere;
  }

  .rail-item.active {
    border-left-color: var(--text);
    color: var(--text);
    font-weight: 700;
  }

  .rail-item:hover {
    color: var(--text);
  }

  .title-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
    padding: 0 var(--space-3);
  }

  .title-row h2 {
    margin: 0;
  }

  .search {
    box-sizing: border-box;
    width: 280px;
    max-width: 100%;
    min-height: var(--space-touch-min);
    margin-left: auto;
    padding: 0 var(--space-3);
    border: var(--text-field-border);
    border-color: var(--border-strong);
    border-radius: var(--radius-sm);
    background-color: var(--text-field-background);
    color: var(--text-field-foreground);
    font: inherit;
  }

  .search::placeholder {
    color: var(--text-field-placeholder);
  }

  .search:focus-visible {
    border-width: 2px;
    border-color: var(--accent);
    padding: 0 calc(var(--space-3) - 1px);
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, 140px);
    justify-content: start;
    gap: var(--space-5);
    margin: 0;
    padding: var(--space-3);
    list-style: none;
  }

  .empty {
    margin: 0;
    padding: var(--space-4) var(--space-3);
    color: var(--text-muted);
    text-align: center;
  }

  .clear {
    display: inline;
    min-height: var(--space-touch-min);
    padding: 0 var(--space-2);
    border: 0;
    background: transparent;
    color: var(--text);
    cursor: pointer;
    font: inherit;
    text-decoration: underline;
  }

  .rail-item:focus-visible,
  .clear:focus-visible,
  .search:focus-visible {
    outline: 4px solid var(--focus-ring);
    outline-offset: 2px;
  }
</style>
