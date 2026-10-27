<script lang="ts">
  import "@arena/ui/base.css";
  import { AlertBadge, Button, StatusBadge, t } from "@arena/ui";
  import { onMount, tick, untrack, type Snippet } from "svelte";
  import { goto, invalidate } from "$app/navigation";
  import { page } from "$app/state";
  import Clock from "../lib/Clock.svelte";
  import DeviceGate from "../lib/DeviceGate.svelte";
  import Icon from "../lib/Icon.svelte";
  import MoreSheet from "../lib/MoreSheet.svelte";
  import LockOverlay from "../lib/LockOverlay.svelte";
  import { createIdleTracker } from "../lib/idle.js";
  import { setUiSettings, uiSettings } from "../lib/ui-settings.svelte";
  import {
    BOTTOM_BAR,
    MORE,
    SCREENS,
    isSearchKey,
    screenOf,
    shortcutTarget,
    type Screen,
  } from "../lib/nav";
  import { SNAPSHOT_DEPENDENCY } from "../lib/snapshot";
  import { PAIR_PATH } from "../lib/pair";
  import {
    SESSION_DEPENDENCY,
    getLocalLocked,
    getToken,
    lockSession,
    onHubNotPaired,
    onSessionLocked,
    safeRedirect,
    setLocalLocked,
    signOut,
    unlockSession,
    type LockResult,
  } from "../lib/auth";
  import { catalog } from "../lib/catalog.svelte";
  import { alertCount, refreshAccess, service } from "../lib/service.svelte";
  import { connectEvents, uiOf } from "../lib/events";
  import { noteAck } from "../lib/admin-unlock";
  import { effectRecorded } from "../lib/undo.svelte";
  import { applyBillReverted, applyWalletChanged } from "../lib/wallet.svelte";
  import { notePairingCode } from "../lib/pairing";
  import type { Connectivity, ConnectivityState, UnlockOk } from "@arena/ui/generated";
  import { applySettingsChanged, setLocale } from "../lib/locale.svelte";
  import type { LayoutData } from "./$types";

  let { data, children }: { data: LayoutData; children: Snippet } = $props();

  /** Auto-retry delays while the Hub is unreachable, in seconds (then 30 s each). */
  const RETRY_DELAYS = [2, 4, 8, 16, 30];
  /** The width at which the rail replaces the bottom bar. */
  const WIDE = "(min-width: 840px)";
  /** Sent by the Tauri shell once it has started (or found) the Hub service. */
  const HUB_READY = "arena:hub-ready";

  /** The sign-in screen: outside the gate, with no rail. */
  const SIGN_IN = "/sign-in";

  const current = $derived(screenOf(page.url.pathname));
  const onSignIn = $derived(page.url.pathname === SIGN_IN);
  const onPair = $derived(page.url.pathname === PAIR_PATH);
  const unpaired = $derived(data.hub.ok && data.hub.snapshot.paired === false);
  const staff = $derived(data.session.state === "signed_in" ? data.session.staff : null);
  const signedOut = $derived(data.session.state === "signed_out");
  /** The Hub answered the snapshot and said who is signed in (an unknown session retries). */
  const reachable = $derived(data.hub.ok && data.session.state !== "unknown");
  /** Not known to be signed in: only the sign-in screen (or the retry state) is shown. */
  const gated = $derived(onSignIn || !staff);
  const inMore = $derived(MORE.some((s: Screen) => s.id === current?.id));
  const branchName = $derived(
    data.hub.ok ? (data.hub.snapshot.branch?.name ?? t("hub.header.no_branch")) : t("app.name"),
  );

  let moreOpen = $state(false);
  /** The idle lock is up: the shell stays mounted but inert behind the overlay. */
  let locked = $state(false);
  /** What had focus when the screen locked (it gets focus back on unlock). */
  let lockedFrom: HTMLElement | null = null;
  /** Who the lock screen names (kept while the Hub is unreachable). */
  let lockedName = $state("");
  /** Whether that member has a PIN (without one, the lock screen says to sign out). */
  let lockedHasPin = $state(true);
  /** Announced after an unlock that switched the member. */
  let announcement = $state("");
  let toast = $state("");
  let toastTimer: ReturnType<typeof setTimeout> | undefined;
  /** Which WAN link is down (`online`: no chip), from the snapshot and `connectivity.changed`. */
  let link: ConnectivityState = $state("online");
  /** When the Hub's probe found `link` (UTC ms; `null`, before any probe, is the oldest). */
  let linkCheckedAt: number | null = null;
  /** The link state last announced (so each change is announced once). */
  let announcedLink: ConnectivityState = "online";
  const LINK_LABELS = {
    internet_offline: "hub.connectivity.internet_offline",
    cloud_unreachable: "hub.connectivity.cloud_unreachable",
  } as const;
  /** The `/ui/lock` call in flight, which an unlock waits for. */
  let lockAttempt: Promise<LockResult> | null = null;
  /** Bumped on every unlock, so a stale lock retry loop stops. */
  let lockGeneration = 0;
  /** This window is sending a PIN (its own `session.unlocked` event is then not news). */
  let unlocking = false;
  /** Waits before retrying `/ui/lock`, in seconds (then 30 s each). */
  const LOCK_RETRY_DELAYS = [1, 2, 4, 8, 15, 30];
  let moreButton: HTMLButtonElement | undefined = $state();
  let mainEl: HTMLElement | undefined = $state();
  let refreshing = $state(false);
  /** Seconds until the next automatic retry, while one is scheduled. */
  let nextRetry: number | null = $state(null);
  let failures = 0;
  let retryTimer: ReturnType<typeof setTimeout> | undefined;
  let wasOk = untrack(() => reachable);

  /** Focuses `el`, making it programmatically focusable when it is not. */
  function focusEl(el: HTMLElement | null | undefined) {
    if (!el) return;
    if (el.tabIndex < 0 && !el.hasAttribute("tabindex")) el.setAttribute("tabindex", "-1");
    el.focus();
  }

  function closeMore(reason: "dismiss" | "navigate") {
    moreOpen = false;
    // Dismissing keeps the current screen and puts focus back on More.
    if (reason === "dismiss") queueMicrotask(() => moreButton?.focus());
  }

  function onkeydown(event: KeyboardEvent) {
    if (gated || locked) return;
    if (isSearchKey(event, document.activeElement)) {
      event.preventDefault();
      moreOpen = false;
      const field = document.querySelector<HTMLInputElement>("[data-player-lookup]");
      if (field) field.focus();
      else void goto("/players?lookup");
      return;
    }
    const target = shortcutTarget(event, document.activeElement);
    if (!target) return;
    event.preventDefault();
    moreOpen = false;
    if (current?.id !== target.id) goto(target.href);
  }

  /** Re-reads the snapshot (Retry, the auto-retry, focus, and the shell's ready event). */
  async function refresh() {
    if (refreshing) return;
    clearTimeout(retryTimer);
    nextRetry = null;
    refreshing = true;
    try {
      await invalidate(SNAPSHOT_DEPENDENCY);
    } finally {
      refreshing = false;
    }
  }

  function scheduleRetry() {
    clearTimeout(retryTimer);
    const seconds = RETRY_DELAYS[Math.min(failures, RETRY_DELAYS.length - 1)];
    failures++;
    nextRetry = seconds;
    retryTimer = setTimeout(() => void refresh(), seconds * 1000);
  }

  // The gate: nobody signed in goes to sign-in; a signed-in member never stays on it.
  $effect(() => {
    if (!data.hub.ok) return;
    if (unpaired) {
      if (!onPair) void goto(PAIR_PATH, { replaceState: true });
      return;
    }
    if (onPair) {
      void goto("/floor", { replaceState: true });
      return;
    }
    if (signedOut && !onSignIn) {
      // Remember where we were going, so sign-in returns there.
      const back = page.url.pathname + page.url.search;
      void goto(SIGN_IN + "?" + new URLSearchParams({ redirectTo: back }), { replaceState: true });
    } else if (staff && onSignIn) {
      void goto(safeRedirect(page.url.searchParams.get("redirectTo")), { replaceState: true });
    }
  });

  // The branch locale comes from each (re)loaded snapshot, then live from `/ui/events`.
  $effect(() => {
    const locale = data.hub.ok ? (data.hub.snapshot.locale ?? null) : null;
    const ui = data.hub.ok && data.hub.snapshot.paired !== false ? data.hub.snapshot.ui : null;
    const connectivity = data.hub.ok ? data.hub.snapshot.connectivity : null;
    if (data.hub.ok)
      untrack(() => {
        setLocale(locale);
        setUiSettings(ui);
        if (connectivity) takeLink(connectivity);
      });
  });

  /**
   * Takes a probe answer unless an older one: a snapshot that was in flight while a newer
   * `connectivity.changed` arrived must not put the old state back.
   */
  function takeLink(c: Connectivity) {
    const at = c.checked_at ?? -Infinity;
    if (at < (linkCheckedAt ?? -Infinity)) return;
    linkCheckedAt = c.checked_at;
    link = c.state;
  }

  // While someone is signed in, this view listens to the Hub's push stream, keyed on the token
  // (signing in again, even as the same member, reopens it with the new one).
  const streamToken = $derived(staff ? getToken() : null);

  // The chip and its announcements need the live push stream (someone signed in); without it
  // the state could be stale indefinitely.
  const shownLink: ConnectivityState = $derived(streamToken ? link : "online");

  // Each change of the WAN link is announced once through the polite region.
  $effect(() => {
    const now = shownLink;
    const live = Boolean(streamToken);
    untrack(() => {
      if (!live) {
        announcedLink = "online";
        return;
      }
      if (now === announcedLink) return;
      announcedLink = now;
      announcement = now === "online" ? t("hub.connectivity.restored") : t(LINK_LABELS[now]);
    });
  });

  $effect(() => {
    const token = streamToken;
    if (!token) return;
    return untrack(() => {
      return connectEvents(token, {
        onSettingsChanged: (e) => {
          applySettingsChanged(e.locale, e.keys);
          setUiSettings(uiOf(e));
        },
        // Another window sharing this session locked or unlocked it.
        onSessionLocked: () => lockScreen(false),
        onSessionUnlocked: (e) => {
          if (locked && !unlocking) void finishUnlock(e);
        },
        onDevicesChanged: () => void refresh(),
        onSessionStarted: () => void refresh(),
        onBillSettled: () => void refresh(),
        onBillReverted: () => {
          applyBillReverted();
          void refresh();
        },
        onWalletChanged: (playerId) => {
          applyWalletChanged(playerId);
          void refresh();
        },
        onEffectRecorded: (orderId) => effectRecorded(orderId),
        onGroupsChanged: (byOwner) => {
          void refresh();
          if (!byOwner) return;
          toast = t("hub.groups.owner_toast");
          clearTimeout(toastTimer);
          toastTimer = setTimeout(() => (toast = ""), 5000);
        },
        onCatalogChanged: () => {
          catalog.version++;
          void refresh();
        },
        onConnectivityChanged: (e) => takeLink(e),
        onAdminUnlockAck: (e) => noteAck(e.exit_id),
        onPcAck: (e) => noteAck(e.cmd_id),
        onPairingCodeChanged: (c) => notePairingCode(c),
        onResync: () => void refresh(),
        // Refused: find out whether the session is still there (sign-in otherwise).
        onClosed: () => void invalidate(SESSION_DEPENDENCY),
      });
    });
  });

  // The More sheet belongs to the signed-in shell.
  $effect(() => {
    if (gated) untrack(() => (moreOpen = false));
  });

  async function onSignOut() {
    await signOut();
    lockGeneration++;
    locked = false;
    await invalidate(SESSION_DEPENDENCY);
  }

  const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

  /** Tells the Hub, retrying with a back-off while it does not answer and the lock stands. */
  async function tellHubLocked() {
    const generation = lockGeneration;
    for (let i = 0; ; i++) {
      lockAttempt = lockSession();
      const result = await lockAttempt;
      lockAttempt = null;
      if (result === "signed_out") return void invalidate(SESSION_DEPENDENCY);
      if (result === "locked") return;
      await sleep(LOCK_RETRY_DELAYS[Math.min(i, LOCK_RETRY_DELAYS.length - 1)] * 1000);
      if (!locked || generation !== lockGeneration) return;
    }
  }

  /**
   * Covers the screen at once; `tellHub` is off when the Hub already holds the session locked.
   */
  function lockScreen(tellHub: boolean) {
    if (staff) {
      lockedName = staff.display_name;
      lockedHasPin = data.session.state !== "signed_in" || data.session.hasPin;
    }
    setLocalLocked(true);
    if (locked) return;
    const active = document.activeElement;
    lockedFrom = active instanceof HTMLElement && active !== document.body ? active : null;
    moreOpen = false;
    announcement = "";
    locked = true;
    if (tellHub) void tellHubLocked();
  }

  /** Uncovers the screen once the Hub has unlocked the session (here or in another window). */
  async function finishUnlock(result: UnlockOk) {
    setLocalLocked(false);
    lockGeneration++;
    // Catch up on what changed while locked (and on who is signed in now) before uncovering.
    await Promise.all([invalidate(SNAPSHOT_DEPENDENCY), invalidate(SESSION_DEPENDENCY)]);
    locked = false;
    announcement = result.switched
      ? t("hub.lock.switched", { name: result.staff.display_name })
      : "";
    await tick();
    const back = lockedFrom;
    lockedFrom = null;
    if (back?.isConnected) back.focus({ preventScroll: true });
    else focusEl(mainEl?.querySelector("h1"));
  }

  /** The PIN typed on the lock screen. */
  async function unlock(pin: string) {
    unlocking = true;
    try {
      // The Hub must hold the session locked before it checks a PIN.
      if (lockAttempt) await lockAttempt;
      let outcome = await unlockSession(pin);
      if (outcome.kind === "not_locked" && (await lockSession()) === "locked") {
        outcome = await unlockSession(pin);
      }
      if (outcome.kind === "signed_out") {
        lockGeneration++;
        locked = false;
        await invalidate(SESSION_DEPENDENCY);
        return { ok: true } as const;
      }
      if (outcome.kind === "rejected") return { ok: false, message: "hub.lock.rejected" } as const;
      if (outcome.kind === "locked") {
        return { ok: false, message: "ui.pin.locked", retryAfterS: outcome.retryAfterS } as const;
      }
      if (outcome.kind === "unreachable" || outcome.kind === "not_locked") {
        return { ok: false, message: "ui.pin.unreachable" } as const;
      }
      if (outcome.kind === "error") return { ok: false, message: "ui.pin.failed" } as const;
      await finishUnlock(outcome);
      return { ok: true } as const;
    } finally {
      unlocking = false;
    }
  }

  // One idle countdown per signed-in view. Its length is read when each period starts, so a
  // new setting never shortens the countdown in progress. It stops while gated.
  const idleTracker = createIdleTracker({
    minutes: () => uiSettings.idleLockMinutes,
    onIdle: () => lockScreen(true),
    target: typeof window === "undefined" ? null : window,
  });
  const tracking = $derived(!gated && !locked);
  $effect(() => {
    if (!tracking) return;
    untrack(() => idleTracker.start());
    return () => idleTracker.stop();
  });

  // The Hub holds this session locked (a reload, or another window locked it), or this window
  // was locked before a reload: lock here too.
  $effect(() => {
    const signedIn = data.session.state === "signed_in";
    const hubLocked = signedIn && data.session.state === "signed_in" && data.session.locked;
    if (hubLocked) untrack(() => lockScreen(false));
    else if (signedIn) untrack(() => getLocalLocked() && !locked && lockScreen(true));
  });

  // Signed out: nothing to unlock.
  $effect(() => {
    if (signedOut) untrack(() => (locked = false));
  });

  onMount(() => {
    onSessionLocked(() => lockScreen(false));
    onHubNotPaired(() => void refresh());
    return () => {
      onSessionLocked(null);
      onHubNotPaired(null);
    };
  });

  // Every load result: back off while unreachable; on recovery stop retrying and move focus
  // to the screen's heading.
  $effect(() => {
    if (data.device.kind !== "ok") return;
    // Read `data` itself (not the `reachable` derived): every load result must re-run this,
    // even when it is unreachable again.
    const ok =
      data.hub.ok && (data.hub.snapshot.paired === false || data.session.state !== "unknown");
    untrack(() => {
      if (ok) {
        clearTimeout(retryTimer);
        nextRetry = null;
        failures = 0;
        if (!wasOk) tick().then(() => focusEl(mainEl?.querySelector("h1")));
      } else {
        scheduleRetry();
      }
      wasOk = ok;
    });
  });

  onMount(refreshAccess);

  onMount(() => {
    const onVisible = () => {
      if (document.visibilityState === "visible") void refresh();
    };
    const onFocus = () => {
      refreshAccess();
      void refresh();
    };
    const onReady = () => void refresh();
    // Back above 840px the bottom bar (and its More button) is hidden: close the sheet.
    const wide = window.matchMedia(WIDE);
    const onWide = (e: MediaQueryListEvent) => {
      if (!e.matches || !moreOpen) return;
      moreOpen = false;
      tick().then(() => focusEl(mainEl));
    };
    window.addEventListener("focus", onFocus);
    window.addEventListener(HUB_READY, onReady);
    document.addEventListener("visibilitychange", onVisible);
    wide.addEventListener("change", onWide);
    return () => {
      clearTimeout(retryTimer);
      window.removeEventListener("focus", onFocus);
      window.removeEventListener(HUB_READY, onReady);
      document.removeEventListener("visibilitychange", onVisible);
      wide.removeEventListener("change", onWide);
    };
  });
