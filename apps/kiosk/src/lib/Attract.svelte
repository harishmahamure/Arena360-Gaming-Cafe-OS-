<script lang="ts">
  import { BrandMark, GameMedia, t } from "@arena/ui";
  import type { AttractView, KioskHubState, PlayerContext } from "@arena/ui/generated";
  import { onMount, tick } from "svelte";
  import {
    ATTRACT_VIDEO_MS,
    LOGIN_IDLE_MS,
    canLogIn,
    mediaAt,
    mediaUrl,
    nextMedia,
    rateText,
    ruleChipText,
  } from "./attract";
  import { playerLogout } from "./bridge";
  import Login from "./Login.svelte";
  import PlanPicker from "./PlanPicker.svelte";

  let {
    view,
    logo,
    hub = null,
  }: {
    view: AttractView | null;
    logo: string | null;
    /** The Hub link as the Kiosk service reports it (`null` until it says). */
    hub?: KioskHubState | null;
  } = $props();

  const MODIFIERS = ["Control", "Alt", "Shift", "Meta", "AltGraph"];

  /** Which curated video plays now. */
  let index = $state(0);
  /** The login panel is open over the media. */
  let panel = $state(false);
  /**
   * The player who logged in, while the plan picker is open (Stories 6.8 and 6.9). Memory only: it
   * is never saved, and it goes with the panel or when a session starts (the session screen
   * replaces this whole screen). The Hub holds the login; this is only who to greet.
   */
  let player: PlayerContext | null = $state(null);
  /** The logo could not be decoded: the Arena 360 mark shows instead. */
  let brokenLogo: string | null = $state(null);
  let idle: ReturnType<typeof setTimeout> | undefined;
  let dialog: HTMLElement | undefined = $state();
  let root: HTMLElement | undefined = $state();

  const count = $derived(view?.media.length ?? 0);
  const current = $derived(view ? mediaAt(view.media, index) : null);
  const offered = $derived(canLogIn(view));
  const rate = $derived(rateText(view));
  const chip = $derived(
    view?.state === "ready" && view.rule ? ruleChipText(view.rule, view.locale) : null,
  );
  const showLogo = $derived(logo !== null && logo !== brokenLogo);

  // A different list of videos starts again from its first item.
  const mediaKey = $derived(view?.media.map((m) => m.video).join(",") ?? "");
  $effect(() => {
    void mediaKey;
    index = 0;
  });

  // Each video plays for ATTRACT_VIDEO_MS, then the next; a single item keeps playing.
  $effect(() => {
    void index;
    if (count <= 1) return;
    const timer = setTimeout(() => (index = nextMedia(index, count)), ATTRACT_VIDEO_MS);
    return () => clearTimeout(timer);
  });

  /** Closes the panel; a logged-in player is forgotten with it, here and at the Hub. */
  function close() {
    clearTimeout(idle);
    if (player !== null) playerLogout();
    panel = false;
    player = null;
  }

  /** A correct login: the plan picker opens for this player, with its own 60 s without input. */
  function hold(who: PlayerContext) {
    // The panel closed while the Hub was still checking: nobody is waiting for this answer.
    if (!panel) return;
    player = who;
    touch();
    void tick().then(() => dialog?.focus());
  }

  /** Input while the panel is open keeps it open another LOGIN_IDLE_MS. */
  function touch() {
    clearTimeout(idle);
    idle = setTimeout(close, LOGIN_IDLE_MS);
  }

  function open() {
    if (!offered) return;
    panel = true;
    touch();
  }

  /**
   * Whether input came to this screen: on the page itself or inside the screen. Typing in
   * another surface's field (the admin-exit PIN) is not for the login panel.
   */
  function mine(target: EventTarget | null): boolean {
    if (!(target instanceof Node)) return true;
    return (
      target === document ||
      target === document.documentElement ||
      target === document.body ||
      (root?.contains(target) ?? false)
    );
  }

  function onKeyDown(e: KeyboardEvent) {
    if (!mine(e.target)) return;
    if (panel) {
      // Esc closes the form; the picker is left with "Not you? Log out" or the idle 60 s.
      if (e.key === "Escape" && player === null) {
        e.preventDefault();
        close();
      } else {
        touch();
      }
      return;
    }
    // The hidden admin-exit combination is handled (and stopped) by the layout first, and a
    // modifier alone is the start of a combination, not a request to log in.
    // The key that wakes the screen is not typed into the form that opens.
    if (offered && !e.defaultPrevented && !MODIFIERS.includes(e.key)) {
      e.preventDefault();
      open();
    }
  }

  function onPointer(e: PointerEvent) {
    if (!mine(e.target)) return;
    if (panel) touch();
    else open();
  }

  function onMove() {
    if (panel) touch();
  }

  // The panel takes focus when it opens (the form puts it on the username field itself).
  $effect(() => {
    if (panel && !dialog?.contains(document.activeElement)) dialog?.focus();
  });

  // A panel open when the PC stops being offered (maintenance, no rate) closes at once.
  $effect(() => {
    if (!offered && panel) close();
  });

  onMount(() => {
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("pointerdown", onPointer);
    window.addEventListener("pointermove", onMove);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("pointerdown", onPointer);
      window.removeEventListener("pointermove", onMove);
      clearTimeout(idle);
    };
  });
