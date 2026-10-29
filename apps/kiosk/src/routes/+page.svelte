<script lang="ts">
  import { t } from "@arena/ui";
  import type { KioskHubState } from "@arena/ui/generated";
  import { onMount } from "svelte";
  import {
    currentAttract,
    currentHubStatus,
    currentPcScreen,
    currentTestSession,
    launchApp,
    launchGame,
    loadApps,
    loadGames,
    loadLaunchers,
    loadMostPlayed,
    loadShopImage,
    loadShopMenu,
    openLauncher,
    placeOrder,
    onAttract,
    onHubStatus,
    onPcScreen,
    onTestSession,
    onTestSessionEnded,
    type AttractShown,
    type PcScreen,
  } from "../lib/bridge";
  import Attract from "../lib/Attract.svelte";
  import Cart, { type LineProblem } from "../lib/Cart.svelte";
  import Shell from "../lib/Shell.svelte";
  import {
    cartCount,
    chosenPay,
    payOffer,
    refreshLines,
    renote,
    stepCart,
    type CartLine,
    type PayChoice,
  } from "../lib/cart";
  import {
    GAMES_POLL_MS,
    MOST_PLAYED_POLL_MS,
    rankedGames,
    type GameCard,
    type LauncherCardData,
    type MostPlayedView,
  } from "../lib/games";
  import { type Tab } from "../lib/shell";
  import { SHOP_POLL_IDLE_MS, SHOP_POLL_OPEN_MS, type ShopView } from "../lib/shop";

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
  let tab = $state<Tab>("home");
  /** The Games tab's tiles and whether any game is running (Story 6.23); asked of the service. */
  let games: GameCard[] | null = $state(null);
  let gameRunning = $state(false);
  /** The Apps tab's tiles (Story 6.28); `null` until the service has answered. */
  let apps: GameCard[] | null = $state(null);
  /** The launcher cards (Story 6.26): the launchers installed on this PC. */
  let launchers: LauncherCardData[] = $state([]);
  /** The Hub's most-played list (Story 6.27); `null` until asked, or when the Hub is unreachable. */
  let mostPlayed: MostPlayedView | null = $state(null);
  const ranked = $derived(rankedGames(mostPlayed, games));
  /** The Hub's Shop menu (Story 6.30); `null` until asked. The Shop item exists only when it says `enabled`. */
  let shop: ShopView | null = $state(null);
  /** The Shop photos fetched so far by product id; each is asked of the Hub once per session. */
  let shopImages: Record<string, string | null> = $state({});
  /** Product ids whose photo is asked for or held (not reactive: it only dedupes the asks). */
  const photoAsked = new Set<string>();
  /** Bumped when a session ends, so a photo that arrives late is not shown to the next player. */
  let shopEpoch = 0;

  /** The cart (Story 6.31): lives for the session, is forgotten when it ends. */
  let cart: CartLine[] = $state([]);
  let cartOpen = $state(false);
  let cartBusy = $state(false);
  /** The last order went through (the empty cart says so until the player adds something). */
  let cartPlaced: PayChoice | null = $state(null);
  let cartError: "reconnecting" | "refused" | "balance" | null = $state(null);
  /** How the player chose to pay (Story 6.32); the Hub's offer decides what is usable. */
  let cartPick: PayChoice | null = $state(null);
  /** The wallet balance the Hub found when it refused a wallet payment. */
  let cartBalance: number | null = $state(null);
  /** The reason a step or a placement was refused, by product id. */
  let cartProblems: Record<string, LineProblem> = $state({});

  function resetCart() {
    cart = [];
    cartOpen = false;
    cartBusy = false;
    cartPlaced = null;
    cartError = null;
    cartPick = null;
    cartBalance = null;
    cartProblems = {};
  }

  function closeCart() {
    cartOpen = false;
    document.querySelector<HTMLElement>('[data-testid="shop-cart-button"]')?.focus();
  }

  /**
   * One step on a cart line (Add is a step up from nothing). A step up asks the Hub for the menu
   * first, so the stock it is checked against is live; a refusal leaves the cart as it was and
   * says why on the line. The Hub not answering says the counter is reconnecting.
   */
  async function stepLine(id: string, delta: 1 | -1, notes: string[] = []) {
    // A placement in flight owns the cart: a step now would be cleared with the placed lines.
    if (cartBusy) return;
    cartPlaced = null;
    const rest = { ...cartProblems };
    delete rest[id];
    cartProblems = rest;
    let fresh = shop?.items.find((i) => i.id === id);
    if (delta > 0) {
      const view = await loadShopMenu();
      if (view === null) {
        cartError = "reconnecting";
        return;
      }
      cartError = null;
      shop = view;
      fresh = view.items.find((i) => i.id === id);
    }
    const result = stepCart(cart, id, delta, fresh, notes);
    cart = result.lines;
    if (result.problem !== null) cartProblems = { ...cartProblems, [id]: result.problem };
  }

  async function addToCart(item: { id: string }) {
    await stepLine(item.id, 1);
    cartOpen = true;
  }

  async function place(pay: PayChoice) {
    if (cartBusy || cart.length === 0) return;
    cartBusy = true;
    cartError = null;
    cartBalance = null;
    const sent = cart;
    const outcome = await placeOrder(sent, pay);
    cartBusy = false;
    if (outcome.kind === "ok") {
      cart = [];
      cartProblems = {};
      cartPlaced = pay;
    } else if (outcome.kind === "unreachable") {
      cartError = "reconnecting";
    } else if (
      outcome.productId !== null &&
      (outcome.code === "OUT_OF_STOCK" || outcome.code === "BUNDLE_SHORT")
    ) {
      // The cart stays and the short line says why (no override on the kiosk).
      cartProblems = {
        [outcome.productId]:
          outcome.code === "BUNDLE_SHORT" ? { kind: "bundle_short" } : { kind: "out_of_stock" },
      };
    } else if (outcome.code === "INSUFFICIENT_BALANCE" && outcome.balanceMinor !== null) {
      // A concurrent spend left too little: the cart stays and shows the real balance.
      cartBalance = outcome.balanceMinor;
      cartError = "balance";
    } else {
      cartError = "refused";
    }
  }

  async function refreshGames() {
    const [view, cards] = await Promise.all([loadGames(), loadLaunchers()]);
    if (cards !== null) launchers = cards;
    if (view === null) return;
    games = view.games;
    gameRunning = view.running;
  }

  async function refreshApps() {
    const cards = await loadApps();
    if (cards !== null) apps = cards;
  }

  async function launchAppTile(app: GameCard) {
    await launchApp(app);
    await refreshApps();
  }

  async function openOwnAccount(card: LauncherCardData) {
    await openLauncher(card);
    await refreshGames();
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
      launchers = [];
      return;
    }
    void tab;
    void refreshGames();
    const timer = setInterval(() => void refreshGames(), GAMES_POLL_MS);
    return () => clearInterval(timer);
  });

  // The Apps tab's tiles are asked of the service only while that tab is open (now, then every few
  // seconds); a session end forgets them.
  $effect(() => {
    if (!inSession) {
      apps = null;
      return;
    }
    if (tab !== "apps") return;
    // An answer that arrives after this run is over (the session ended, the tab changed) is
    // dropped, so a slow reply never shows an old list to the next player.
    let live = true;
    const refresh = async () => {
      const cards = await loadApps();
      if (live && cards !== null) apps = cards;
    };
    void refresh();
    const timer = setInterval(() => void refresh(), GAMES_POLL_MS);
    return () => {
      live = false;
      clearInterval(timer);
    };
  });

  // The most-played list is asked of the Hub when Home or the Games tab opens and every few
  // minutes while either stays open (the Hub recomputes it at most every 15 minutes): Home's
  // featured game and the Games carousel read it. No answer hides the carousel and Home falls
  // back to the first library game with media (Story 6.29).
  const wantsMostPlayed = $derived(tab === "games" || tab === "home");
  $effect(() => {
    if (!inSession) {
      mostPlayed = null;
      return;
    }
    if (!wantsMostPlayed) return;
    // An answer that arrives after this run is over (the session ended, the tab changed) is dropped,
    // so a slow reply never shows an old carousel to the next player.
    let live = true;
    const refresh = async () => {
      const view = await loadMostPlayed();
      if (live) mostPlayed = view;
    };
    void refresh();
    const timer = setInterval(() => void refresh(), MOST_PLAYED_POLL_MS);
    return () => {
      live = false;
      clearInterval(timer);
    };
  });

  // The Shop menu is asked of the Hub as soon as a session is on screen (the Shop item appears only
  // when orders are enabled), every second while the Shop tab is open so a stock change shows
  // within 2 s, and every 30 s otherwise. One ask at a time; a failed ask keeps the last good menu.
  $effect(() => {
    if (!inSession) {
      shop = null;
      shopImages = {};
      photoAsked.clear();
      shopEpoch++;
      resetCart();
      return;
    }
    const period = tab === "shop" ? SHOP_POLL_OPEN_MS : SHOP_POLL_IDLE_MS;
    let live = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const refresh = async () => {
      const view = await loadShopMenu();
      if (!live) return;
      if (view !== null) {
        shop = view;
        cart = refreshLines(cart, view.items);
        // The Hub answered again: Place order is usable without the player having to tap a step.
        if (cartError === "reconnecting") cartError = null;
      }
      timer = setTimeout(() => void refresh(), period);
    };
    void refresh();
    return () => {
      live = false;
      clearTimeout(timer);
    };
  });

  // A Shop tab left open when the Hub turns orders off is replaced by Home.
  $effect(() => {
    if (tab === "shop" && shop !== null && !shop.enabled) tab = "home";
  });

  // Photos come from the Hub one at a time, once per product for the session, while the Shop tab
  // is open. A failed ask is forgotten so the next menu answer asks again.
  async function fetchPhoto(id: string) {
    const epoch = shopEpoch;
    const image = await loadShopImage(id);
    if (epoch !== shopEpoch) return;
    if (image === undefined) photoAsked.delete(id);
    else shopImages = { ...shopImages, [id]: image };
  }

  $effect(() => {
    if (tab !== "shop" || shop === null) return;
    for (const item of shop.items) {
      if (!item.has_image || photoAsked.has(item.id)) continue;
      photoAsked.add(item.id);
      void fetchPhoto(item.id);
    }
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
      reconnecting={hub === "reconnecting"}
      {games}
      {apps}
      {gameRunning}
      {launchers}
      mostPlayed={ranked}
      mostPlayedView={mostPlayed}
      {shop}
      {shopImages}
      cartCount={cartCount(cart)}
      oncart={() => (cartOpen = true)}
      onshopadd={addToCart}
      branch={attract.view?.branch_name ?? ""}
      onlaunch={launch}
      onlaunchapp={launchAppTile}
      onopenlauncher={openOwnAccount}
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
    {#if cartOpen && shop?.enabled}
      <Cart
        lines={cart}
        problems={cartProblems}
        locale={attract.view?.locale ?? null}
        pc={session.name}
        zone={attract.view?.zone ?? ""}
        connected={hub === "connected"}
        busy={cartBusy}
        placed={cartPlaced}
        error={cartError}
        offer={payOffer(shop.pay)}
        picked={cartPick}
        balance={cartBalance}
        onpick={(pay) => (cartPick = pay)}
        onstep={(id, delta, notes) => void stepLine(id, delta, notes)}
        onnotes={(id, from, to) => {
          if (!cartBusy) cart = renote(cart, id, from, to);
        }}
        onplace={(pay) => void place(pay)}
        onclose={closeCart}
      />
    {/if}
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
