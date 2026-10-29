<script lang="ts">
  import { t } from "@arena/ui";
  import type { KioskHubState } from "@arena/ui/generated";
  import { onMount } from "svelte";
  import {
    currentAttract,
    currentHubStatus,
    currentPcScreen,
    currentTestSession,
    launchGame,
    loadGames,
    onAttract,
    onHubStatus,
    onPcScreen,
    onTestSession,
    onTestSessionEnded,
    type AttractShown,
    type PcScreen,
  } from "../lib/bridge";
  import Attract from "../lib/Attract.svelte";
  import Shell from "../lib/Shell.svelte";
  import { GAMES_POLL_MS, type GameCard } from "../lib/games";
  import { type Tab } from "../lib/shell";

  /** The idle screen: the last attract view and logo (from the service cache at first). */
  let attract: AttractShown = $state({ view: null, logo: null });
  /** The Hub link as the Kiosk service reports it (the login needs it connected). */
  let hub: KioskHubState | null = $state(null);
  /** A lock covers the PC (the layout draws it); the idle screen forgets any held player. */
  let locked = $state(false);
  let endsAt: number | null = $state(null);
  let now = $state(performance.now());
  const left = $derived(endsAt === null ? 0 : Math.max(0, Math.ceil((endsAt - now) / 1000)));
  const clock = $derived([Math.floor(left / 60), String(left % 60).padStart(2, "0")].join(":"));

  let session = $state<{
    name: string;
    from: number;
    warning: number | null;
    graceFrom: number | null;
    graceMs: number;
    ordered: number;
    username: string | null;
    guestNo: number | null;
    /** The profiled player's wallet balance in minor units, for the account menu. */
    balance: number | null;
    /** When a prepaid session runs out, on the monotonic clock (`null`: it counts up). */
    until: number | null;
  } | null>(null);
  /** The shell's active item: it lasts for the session and is forgotten when the session ends. */
  let tab: Tab = $state("home");
  /** The Games tab's tiles and whether any game is running (Story 6.23); asked of the service. */
  let games: GameCard[] | null = $state(null);
  let gameRunning = $state(false);

  async function refreshGames() {
    const view = await loadGames();
    if (view === null) return;
    games = view.games;
    gameRunning = view.running;
  }

  async function launch(game: GameCard) {
    await launchGame(game);
    await refreshGames();
  }

  // While a session is on screen the games and the running signal are asked for now, when the
  // Games tab opens, and every few seconds; with no session there is nothing to show.
  const inSession = $derived(session !== null);
  $effect(() => {
    if (!inSession) {
      games = null;
      gameRunning = false;
      return;
    }
    void tab;
    void refreshGames();
    const timer = setInterval(() => void refreshGames(), GAMES_POLL_MS);
    return () => clearInterval(timer);
  });

  const graceLeft = $derived(
    session === null || session.graceFrom === null
      ? null
      : Math.max(0, Math.ceil((session.graceFrom + session.graceMs - now) / 1000)),
  );
  const inGrace = $derived(
    session !== null && session.graceFrom !== null && now >= session.graceFrom,
  );
  const graceClock = $derived(
    graceLeft === null
      ? ""
      : [Math.floor(graceLeft / 60), String(graceLeft % 60).padStart(2, "0")].join(":"),
  );
  const elapsed = $derived(
    session === null ? 0 : Math.max(0, Math.floor((now - session.from) / 1000)),
  );
  /** What the time chip counts: seconds left (prepaid) or played (postpaid). */
  const chipSeconds = $derived(
    session?.until != null ? Math.max(0, Math.ceil((session.until - now) / 1000)) : elapsed,
  );

  const follow = (s: PcScreen) => {
    now = performance.now();
    locked = s.locked;
    session = s.session
      ? {
          name: s.session.device_name,
          from: now - s.session.elapsed_ms,
          warning: s.session.warning_min,
          graceFrom: s.session.grace_in_ms === null ? null : now + s.session.grace_in_ms,
          graceMs: s.session.grace_ms,
          ordered: s.session.ordered_items,
          username: s.session.username,
          guestNo: s.session.guest_no,
          balance: s.session.username === null ? null : s.session.balance_minor,
          until: s.session.remaining_ms === null ? null : now + s.session.remaining_ms,
        }
      : null;
    if (session === null) tab = "home";
  };

  const start = (remainingMs: number) => {
    now = performance.now();
    endsAt = now + remainingMs;
  };

  onMount(() => {
    const tick = setInterval(() => (now = performance.now()), 250);
    const stops: Array<() => void> = [() => clearInterval(tick)];
    let gone = false;
    void (async () => {
      const add = (stop: () => void) => (gone ? stop() : stops.push(stop));
      // First: the idle screen paints from the service's cache before anything else is asked.
      // An `attract` event is newer than the reply to the `attract` command asked below.
      let attracted = false;
      add(
        await onAttract((s) => {
          attracted = true;
          attract = s;
        }),
      );
      const shown = await currentAttract();
      if (shown && !attracted) attract = shown;
      add(await onHubStatus((s) => (hub = s.state)));
      const link = await currentHubStatus();
      if (link && hub === null) hub = link.state;
      add(await onTestSession(start));
      add(await onTestSessionEnded(() => (endsAt = null)));
      add(await onPcScreen(follow));
      const screen = await currentPcScreen();
      if (screen && session === null) follow(screen);
      const known = await currentTestSession();
      if (known !== null && endsAt === null) start(known);
    })();
    return () => {
      gone = true;
      for (const stop of stops) stop();
    };
  });