</script>

<main class="attract" data-testid="attract" bind:this={root}>
  <!-- A new item is a new player: a changed source on one element would not start playing. -->
  {#key current?.video ?? ""}
    <GameMedia
      class="media"
      data-testid="attract-media"
      video={current ? mediaUrl(current.video) : null}
      cover={current?.cover ? mediaUrl(current.cover) : null}
      scrim="hero"
    >
      <div class="copy-body">
        <h1 class="branch type-kiosk-headline" data-testid="attract-branch">
          {view?.branch_name ?? t("app.name")}
        </h1>
        {#if rate}
          <p class="rate type-kiosk-title tabular-nums" data-testid="attract-rate">{rate}</p>
          {#if chip}
            <p
              class="chip-rule type-kiosk-label"
              data-testid="chip-rule"
              aria-label={t("player.attract.rule_label")}
            >
              <span aria-hidden="true">⚡</span>
              {chip}
            </p>
          {/if}
          <p class="hint type-kiosk-body">{t("player.attract.start_hint")}</p>
        {:else}
          <p class="rate type-kiosk-title" data-testid="attract-ask">
            {t("player.attract.ask_counter")}
          </p>
        {/if}
      </div>
    </GameMedia>
  {/key}

  <div class="logo-slot" data-testid="logo-slot">
    {#if showLogo}
      <img
        src={logo}
        alt={t("player.attract.logo_alt", { name: view?.branch_name ?? t("app.name") })}
        draggable="false"
        onerror={() => (brokenLogo = logo)}
      />
    {:else}
      <BrandMark size={72} />
    {/if}
  </div>

  {#if panel}
    <div class="scrim" data-testid="login-scrim">
      <div
        class="login-panel"
        role="dialog"
        aria-modal="true"
        aria-labelledby="login-title"
        tabindex="-1"
        data-testid="login-panel"
        bind:this={dialog}
      >
        {#if player}
          <PlanPicker {view} {hub} {player} onlogout={close} />
        {:else}
          <Login {view} {hub} onplayer={hold} onclose={close} />
        {/if}
      </div>
    </div>
  {/if}
</main>

<style>
  .attract {
    position: relative;
    min-height: 100vh;
    color: var(--text);
  }

  .attract :global(.media) {
    position: absolute;
    inset: 0;
  }

  .copy-body {
    display: grid;
    gap: var(--space-3);
    justify-items: start;
    text-align: start;
  }

  .copy-body > * {
    margin: 0;
  }

  .branch {
    max-width: 100%;
    overflow-wrap: anywhere;
  }

  .hint {
    color: var(--text-muted);
  }

  /* A rule in effect: tinted, with a glyph and words (never colour alone). */
  .chip-rule {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-4);
    border-radius: var(--radius-full);
    background-color: var(--chip-rule-background);
    color: var(--chip-rule-foreground);
  }

  .logo-slot {
    position: absolute;
    top: var(--space-kiosk-margin);
    left: var(--space-kiosk-margin);
    display: flex;
    align-items: center;
    max-width: 40vw;
    max-height: 144px;
  }

  .logo-slot img {
    display: block;
    max-width: 100%;
    max-height: 144px;
    object-fit: contain;
    user-select: none;
  }

  .scrim {
    position: absolute;
    inset: 0;
    z-index: 5;
    display: grid;
    place-content: center;
    padding: var(--space-kiosk-margin);
    background-color: color-mix(in srgb, var(--scrim-base) 70%, transparent);
  }

  :global([data-theme="hc"]) .scrim {
    background-color: var(--bg);
  }

  .login-panel {
    display: grid;
    gap: var(--space-4);
    justify-items: center;
    min-width: min(32rem, 80vw);
    padding: var(--space-6) var(--space-7);
    border: var(--login-panel-border);
    border-radius: var(--radius-xl);
    background-color: var(--login-panel-background);
    color: var(--login-panel-foreground);
    text-align: center;
  }

  .login-panel:focus {
    outline: none;
  }
</style>
