<script lang="ts">
  import "@arena/ui/base.css";
  import { InlinePin, t } from "@arena/ui";
  import type { KioskHubState } from "@arena/ui/generated";
  import { afterNavigate, goto } from "$app/navigation";
  import { page } from "$app/state";
  import { onMount, type Snippet } from "svelte";
  import {
    adminExit,
    adminExitStatus,
    isAdminExitCombo,
    onAdminExit,
    onAdminExitEnded,
    currentHubStatus,
    onHubStatus,
    onRestoreState,
    onToast,
    reportAppState,
    restoredRoute,
    startHeartbeat,
    takeRestoreState,
    type ToastKey,
  } from "../lib/bridge";
  import { installKioskGuards, placeOnPrimary } from "../lib/guards";

  let { children }: { children: Snippet } = $props();

  /** The window-filling stage; positioned over the primary monitor when the shell says where. */
  let stage: HTMLDivElement;

  /** The Hub link as the Kiosk service reports it (unknown until it says). */
  let hub: KioskHubState | null = $state(null);

  /**
   * The toast on screen: it stays until the player dismisses it, and the same toast again
   * (another blocked app) replaces it rather than stacking.
   */
  let toast: ToastKey | null = $state(null);

  /**
   * The hidden admin exit (Story 1.20): Ctrl+Alt+Shift+A opens a manager PIN panel (nothing on
   * screen says it exists). While an exit runs the window is the small corner badge and shows
   * only the countdown.
   */
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
      add(await onHubStatus((s) => (hub = s.state)));
      add(await onToast((key) => (toast = key)));
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
  {@render children()}
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
  {#if toast}
    <div class="toast" data-testid="toast">
      <p class="type-kiosk-body">{t(toast)}</p>
      <button type="button" class="dismiss type-kiosk-label" onclick={() => (toast = null)}>
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

  .admin-exit {
    position: fixed;
    inset: 0;
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

  .pill {
    display: inline-flex;
    align-items: center;
    padding: var(--space-2) var(--space-4);
    border-radius: var(--radius-full);
    background-color: var(--reconnecting-indicator-background);
    color: var(--reconnecting-indicator-foreground);
  }
</style>