</script>

<svelte:window {onkeydown} />

<svelte:head>
  {#if data.device.kind === "ok" && !reachable && !unpaired}
    <title>{t("hub.page_title", { screen: t("hub.error.unreachable.title") })}</title>
  {/if}
</svelte:head>

{#snippet unreachable()}
  <section class="unreachable" data-testid="hub-unreachable">
    <h1 class="type-hub-numeric">{t("hub.error.unreachable.title")}</h1>
    <p class="type-hub-body">{t("hub.error.unreachable.body")}</p>
    <p class="retry-status type-hub-label" role="status" data-testid="hub-retry-status">
      {#if refreshing}
        {t("hub.error.retrying_status")}
      {:else if nextRetry !== null}
        {t("hub.error.next_retry", { seconds: nextRetry })}
      {/if}
    </p>
    <Button
      label="common.retry"
      disabled={refreshing}
      disabledReason="hub.error.retrying"
      onclick={refresh}
    />
  </section>
{/snippet}

{#if data.device.kind !== "ok"}
  <div class="pair-screen" data-theme="dark">
    <main class="hub-main" bind:this={mainEl}>
      <DeviceGate device={data.device} checking={refreshing} oncheck={refresh} />
    </main>
  </div>
{:else if unpaired}
  <div class="pair-screen" data-theme="dark">
    <main class="hub-main" bind:this={mainEl}>
      {#if onPair}
        {@render children()}
      {/if}
    </main>
  </div>
{:else if gated}
  <div class="gate" inert={locked} aria-hidden={locked ? "true" : undefined}>
    <header class="hub-header">
      <p class="branch type-hub-title" data-testid="hub-branch">{branchName}</p>
      <Clock />
    </header>
    <main class="hub-main" bind:this={mainEl}>
      {#if !reachable}
        {@render unreachable()}
      {:else if onSignIn}
        {@render children()}
      {/if}
    </main>
  </div>
{:else}
  <div class="shell" inert={moreOpen || locked} aria-hidden={locked ? "true" : undefined}>
    <header class="hub-header">
      <p class="branch type-hub-title" data-testid="hub-branch">{branchName}</p>
      <div class="header-end">
        {#if staff}
          <p class="staff type-hub-label" data-testid="hub-staff">
            {t("hub.header.signed_in", { name: staff.display_name, role: staff.role_name })}
          </p>
          <Button label="hub.header.sign_out" variant="secondary" onclick={onSignOut} />
        {/if}
        {#if shownLink !== "online"}
          <p
            class="chip-offline type-hub-label"
            data-testid="hub-chip-offline"
            data-link={shownLink}
          >
            <span class="chip-glyph" aria-hidden="true">⚠</span>
            {t(LINK_LABELS[shownLink])}
          </p>
        {/if}
        {#if data.hub.ok && data.hub.snapshot.pairing_revoked}
          <p class="chip-offline type-hub-label" role="status" data-testid="hub-revoked">
            <span class="chip-glyph" aria-hidden="true">⚠</span>
            {t("hub.pairing.revoked")}
          </p>
        {/if}
        {#if data.hub.ok && (data.hub.snapshot.licence_unverified || data.hub.snapshot.clock_wrong)}
          <div class="status-slot" role="status" data-testid="hub-status-slot">
            {#if data.hub.snapshot.licence_unverified}
              <StatusBadge state="warning" label="hub.licence.unverified" />
            {/if}
            {#if data.hub.snapshot.clock_wrong}
              <StatusBadge state="warning" label="hub.clock.wrong" />
            {/if}
          </div>
        {/if}
        {#if alertCount(service.access) > 0}
          <a
            class="service-alert ui-interactive"
            href="/system/service"
            data-testid="hub-service-alert"
          >
            <AlertBadge count={alertCount(service.access)} />
          </a>
        {/if}
        <Clock />
      </div>
    </header>

    <nav class="hub-rail" aria-label={t("hub.nav.label")}>
      <ul>
        {#each SCREENS as screen (screen.id)}
          <li>
            <a
              class="rail-item ui-interactive"
              href={screen.href}
              aria-current={current?.id === screen.id ? "page" : undefined}
            >
              <Icon name={screen.id} />
              <span class="type-hub-micro">{t(screen.label)}</span>
            </a>
          </li>
        {/each}
      </ul>
    </nav>

    <main class="hub-main" bind:this={mainEl}>
      {#if data.hub.ok && !onPair}
        {@render children()}
      {:else}
        {@render unreachable()}
      {/if}
    </main>

    <nav class="hub-bottom-bar" aria-label={t("hub.nav.label")}>
      <ul>
        {#each BOTTOM_BAR as screen (screen.id)}
          <li>
            <a
              class="bar-item ui-interactive"
              href={screen.href}
              aria-current={current?.id === screen.id ? "page" : undefined}
            >
              <Icon name={screen.id} />
              <span class="type-hub-micro">{t(screen.short ?? screen.label)}</span>
            </a>
          </li>
        {/each}
        <li>
          <button
            class="bar-item ui-interactive"
            type="button"
            bind:this={moreButton}
            aria-haspopup="dialog"
            aria-expanded={moreOpen}
            aria-controls={moreOpen ? "hub-more-sheet" : undefined}
            data-active={inMore ? "true" : undefined}
            onclick={() => (moreOpen = true)}
          >
            <Icon name="more" />
            <span class="type-hub-micro">{t("hub.nav.more")}</span>
          </button>
        </li>
      </ul>
    </nav>
  </div>
{/if}

{#if moreOpen}
  <MoreSheet id="hub-more-sheet" current={current?.id} onclose={closeMore} />
{/if}

{#if locked}
  <LockOverlay
    name={staff?.display_name ?? lockedName}
    hasPin={lockedHasPin}
    onunlock={unlock}
    onsignout={onSignOut}
  />
{/if}

<p class="visually-hidden" aria-live="polite" data-testid="hub-announce">{announcement}</p>
{#if toast}
  <p class="toast type-hub-label" role="status" data-testid="hub-toast">{toast}</p>
{/if}

<style>
  :global(body) {
    margin: 0;
    background-color: var(--hub-bg);
  }

  .toast {
    position: fixed;
    right: var(--space-4);
    bottom: var(--space-4);
    z-index: 10;
    margin: 0;
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--hud-toast-border);
    border-radius: var(--radius-md);
    background-color: var(--hud-toast-bg);
    color: var(--hud-toast-text);
  }

  .shell {
    display: grid;
    grid-template-columns: var(--space-hub-rail-width) minmax(0, 1fr);
    grid-template-rows: var(--space-hub-header-height) minmax(0, 1fr);
    grid-template-areas:
      "header header"
      "rail main";
    height: 100dvh;
    background-color: var(--hub-bg);
    color: var(--text);
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  /* ── Header ─────────────────────────────────────────────────────────────────── */
  .hub-header {
    grid-area: header;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-5);
    min-width: 0;
    padding: 0 var(--space-hub-gutter);
    border-bottom: 1px solid var(--hub-line);
    background-color: var(--hub-header-background);
  }

  .header-end {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    min-width: 0;
  }

  /* The WAN link that is down: tinted, with a glyph and words (never colour alone). */
  .chip-offline {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    margin: 0;
    padding: var(--space-1) var(--space-3);
    border-radius: var(--radius-full);
    background-color: var(--chip-offline-background);
    color: var(--chip-offline-foreground);
    white-space: nowrap;
  }

  .service-alert {
    display: inline-flex;
    align-items: center;
    min-height: var(--space-touch-min);
    min-width: var(--space-touch-min);
    justify-content: center;
    text-decoration: none;
  }

  .status-slot {
    display: inline-flex;
    gap: var(--space-2);
  }

  .staff {
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Signed out: the header and the sign-in screen, no rail. */
  .gate {
    display: grid;
    grid-template-rows: var(--space-hub-header-height) minmax(0, 1fr);
    height: 100dvh;
    background-color: var(--hub-bg);
    color: var(--text);
  }

  .pair-screen {
    display: grid;
    align-content: center;
    justify-items: center;
    min-height: 100dvh;
    background-color: var(--hub-bg);
    color: var(--text);
  }

  .pair-screen .hub-main {
    width: 100%;
    max-width: 32rem;
    box-sizing: border-box;
  }

  .gate .hub-header {
    grid-area: auto;
  }

  .gate .hub-main {
    grid-area: auto;
  }

  .branch {
    margin: 0;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* ── Rail ───────────────────────────────────────────────────────────────────── */
  .hub-rail {
    --ring-gap: var(--hub-rail-background);
    grid-area: rail;
    overflow-y: auto;
    border-right: 1px solid var(--hub-line);
    background-color: var(--hub-rail-background);
  }

  .hub-rail ul {
    display: grid;
    gap: var(--space-2);
    padding: var(--space-2);
  }

  .rail-item,
  .bar-item {
    --ui-bg: transparent;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-1);
    min-width: var(--space-touch-min);
    min-height: var(--space-9);
    padding: var(--space-2) var(--space-1);
    border: 0;
    border-radius: var(--radius-md);
    color: var(--text-muted);
    font: inherit;
    text-align: center;
    text-decoration: none;
    cursor: pointer;
  }

  /* hub-micro labels in sentence case, so whole words fit the 88px rail. */
  .rail-item span,
  .bar-item span {
    max-width: 100%;
    text-transform: none;
    letter-spacing: normal;
    overflow-wrap: break-word;
  }

  /* The current screen: filled, in full ink and a heavier weight (never colour alone). */
  .rail-item[aria-current="page"] {
    --ui-bg: var(--hub-rail-item-active);
    color: var(--text);
  }

  .bar-item:is([aria-current="page"], [data-active="true"]) {
    --ui-bg: var(--hub-bottom-bar-item-active);
    color: var(--text);
  }

  .rail-item[aria-current="page"] span,
  .bar-item:is([aria-current="page"], [data-active="true"]) span {
    font-weight: var(--type-hub-numeric-weight);
  }

  /* ── Main ───────────────────────────────────────────────────────────────────── */
  .hub-main {
    grid-area: main;
    min-width: 0;
    overflow-y: auto;
    padding: var(--space-hub-gutter);
  }

  .unreachable {
    display: grid;
    justify-items: start;
    gap: var(--space-4);
    max-width: 60ch;
  }

  .unreachable h1,
  .unreachable p {
    margin: 0;
  }

  .unreachable p {
    color: var(--text-muted);
  }

  .retry-status {
    min-height: 1lh;
  }

  /* ── Bottom bar (below 840px it replaces the rail) ──────────────────────────── */
  .hub-bottom-bar {
    display: none;
  }

  @media (max-width: 839.98px) {
    .shell {
      grid-template-columns: minmax(0, 1fr);
      grid-template-rows: var(--space-hub-header-height) minmax(0, 1fr) var(--space-9);
      grid-template-areas:
        "header"
        "main"
        "bar";
    }

    .hub-header {
      padding: 0 var(--space-hub-gutter-phone);
    }

    .hub-main {
      padding: var(--space-hub-gutter-phone);
    }

    .hub-rail {
      display: none;
    }

    .hub-bottom-bar {
      --ring-gap: var(--hub-bottom-bar-background);
      grid-area: bar;
      display: block;
      border-top: 1px solid var(--border-strong);
      background-color: var(--hub-bottom-bar-background);
    }

    .hub-bottom-bar ul {
      display: grid;
      grid-template-columns: repeat(5, minmax(0, 1fr));
      height: 100%;
    }

    .bar-item {
      width: 100%;
      height: 100%;
      min-height: var(--space-9);
      border-radius: 0;
    }
  }
</style>