</script>

<svelte:head>
  <title>{t("app.title", { surface: t("surface.kiosk") })}</title>
</svelte:head>

{#if endsAt !== null}
  <main class="locked" data-testid="test-session">
    <p class="brand type-kiosk-eyebrow">{t("app.name")}</p>
    <h1 class="type-kiosk-headline">{t("player.test_session.title")}</h1>
    <p class="clock type-kiosk-display tabular-nums" data-testid="test-session-clock">{clock}</p>
    <p class="hint type-kiosk-body">{t("player.test_session.hint")}</p>
  </main>
{:else if session !== null}
  <main class="session" data-testid="session">
    <Shell
      session={{
        device: session.name,
        username: session.username,
        guestNo: session.guestNo,
        prepaid: session.until !== null,
        warningMin: session.warning,
      }}
      logo={attract.logo}
      seconds={chipSeconds}
      balance={session.balance}
      locale={attract.view?.locale ?? null}
      connected={hub === "connected"}
      {games}
      {gameRunning}
      onlaunch={launch}
      bind:tab
    >
      {#snippet status()}
        {#if inGrace}
          <p class="warning type-kiosk-body" role="status" data-testid="session-grace">
            <span aria-hidden="true">⚠</span>
            {t("player.session.grace", { time: graceClock })}
          </p>
        {:else if session?.warning != null}
          <p class="warning type-kiosk-body" role="status" data-testid="session-warning">
            <span aria-hidden="true">⚠</span>
            {t("player.session.warning", { minutes: session.warning })}
          </p>
        {/if}
        {#if session && session.ordered > 0}
          <p class="ordered type-kiosk-body" data-testid="ordered">
            {session.ordered === 1
              ? t("player.session.ordered_one")
              : t("player.session.ordered", { count: session.ordered })}
          </p>
        {/if}
      {/snippet}
    </Shell>
  </main>
{:else}
  <!-- No session yet: the attract screen (Story 6.7), painted from the cache at once. -->
  <Attract view={attract.view} logo={attract.logo} {hub} {locked} />
{/if}

<style>
  .locked {
    display: grid;
    place-content: center;
    gap: var(--space-6);
    min-height: 100vh;
    padding: var(--space-kiosk-margin);
    color: var(--text);
    text-align: center;
  }

  .session {
    min-height: 100vh;
  }

  h1 {
    max-width: 24ch;
    margin: 0 auto;
  }

  .brand,
  .hint,
  .clock,
  .ordered,
  .warning {
    margin: 0;
  }

  .warning {
    color: var(--text);
    font-weight: 600;
  }

  .brand,
  .hint {
    color: var(--text-muted);
  }
</style>
