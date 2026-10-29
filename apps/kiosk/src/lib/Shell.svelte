<script lang="ts">
  import { BrandMark, t } from "@arena/ui";
  import type { SnapshotLocale } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import AccountMenu from "./AccountMenu.svelte";
  import { callStaff } from "./bridge";
  import Games from "./Games.svelte";
  import type { GameCard, LauncherCardData, MostPlayedView, RankedGame } from "./games";
  import Home from "./Home.svelte";
  import { INFO_TOAST_MS } from "./hud";
  import MessagesBell from "./MessagesBell.svelte";
  import VolumeControl from "./VolumeControl.svelte";
  import {
    chipText,
    initialOf,
    isLow,
    playerName,
    type ShellSession,
    TABS,
    type Tab,
  } from "./shell";

  let {
    session,
    logo,
    seconds,
    tab = $bindable("home"),
    status,
    balance = null,
    locale = null,
    connected = false,
    games = null,
    apps = null,
    gameRunning = false,
    launchers = [],
    mostPlayed = [],
    mostPlayedView = null,
    branch = "",
    onlaunch = () => {},
    onlaunchapp = () => {},
    onopenlauncher = () => {},
  }: {
    session: ShellSession;
    /** The branch logo (a `data:` URI), or `null` for the Arena 360 mark. */
    logo: string | null;
    /** Seconds on the chip: left for prepaid, played for postpaid. */
    seconds: number;
    /** The active item; it lives for the session (the page forgets it when the session ends). */
    tab?: Tab;
    /** The grace or warning line, and the order count: shown under the heading on Home. */
    status?: import("svelte").Snippet;
    /** The wallet balance in minor units (profiled players), for the account menu. */
    balance?: number | null;
    /** The branch's money format, once the attract view has told it. */
    locale?: SnapshotLocale | null;
    /** The Hub link is up (the account menu's Hub-dependent rows need it). */
    connected?: boolean;
    /** The Games tab's tiles (Story 6.23); `null` until the service has answered. */
    games?: GameCard[] | null;
    /** The Apps tab's tiles (Story 6.28); `null` until the service has answered. */
    apps?: GameCard[] | null;
    /** A game is running: the shell's signal, wired into every `game-media` inside. */
    gameRunning?: boolean;
    /** The launchers installed on this PC (Story 6.26), for the Games tab's card row. */
    launchers?: LauncherCardData[];
    /** The "Most played here" tiles for the Games tab (Story 6.27); none hides the carousel. */
    mostPlayed?: RankedGame[];
    /** The Hub's most-played answer: Home's featured game, its media and last-played times (Story 6.29). */
    mostPlayedView?: MostPlayedView | null;
    /** The branch's name, for the carousel's caption and Home's eyebrow. */
    branch?: string;
    /** A game tile was activated. */
    onlaunch?: (game: GameCard) => void;
    /** An app tile was activated (Story 6.28). */
    onlaunchapp?: (app: GameCard) => void;
    /** A launcher card was activated. */
    onopenlauncher?: (launcher: LauncherCardData) => void;
  } = $props();

  const LABELS: Record<Tab, MessageKey> = {
    home: "player.shell.tab.home",
    games: "player.shell.tab.games",
    apps: "player.shell.tab.apps",
  };

  let brokenLogo: string | null = $state(null);
  /** The account menu is open (the time chip or the account chip opens it). */
  let menuOpen = $state(false);
  /** The chip that opened it, so Esc can hand focus back. */
  let opener: HTMLElement | null = null;
  let bell: MessagesBell | undefined = $state();
  let items: HTMLButtonElement[] = $state([]);
  /** The item with the roving focus; Enter or a click makes it the active one. */
  let focused: Tab = $state("home");
  /** The Home hero video was paused: it holds its frame for the rest of the session (Story 6.29). */
  let heroPaused = $state(false);

  const name = $derived(playerName(session));
  const low = $derived(isLow(session, seconds));
  const time = $derived(chipText(seconds));
  const showLogo = $derived(logo !== null && logo !== brokenLogo);

  /** Call staff was taken by the Hub; the button says so for as long as an info toast stays. */
  let notified = $state(false);
  let calling = false;
  let notifiedTimer: ReturnType<typeof setTimeout> | undefined;

  async function call() {
    if (calling) return;
    calling = true;
    try {
      if (await callStaff()) {
        notified = true;
        clearTimeout(notifiedTimer);
        notifiedTimer = setTimeout(() => (notified = false), INFO_TOAST_MS);
      }
    } finally {
      calling = false;
    }
  }

  function toggleMenu(event: MouseEvent) {
    opener = event.currentTarget as HTMLElement;
    menuOpen = !menuOpen;
  }

  function closeMenu(refocus: boolean) {
    menuOpen = false;
    if (refocus) opener?.focus();
  }

  function onkeydown(event: KeyboardEvent, index: number) {
    if (event.altKey || event.ctrlKey || event.metaKey) return;
    const last = TABS.length - 1;
    const next =
      event.key === "ArrowRight"
        ? index === last
          ? 0
          : index + 1
        : event.key === "ArrowLeft"
          ? index === 0
            ? last
            : index - 1
          : -1;
    if (next === -1) return;
    event.preventDefault();
    focused = TABS[next];
    items[next]?.focus();
  }
