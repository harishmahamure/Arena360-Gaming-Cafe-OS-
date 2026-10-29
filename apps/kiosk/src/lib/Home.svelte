<script lang="ts">
  import { GameMedia, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { tick } from "svelte";
  import GameTile from "./GameTile.svelte";
  import {
    daysSince,
    featuredOf,
    featuredRow,
    lastPlayedAt,
    mediaOf,
    type GameCard,
    type MostPlayedView,
  } from "./games";

  let {
    games,
    view = null,
    branch = "",
    gameRunning = false,
    paused = $bindable(false),
    onlaunch,
    onopengames,
  }: {
    /** This PC's games; `null` until the service has answered (nothing is drawn yet). */
    games: GameCard[] | null;
    /** The Hub's most-played answer (also the games' media and this player's last-played times). */
    view?: MostPlayedView | null;
    /** The branch's name, for the eyebrow. */
    branch?: string;
    /** Any game is running (the shell's signal): the hero video is unloaded. */
    gameRunning?: boolean;
    /** The player paused the hero video; it holds its frame for the rest of the session. */
    paused?: boolean;
    /** Play now, or a featured-row tile: launches the game (Story 6.23). */
    onlaunch: (game: GameCard) => void | Promise<void>;
    /** The "All games" tile: opens the Games tab with no filter. */
    onopengames: () => void;
  } = $props();

  const LAUNCHERS: Record<string, MessageKey> = {
    steam: "player.home.launcher.steam",
    epic: "player.home.launcher.epic",
    riot: "player.home.launcher.riot",
    battlenet: "player.home.launcher.battlenet",
    none: "player.home.launcher.none",
  };

  const featured = $derived(games === null ? null : featuredOf(view, games));
  const row = $derived(games === null ? [] : featuredRow(view, games));
  const hero = $derived(featured === null ? null : featured.game);
  const media = $derived(hero === null ? { video: null, cover: null } : mediaOf(view, hero));

  /** The launcher's name for the details panel; "Not listed" for an unknown one. */
  const launcherName = (id: string) => (LAUNCHERS[id] ? t(LAUNCHERS[id]) : "");
  const meta = $derived.by(() => {
    if (hero === null) return "";
    const parts = [
      hero.category.trim(),
      hero.launcher === "none" ? "" : launcherName(hero.launcher),
    ];
    const at = lastPlayedAt(view, hero);
    if (at !== null) {
      const days = daysSince(at, Date.now());
      parts.push(
        days === 0
          ? t("player.home.last_played_today")
          : days === 1
            ? t("player.home.last_played_yesterday")
            : t("player.home.last_played_days", { days }),
      );
    }
    return parts.filter((p) => p !== "").join(" · ");
  });
  const eyebrow = $derived(
    featured?.mostPlayed
      ? branch === ""
        ? t("player.home.eyebrow_most_played_no_branch")
        : t("player.home.eyebrow_most_played", { branch })
      : branch === ""
        ? t("player.home.eyebrow_featured_no_branch")
        : t("player.home.eyebrow_featured", { branch }),
  );

  /** Play now was activated and the launch has not come back: the video is already stopped. */
  let starting = $state(false);

  async function play(game: GameCard) {
    if (starting) return;
    starting = true;
    try {
      await onlaunch(game);
    } finally {
      starting = false;
    }
  }

  // Game details: a `card` panel, closed by Esc (focus returns to the button that opened it).
  let open = $state(false);
  let opener: HTMLButtonElement | undefined = $state();
  let closer: HTMLButtonElement | undefined = $state();
  let rowEl: HTMLElement | undefined = $state();

  async function toggleDetails() {
    open = !open;
    if (open) {
      await tick();
      closer?.focus();
    }
  }

  function closeDetails() {
    open = false;
    opener?.focus();
  }

  function onWindowKey(e: KeyboardEvent) {
    if (!open || e.key !== "Escape" || e.defaultPrevented) return;
    e.preventDefault();
    closeDetails();
  }

  // The panel belongs to the game it was opened for; another featured game closes it.
  $effect(() => {
    void hero?.id;
    open = false;
  });

  // Left and Right move focus along the featured row on top of Tab (hidden tiles are skipped).
  function move(e: KeyboardEvent) {
    if ((e.key !== "ArrowLeft" && e.key !== "ArrowRight") || !rowEl) return;
    const items = [...rowEl.querySelectorAll<HTMLElement>("button")].filter(
      (el) => el.offsetParent !== null,
    );
    const at = items.indexOf(document.activeElement as HTMLElement);
    if (at < 0) return;
    const to = at + (e.key === "ArrowLeft" ? -1 : 1);
    if (to < 0 || to >= items.length) return;
    e.preventDefault();
    items[to].focus();
  }
</script>

<svelte:window onkeydown={onWindowKey} />

{#if games !== null}
  <section class="home" aria-label={t("player.home.hero_label")} data-testid="home">
    <!-- The hero: video, else cover, else `bg`, under the scrim, behind the whole shell. Its copy
         stays in the scrim's zone (left 45% × bottom 50%). -->
    <div class="stage" data-testid="hero">
      <GameMedia
        class="hero-media"
        scrim="hero"
        video={media.video}
        cover={media.cover}
        gameRunning={gameRunning || starting}
        bind:paused
        data-testid="hero-media"
      >
        <div class="hero-copy" data-testid="hero-copy">
          {#if hero === null}
            <p class="none type-kiosk-title" data-testid="home-empty">{t("player.shell.empty")}</p>
          {:else}
            <p class="eyebrow type-kiosk-eyebrow" data-testid="hero-eyebrow">{eyebrow}</p>
            <h2 class="title type-kiosk-display" data-testid="hero-title">{hero.name}</h2>
            {#if meta !== ""}
              <p class="meta type-kiosk-body" data-testid="hero-meta">{meta}</p>
            {/if}
            <div class="actions">
              <button
                type="button"
                class="play type-kiosk-cta"
                data-testid="hero-play"
                onclick={() => void play(hero)}
              >
                {t("player.home.play_now")}
              </button>
              <button
                type="button"
                class="details type-kiosk-label"
                aria-haspopup="dialog"
                aria-expanded={open}
                aria-controls={open ? "game-details" : undefined}
                data-testid="hero-details"
                bind:this={opener}
                onclick={() => void toggleDetails()}
              >
                {t("player.home.game_details")}
              </button>
            </div>
          {/if}
        </div>
      </GameMedia>
    </div>

    {#if row.length > 0}
      <nav class="featured" aria-label={t("player.home.featured_label")} data-testid="featured-row">
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <ul class="row" bind:this={rowEl} onkeydown={move}>
          {#each row as game, i (game.id)}
            <li class:lead={i === 0}>
              <GameTile
                name={game.name}
                cover={mediaOf(view, game).cover}
                running={game.running}
                gameRunning={gameRunning || starting}
                featured={i === 0}
                onactivate={() => void play(game)}
              />
            </li>
          {/each}
          <li class="all">
            <button
              type="button"
              class="all-games-tile type-kiosk-tile-name"
              data-testid="all-games-tile"
              onclick={onopengames}
            >
              <svg
                viewBox="0 0 24 24"
                width="40"
                height="40"
                aria-hidden="true"
                fill="currentColor"
              >
                <path d="M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z" />
              </svg>
              <span>{t("player.home.all_games")}</span>
            </button>
          </li>
        </ul>
      </nav>
    {/if}

    {#if open && hero !== null}
      <div
        id="game-details"
        class="panel"
        role="dialog"
        aria-modal="false"
        aria-labelledby="game-details-title"
        data-testid="game-details"
      >
        <h3 id="game-details-title" class="type-kiosk-title">
          {t("player.home.details_title", { name: hero.name })}
        </h3>
        <dl class="facts type-kiosk-body">
          <dt>{t("player.home.details_genre")}</dt>
          <dd data-testid="details-genre">
            {hero.category.trim() || t("player.home.details_unknown")}
          </dd>
          <dt>{t("player.home.details_age")}</dt>
          <dd data-testid="details-age">
            {hero.ageRating.trim() || t("player.home.details_unknown")}
          </dd>
          <dt>{t("player.home.details_launcher")}</dt>
          <dd data-testid="details-launcher">
            {launcherName(hero.launcher) || t("player.home.details_unknown")}
          </dd>
        </dl>
        <button
          type="button"
          class="close type-kiosk-label"
          data-testid="details-close"
          bind:this={closer}
          onclick={closeDetails}
        >
          {t("player.home.details_close")}
        </button>
      </div>
    {/if}
  </section>
{/if}

<style>
  .home {
    width: 100%;
  }

  /* Behind the shell's bar and copy (the shell isolates its own stacking context). */
  .stage {
    position: fixed;
    inset: 0;
    z-index: -1;
    pointer-events: auto;
  }

  .stage :global(.hero-media) {
    position: absolute;
    inset: 0;
  }

  .hero-copy {
    box-sizing: border-box;
    width: 100%;
    max-width: 864px;
    /* The copy zone has 16px of padding already: the rest lines it up with the 56px margin. */
    padding: 0 0 calc(72px - var(--space-4)) calc(var(--space-kiosk-margin) - var(--space-4));
    text-align: left;
  }

  .hero-copy > :global(*) {
    margin: 0;
  }

  .eyebrow {
    margin-bottom: var(--space-3);
    color: var(--text);
    text-transform: uppercase;
  }

  .title {
    display: -webkit-box;
    overflow: hidden;
    overflow-wrap: anywhere;
    color: var(--text);
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
  }

  .meta {
    margin-top: var(--space-3);
    color: var(--text);
  }

  .none {
    color: var(--text);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-4);
    margin-top: var(--space-5);
  }

  .play {
    box-sizing: border-box;
    min-width: 340px;
    min-height: 64px;
    padding: 0 var(--space-6);
    border: 0;
    border-radius: var(--radius-full);
    background-color: var(--button-primary-background);
    color: var(--button-primary-foreground);
    cursor: pointer;
  }

  .details,
  .close {
    box-sizing: border-box;
    min-height: 64px;
    padding: 0 var(--space-5);
    border: var(--button-secondary-border);
    border-radius: var(--radius-full);
    background-color: var(--button-secondary-background);
    color: var(--button-secondary-foreground);
    cursor: pointer;
  }

  .close {
    min-height: var(--space-touch-min);
  }

  .play:focus-visible,
  .details:focus-visible,
  .close:focus-visible,
  .all-games-tile:focus-visible {
    outline: 3px solid var(--focus-ring);
    outline-offset: 3px;
  }

  .featured {
    pointer-events: auto;
    justify-self: start;
    width: fit-content;
    max-width: 100%;
    text-align: left;
  }

  .row {
    display: flex;
    align-items: flex-start;
    gap: var(--space-4);
    margin: 0;
    /* Room for the featured outline and the focus ring around the lead tile. */
    padding: var(--space-4) var(--space-4) var(--space-4) calc(var(--space-4) + 3px);
    list-style: none;
  }

  .row li {
    position: relative;
    flex: none;
  }

  /* The featured tile's own mark: a 3px magenta outline offset 3px (not a focus ring). */
  .row li.lead::after {
    content: "";
    position: absolute;
    inset: -6px;
    border: 3px solid var(--accent);
    border-radius: calc(var(--radius-lg) + 6px);
    pointer-events: none;
  }

  /* Row length follows the width: 4 tiles + All games at 1366, 6 at 1920, 8 at 2560. */
  .row li:nth-child(n + 5):not(.all) {
    display: none;
  }

  @media (min-width: 1920px) {
    .row li:nth-child(n + 5):not(.all) {
      display: block;
    }

    .row li:nth-child(n + 7):not(.all) {
      display: none;
    }
  }

  @media (min-width: 2560px) {
    .row li:nth-child(n + 7):not(.all) {
      display: block;
    }

    .row li:nth-child(n + 9):not(.all) {
      display: none;
    }
  }

  @media (max-width: 1366px) {
    .hero-copy {
      max-width: 720px;
    }
  }

  /* An ultrawide (21:9 and wider): the content is capped at 2560px and centred, and the hero copy
     stays in the left 45% of that content (Story 6.37). */
  @media (min-aspect-ratio: 21/9) {
    .hero-copy {
      max-width: calc((100vw - min(100vw, 2560px)) / 2 + min(100vw, 2560px) * 0.45);
      padding-left: calc(
        (100vw - min(100vw, 2560px)) / 2 + var(--space-kiosk-margin) - var(--space-4)
      );
    }
  }

  .all-games-tile {
    box-sizing: border-box;
    display: grid;
    place-content: center;
    justify-items: center;
    gap: var(--space-3);
    width: 140px;
    height: 196px;
    padding: var(--space-3);
    border: var(--all-games-tile-border);
    border-radius: var(--radius-lg);
    background-color: var(--all-games-tile-background);
    color: var(--all-games-tile-foreground);
    cursor: pointer;
    font: inherit;
    text-align: center;
  }

  .panel {
    position: fixed;
    right: var(--space-kiosk-margin);
    bottom: 160px;
    z-index: 5;
    box-sizing: border-box;
    width: min(420px, 40vw);
    padding: var(--space-5);
    border: var(--card-border);
    border-radius: var(--radius-xl);
    background-color: var(--card-background);
    color: var(--text);
    box-shadow: var(--elevation-menu);
    pointer-events: auto;
    text-align: left;
  }

  .panel h3 {
    margin: 0 0 var(--space-4);
    overflow-wrap: anywhere;
  }

  .facts {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--space-2) var(--space-4);
    margin: 0 0 var(--space-5);
  }

  .facts dt {
    color: var(--text-muted);
  }

  .facts dd {
    margin: 0;
    overflow-wrap: anywhere;
  }
</style>
