<script lang="ts">
  import { BrandMark, InlinePin, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { KioskHubState } from "@arena/ui/generated";
  import { afterNavigate, goto } from "$app/navigation";
  import { page } from "$app/state";
  import { onMount, type Snippet } from "svelte";
  import {
    adminExit,
    applyTheme,
    currentPcScreen,
    currentTheme,
    dismissPcMessage,
    onPcScreen,
    onTheme,
    adminExitStatus,
    isAdminExitCombo,
    onAdminExit,
    onAdminExitEnded,
    currentAttract,
    currentHubStatus,
    onAttract,
    onHubStatus,
    onRestoreState,
    reportAppState,
    restoredRoute,
    startHeartbeat,
    takeRestoreState,
  } from "./bridge";
  import Discovery from "./Discovery.svelte";
  import { installKioskGuards, placeOnPrimary } from "./guards";

  let { children }: { children: Snippet } = $props();

  /** The window-filling stage; positioned over the primary monitor when the shell says where. */
  let stage: HTMLDivElement;

  /** The Hub link as the Kiosk service reports it (unknown until it says). */
  let hub: KioskHubState | null = $state(null);
  let justPaired = $state(false);
  const showDiscovery = $derived(hub === "unpaired" || (justPaired && hub !== "connected"));

  $effect(() => {
    if (justPaired && hub === "connected") justPaired = false;
  });

  /**
   * The hidden admin exit (Story 1.20): Ctrl+Alt+Shift+A opens a manager PIN panel (nothing on
   * screen says it exists). While an exit runs the window is the small corner badge and shows
   * only the countdown.
   */
  let locked = $state(false);
  let message: string | null = $state(null);
  /** The branch logo for the lock screen's `logo-slot` (a `data:` URI; the mark when none). */
  let logo: string | null = $state(null);
  let brokenLogo: string | null = $state(null);
  let reason: string | null = $state(null);
  const REASONS: Record<string, MessageKey> = {
    pay_at_counter: "player.lock.pay_at_counter",
    ended_by_staff: "player.lock.ended_by_staff",
    times_up: "player.lock.times_up",
    times_up_see_counter: "player.lock.times_up_see_counter",
    locked_by_staff: "player.pc.locked",
    paused: "player.lock.paused",
    preparing_pc: "player.lock.preparing_pc",
  };
  let adminPanel = $state(false);
  let exitUntil: number | null = $state(null);
  let now = $state(Date.now());

  /** "14:59": whole minutes and seconds left, rounded up. */
  function countdown(until: number, at: number): string {
    const secs = Math.max(0, Math.ceil((until - at) / 1000));
    return `${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, "0")}`; // i18n-ignore (a clock)
  }

  async function submitPin(pin: string) {
    const result = await adminExit(pin);
    if (result.kind === "granted") {
      adminPanel = false;
      exitUntil = result.until;
      return { ok: true } as const;
    }
    if (result.kind === "locked") {
      return { ok: false, message: "ui.pin.locked", retryAfterS: result.retryAfterS } as const;
    }
    if (result.kind === "rejected") return { ok: false, message: "ui.pin.rejected" } as const;
    return { ok: false, message: "ui.pin.failed" } as const;
  }

  function onKeyDown(e: KeyboardEvent) {
    if (!isAdminExitCombo(e) || exitUntil !== null) return;
    e.preventDefault();
    adminPanel = true;
  }

  /** Where the player is: the path and query. */
  const here = () => page.url.pathname + page.url.search;

  function restore(state: unknown) {
    const route = restoredRoute(state);
    if (route && route !== here()) void goto(route, { replaceState: true });
  }

  onMount(() => {
    const tick = setInterval(() => (now = Date.now()), 1000);
    window.addEventListener("keydown", onKeyDown, { capture: true });
    const stops: Array<() => void> = [
      startHeartbeat(),
      installKioskGuards(),
      placeOnPrimary(stage),
      () => clearInterval(tick),
      () => window.removeEventListener("keydown", onKeyDown, { capture: true }),
    ];
    let gone = false;
    void (async () => {
      const add = (stop: () => void) => (gone ? stop() : stops.push(stop));
      // A `theme` event is newer than the reply to the `theme` command asked below.
      let themed = false;
      let screened = false;
      add(
        await onTheme((hc) => {
          themed = true;
          applyTheme(hc);
        }),
      );
      add(await onHubStatus((s) => (hub = s.state)));
      add(
        await onPcScreen((s) => {
          screened = true;
          locked = s.locked;
          reason = s.reason;
          message = s.message;
        }),
      );
      add(await onAttract((s) => (logo = s.logo)));
      const shown = await currentAttract();
      if (shown && logo === null) logo = shown.logo;
      add(
        await onAdminExit((until) => {
          adminPanel = false;
          exitUntil = until;
        }),
      );
      add(await onAdminExitEnded(() => (exitUntil = null)));
      const status = await adminExitStatus();
      if (status.until !== null && exitUntil === null) exitUntil = status.until;
      add(
        await onRestoreState((state) => {
          restore(state);
          // Delivered: clear the shell's copy so a reload does not restore it again.
          void takeRestoreState();
        }),
      );
      const screen = await currentPcScreen();
      if (screen && !screened) {
        locked = screen.locked;
        reason = screen.reason;
        message = screen.message;
      }
      const hc = await currentTheme();
      if (hc !== null && !themed) applyTheme(hc);
      const known = await currentHubStatus();
      if (known && hub === null) hub = known.state;
      const pending = await takeRestoreState();
      if (pending) restore(pending);
    })();
    return () => {
      gone = true;
      for (const stop of stops) stop();
    };
  });

  afterNavigate(() => reportAppState({ route: here() }));
</script>

<div class="kiosk" bind:this={stage} hidden={exitUntil !== null}>
  <div class="app" inert={locked}>
    {#if showDiscovery}
      <Discovery onpaired={() => (justPaired = true)} />
    {:else}
      {@render children()}
    {/if}
  </div>
  {#if locked}
    <div class="cover" role="status" data-testid="pc-locked">
      <div class="logo-slot" data-testid="lock-logo">
        {#if logo !== null && logo !== brokenLogo}
          <img
            src={logo}
            alt={t("player.attract.logo_alt", { name: t("app.name") })}
            draggable="false"
            onerror={() => (brokenLogo = logo)}
          />
        {:else}
          <BrandMark size={72} />
        {/if}
      </div>
      <h1 class="type-kiosk-headline">{t((reason && REASONS[reason]) || "player.pc.locked")}</h1>
    </div>
  {/if}
  {#if adminPanel}
    <div class="admin-exit" data-testid="admin-exit-panel">
      <InlinePin
        message="player.admin_exit.title"
        onsubmit={submitPin}
        oncancel={() => (adminPanel = false)}
      />
    </div>
  {/if}
</div>

{#if exitUntil !== null}
  <!-- The admin exit's countdown: the whole (shrunken, non-topmost) window. -->
  <div class="badge" role="status" data-testid="admin-exit-badge">
    <p class="type-kiosk-label">
      {t("player.admin_exit.badge", { countdown: countdown(exitUntil, now) })}
    </p>
  </div>
{/if}

<!-- Non-blocking: a quiet pill while the Hub is unreachable, announced politely. -->
<div class="link" role="status" aria-live="polite" data-testid="link-status">
  {#if hub === "reconnecting"}
    <span class="pill type-kiosk-label">{t("player.hub.reconnecting")}</span>
  {:else if hub === "token_rejected"}
    <span class="pill type-kiosk-label">{t("player.hub.token_rejected")}</span>
  {/if}
</div>

<!--
  Persistent, polite and never focus-stealing: announced through aria-live, focus stays where
  it was, and only its button dismisses it. The region itself lets clicks through.
-->
<div class="toasts" aria-live="polite" data-testid="toasts">
  {#if message}
    <div class="toast" data-testid="pc-message">
      <div>
        <p class="type-kiosk-label">{t("player.pc.message_title")}</p>
        <p class="type-kiosk-body">{message}</p>
      </div>
      <button
        type="button"
        class="dismiss type-kiosk-label"
        onclick={() => {
          message = null;
          dismissPcMessage();
        }}
      >
        {t("common.dismiss")}
      </button>
    </div>
  {/if}
</div>

<style>
  /* The kiosk window is full screen; the app fills all of it. */
  .kiosk {
    position: fixed;
    inset: 0;
    overflow: auto;
    background-color: var(--bg);
  }

  .app {
    min-height: 100%;
  }

  .cover {
    position: fixed;
    inset: 0;
    z-index: 10;
    display: grid;
    place-content: center;
    padding: var(--space-kiosk-margin);
    background-color: var(--bg);
    color: var(--text);
    text-align: center;
  }

  .cover h1 {
    margin: 0;
  }

  .cover .logo-slot {
    position: absolute;
    top: var(--space-kiosk-margin);
    left: var(--space-kiosk-margin);
    display: flex;
    align-items: center;
    max-width: 40vw;
    max-height: 144px;
  }

  .cover .logo-slot img {
    display: block;
    max-width: 100%;
    max-height: 144px;
    object-fit: contain;
  }

  .admin-exit {
    position: fixed;
    inset: 0;
    z-index: 20;
    display: grid;
    place-content: center;
    padding: var(--space-kiosk-margin);
    background-color: var(--bg);
  }

  .badge {
    position: fixed;
    inset: 0;
    display: grid;
    place-content: center;
    padding: var(--space-3);
    background-color: var(--bg);
    color: var(--text);
  }

  .badge p {
    margin: 0;
  }

  .link {
    position: fixed;
    top: var(--space-kiosk-margin);
    right: var(--space-kiosk-margin);
    pointer-events: none;
  }

  .toasts {
    position: fixed;
    z-index: 15;
    right: 0;
    bottom: var(--space-kiosk-margin);
    left: 0;
    display: flex;
    justify-content: center;
    pointer-events: none;
  }

  .toast {
    display: flex;
    align-items: center;
    gap: var(--space-5);
    max-width: min(48rem, calc(100vw - 2 * var(--space-kiosk-margin)));
    padding: var(--space-4) var(--space-5);
    border: 2px solid var(--hud-toast-title);
    border-radius: var(--radius-md);
    background-color: var(--hud-toast-background);
    box-shadow: var(--elevation-toast);
    color: var(--hud-toast-foreground);
    pointer-events: auto;
  }

  .toast p {
    margin: 0;
    overflow-wrap: anywhere;
  }

  .dismiss {
    flex: none;
    min-height: 48px;
    padding: var(--space-2) var(--space-5);
    border: 2px solid var(--hud-toast-title);
    border-radius: var(--radius-full);
    background: transparent;
    color: var(--hud-toast-foreground);
    cursor: pointer;
  }

  .dismiss:focus-visible {
    outline: 3px solid var(--hud-toast-title);
    outline-offset: 2px;
  }

  /* HC: the shared 4px focus ring and 2px white outline, never the toast's own colours. */
  :global([data-theme="hc"]) .dismiss {
    outline: 2px solid var(--text);
    outline-offset: 0;
  }

  :global([data-theme="hc"]) .dismiss:focus-visible {
    outline: 4px solid var(--focus-ring);
    outline-offset: 2px;
  }

  .pill {
    display: inline-flex;
    align-items: center;
    padding: var(--space-2) var(--space-4);
    border-radius: var(--radius-full);
    background-color: var(--reconnecting-indicator-background);
    color: var(--reconnecting-indicator-foreground);
  }
</style>