</script>

<div class="shell" data-testid="shell">
  <header class="bar">
    <div class="logo-slot" data-testid="shell-logo">
      {#if showLogo}
        <img
          src={logo}
          alt={t("player.attract.logo_alt", { name: t("app.name") })}
          draggable="false"
          onerror={() => (brokenLogo = logo)}
        />
      {:else}
        <BrandMark size={48} />
      {/if}
    </div>

    <nav class="pill-nav" aria-label={t("player.shell.nav_label")} data-testid="pill-nav">
      {#each TABS as id, index (id)}
        <button
          bind:this={items[index]}
          type="button"
          class="item type-kiosk-label"
          class:active={tab === id}
          aria-current={tab === id ? "page" : undefined}
          tabindex={focused === id ? 0 : -1}
          data-testid={"tab-" + id}
          onfocus={() => (focused = id)}
          onclick={() => (tab = id)}
          onkeydown={(e) => onkeydown(e, index)}
        >
          {t(LABELS[id])}
        </button>
      {/each}
    </nav>

    <div class="right">
      <VolumeControl />
      <MessagesBell bind:this={bell} />
      <button
        type="button"
        class="call-staff type-kiosk-label"
        aria-live="polite"
        data-testid="call-staff-button"
        onclick={call}
      >
        <span class="mark" aria-hidden="true">?</span>
        <span data-testid="call-staff-label">
          {t(notified ? "player.call_staff.notified" : "player.call_staff.label")}
        </span>
      </button>
      <button
        type="button"
        class="time-chip"
        class:low
        aria-haspopup="dialog"
        aria-expanded={menuOpen}
        data-account-opener
        onclick={toggleMenu}
        aria-label={t(
          low
            ? "player.shell.chip_low"
            : session.prepaid
              ? "player.shell.chip_left"
              : "player.shell.chip_played",
          { time },
        )}
        data-testid="time-chip"
      >
        <span class="dot" aria-hidden="true" data-testid="time-chip-dot">{low ? "⚠" : ""}</span>
        <span class="num type-hud-numeric tabular-nums" data-testid="session-clock">{time}</span>
      </button>
      <button
        type="button"
        class="account type-kiosk-label"
        aria-haspopup="dialog"
        aria-expanded={menuOpen}
        data-account-opener
        data-testid="account-chip"
        onclick={toggleMenu}
      >
        <span class="initial" aria-hidden="true">{initialOf(name)}</span>
        <span class="who" data-testid="account-name">{name}</span>
      </button>
    </div>
    {#if menuOpen}
      <AccountMenu
        {name}
        profiled={session.username !== null}
        prepaid={session.prepaid}
        {seconds}
        {balance}
        {locale}
        {connected}
        onclose={closeMenu}
        onmessages={() => void bell?.show()}
      />
    {/if}
  </header>

  <main class="content" class:on-home={tab === "home"} aria-label={t(LABELS[tab])}>
    <h1 class="type-kiosk-headline" data-testid="session-device">{session.device}</h1>
    {@render status?.()}
    {#if tab === "games"}
      <Games {games} {gameRunning} {launchers} {mostPlayed} {branch} {onlaunch} {onopenlauncher} />
    {:else if tab === "apps"}
      <Games kind="apps" games={apps} {gameRunning} onlaunch={onlaunchapp} />
    {:else}
      <Home
        {games}
        view={mostPlayedView}
        {branch}
        {gameRunning}
        bind:paused={heroPaused}
        {onlaunch}
        onopengames={() => (tab = "games")}
      />
    {/if}
  </main>
</div>

<style>
  .shell {
    /* Its own stacking context: the Home hero sits behind everything in it, above the page. */
    isolation: isolate;
    display: grid;
    grid-template-rows: auto 1fr;
    min-height: 100vh;
    padding: 32px var(--space-kiosk-margin) var(--space-kiosk-margin);
    color: var(--text);
  }

  .bar {
    position: relative;
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    gap: var(--space-5);
    height: 64px;
  }

  .logo-slot {
    display: flex;
    align-items: center;
    max-width: 30vw;
    height: 64px;
  }

  .logo-slot img {
    display: block;
    max-width: 100%;
    max-height: 64px;
    object-fit: contain;
    user-select: none;
  }

  .pill-nav {
    display: inline-flex;
    gap: var(--space-1);
    padding: var(--space-1);
    border: var(--pill-nav-border);
    border-radius: var(--radius-full);
    background-color: var(--pill-nav-background);
  }

  .item {
    min-width: 96px;
    min-height: 48px;
    padding: 0 var(--space-5);
    border: 0;
    border-radius: var(--radius-full);
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
  }

  .item.active {
    background-color: var(--pill-nav-item-active-background);
    color: var(--pill-nav-item-active-foreground);
  }

  .item:focus-visible,
  .item:focus {
    outline: 3px solid var(--focus-ring);
    outline-offset: 2px;
  }

  .right {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--space-4);
    min-width: 0;
  }

  .time-chip,
  .account {
    margin: 0;
    font: inherit;
    cursor: pointer;
  }

  .time-chip:focus-visible,
  .time-chip:focus,
  .account:focus-visible,
  .account:focus {
    outline: 3px solid var(--focus-ring);
    outline-offset: 2px;
  }

  .call-staff {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    min-height: var(--space-touch-min);
    padding: 0 var(--space-4);
    border: var(--call-staff-button-border);
    border-radius: var(--radius-full);
    background-color: var(--call-staff-button-background);
    color: var(--call-staff-button-foreground);
    cursor: pointer;
    white-space: nowrap;
  }

  .call-staff:focus-visible,
  .call-staff:focus {
    outline: 3px solid var(--focus-ring);
    outline-offset: 2px;
  }

  .mark {
    display: inline-grid;
    place-content: center;
    width: 24px;
    height: 24px;
    border: 2px solid currentColor;
    border-radius: var(--radius-full);
    line-height: 1;
  }

  .time-chip {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    min-height: 48px;
    padding: 0 var(--space-4);
    border: 2px solid transparent;
    border-radius: var(--radius-full);
    background-color: var(--time-chip-background);
    color: var(--time-chip-foreground);
  }

  /* The status dot: a solid disc, replaced by a ⚠ glyph (and a 2px warning border) when low. */
  .dot {
    display: inline-grid;
    place-content: center;
    width: 12px;
    height: 12px;
    border-radius: var(--radius-full);
    background-color: var(--time-chip-indicator);
    line-height: 1;
  }

  .time-chip.low {
    border-color: var(--time-chip-indicator-low);
  }

  .time-chip.low .dot {
    width: auto;
    height: auto;
    background: none;
    color: var(--time-chip-indicator-low);
  }

  .account {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
    padding: 0;
    border: 0;
    border-radius: var(--radius-full);
    background: transparent;
    color: inherit;
  }

  .initial {
    display: inline-grid;
    place-content: center;
    width: 40px;
    height: 40px;
    border-radius: var(--radius-full);
    background-color: var(--surface);
    border: 1px solid var(--line);
  }

  .who {
    max-width: 14ch;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .content {
    display: grid;
    /* One column that never grows past the content area, so a row that scrolls sideways (the
       most-played carousel) scrolls instead of widening the page. */
    grid-template-columns: minmax(0, 1fr);
    align-content: start;
    justify-items: center;
    gap: var(--space-5);
    padding-top: var(--space-8);
    text-align: center;
  }

  .content > :global(*) {
    margin: 0;
  }

  /* Home: the hero is behind the content, so the empty content lets clicks through to it; the
     content's own parts (the device name, the status lines, the featured row) still take them. */
  .content.on-home {
    pointer-events: none;
  }

  .content.on-home > :global(*) {
    pointer-events: auto;
  }
</style>
