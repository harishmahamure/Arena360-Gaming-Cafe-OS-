<script lang="ts">
  import "@arena/ui/base.css";
  import { AlertBadge, Button, openAdExclusions, registerAdExclusion, t } from "@arena/ui";
  import { onMount, tick, untrack, type Snippet } from "svelte";
  import { goto, invalidate } from "$app/navigation";
  import { page } from "$app/state";
  import AdBanner from "../lib/AdSlot.svelte";
  import AlertList from "../lib/AlertList.svelte";
  import BackupChip from "../lib/BackupChip.svelte";
  import { agoOf } from "../lib/backup-status";
  import { backup, backupBehind, hubNow, watchBackup } from "../lib/backup-status.svelte";
  import Clock from "../lib/Clock.svelte";
  import RuleChips from "../lib/RuleChips.svelte";
  import AwaitAndroid from "../lib/AwaitAndroid.svelte";
  import HandoverStopped from "../lib/HandoverStopped.svelte";
  import MoveBranch from "../lib/MoveBranch.svelte";
  import RestoreBranch from "../lib/RestoreBranch.svelte";
  import { nextRuleChange, ruleChips } from "../lib/rules";
  import DeviceGate from "../lib/DeviceGate.svelte";
  import Icon from "../lib/Icon.svelte";
  import MoreSheet from "../lib/MoreSheet.svelte";
  import LockOverlay from "../lib/LockOverlay.svelte";
  import { createIdleTracker } from "../lib/idle.js";
  import { setUiSettings, uiSettings } from "../lib/ui-settings.svelte";
  import { viewport } from "../lib/viewport.svelte";
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
  import {
    ackAlerts,
    staysOpen,
    announcements,
    listRows,
    onAlertsChanged,
    requestSheet,
    stopAlerts,
    syncAlerts,
    type AlertRow,
  } from "../lib/alerts.svelte";
  import { catalog, library } from "../lib/catalog.svelte";
  import { alertCount, refreshAccess, service } from "../lib/service.svelte";
  import { adRatingOf, ads } from "../lib/ads";
  import { AdMetrics } from "../lib/ad-metrics";
  import { InterstitialPolicy } from "../lib/interstitial";
  import { KEY_ADS_CONSENT, KEY_ADS_RATING } from "../lib/operating";
  import { connectEvents, uiOf } from "../lib/events";
  import { noteAck } from "../lib/admin-unlock";
  import { effectRecorded } from "../lib/undo.svelte";
  import { applyBillReverted, applyWalletChanged } from "../lib/wallet.svelte";
  import { notePairingCode } from "../lib/pairing";
  import { ownerUpdateKey } from "../lib/owner-update";
  import type {
    Connectivity,
    ConnectivityState,
    OverwrittenChange,
    UnlockOk,
  } from "@arena/ui/generated";
  import { applySettingsChanged, setLocale } from "../lib/locale.svelte";
  import { setShift } from "../lib/shift.svelte";
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
  /** A replacement Hub waiting for its branch to be restored: nothing else is shown (Story 9.9). */
  const restoring = $derived(data.hub.ok ? (data.hub.snapshot.restore_pending ?? null) : null);
  /** A Windows Hub paired for a Hub move, waiting for the Android Hub's handover (Story 10.16). */
  const awaitingAndroid = $derived(data.hub.ok ? (data.hub.snapshot.migration_wait ?? null) : null);
  /** The handover of the branch between an Android and a Windows Hub, as this Hub sees it (Story 10.17). */
  const handover = $derived(data.hub.ok ? (data.hub.snapshot.handover ?? null) : null);
  /** An Android Hub that has stopped to hand the branch over: nothing else is shown. */
  const handingOver = $derived(
    handover !== null && ["transferring", "file", "moved"].includes(handover.phase)
      ? handover
      : null,
  );
  let movePanel = $state(false);
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

  /** The ads gate is on: the layout reserves the permanent `ad-banner` slot (Story 4.11). */
  let adsOn = $state(ads.state.enabled);
  // Declared before the effect that evaluates the gate, so its first change is never missed.
  $effect(() => {
    adsOn = ads.state.enabled;
    return ads.onChange((s) => (adsOn = s.enabled));
  });
  // Overlays outside the shell (the lock screen, the More sheet, the phone tile sheet) stop above
  // the slot through these two properties; both are 0 while the gate is off.
  $effect(() => {
    const root = document.documentElement;
    if (adsOn) root.dataset.ads = "on";
    else delete root.dataset.ads;
    return () => delete root.dataset.ads;
  });

  let moreOpen = $state(false);
  /** The header's alert list is open. */
  let alertsOpen = $state(false);
  let alertButton: HTMLButtonElement | undefined = $state();
  const alertRows = $derived(listRows());
  /** The daily export could not be saved: the backup folder is not available (Story 9.5). */
  const backupFailed = $derived(
    data.hub.ok && data.hub.snapshot.setup?.backup_export_failed === true,
  );
  const deviceNames = $derived(
    Object.fromEntries(
      (data.hub.ok ? data.hub.snapshot.devices : []).map((d) => [d.device_id, d.label]),
    ),
  );
  /** The idle lock is up: the shell stays mounted but inert behind the overlay. */
  let locked = $state(false);
  // Every counted plugin event goes to the Hub once (Story 4.14); the gate keeps a paid or
  // Windows Hub from ever receiving one.
  $effect(() => {
    const metrics = new AdMetrics(ads);
    return () => metrics.dispose();
  });
  /**
   * When a full-screen ad may follow a PIN unlock (Story 4.13). `adShowing` holds the idle
   * countdown while Google's interstitial is on screen; it restarts from zero when it ends.
   */
  const interstitial = new InterstitialPolicy(ads);
  let adShowing = $state(false);
  /** Where focus goes back to once the interstitial has closed. */
  let adReturnFocus: HTMLElement | null = null;
  $effect(() => {
    const stop = interstitial.onShowing((showing) => {
      adShowing = showing;
      if (showing) return;
      const back = adReturnFocus;
      adReturnFocus = null;
      void tick().then(() => {
        if (back?.isConnected) back.focus({ preventScroll: true });
      });
    });
    return () => {
      stop();
      interstitial.dispose();
    };
  });
  // Leaving through the ad and coming back also ends the wait.
  $effect(() => {
    if (!adShowing) return;
    let hidden = false;
    const onVisibility = () => {
      if (document.visibilityState === "hidden") hidden = true;
      else if (hidden) interstitial.ended();
    };
    document.addEventListener("visibilitychange", onVisibility);
    return () => document.removeEventListener("visibilitychange", onVisibility);
  });
  // The header's alert list is open: no full-screen ad may follow a lock that starts now.
  $effect(() => (alertsOpen ? registerAdExclusion("alert-list") : undefined));

  /** What had focus when the screen locked (it gets focus back on unlock). */
  let lockedFrom: HTMLElement | null = null;
  /** Who the lock screen names (kept while the Hub is unreachable). */
  let lockedName = $state("");
  /** Whether that member has a PIN (without one, the lock screen says to sign out). */
  let lockedHasPin = $state(true);
  /** Announced after an unlock that switched the member. */
  let announcement = $state("");
  let toast = $state("");
  /** The change an owner's update replaced, listed where the toast opens its detail. */
  let toastDetail: OverwrittenChange | null = $state(null);
  let toastTimer: ReturnType<typeof setTimeout> | undefined;
  function showToast(text: string, detail: OverwrittenChange | null = null) {
    toast = text;
    toastDetail = detail;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(
      () => {
        toast = "";
        toastDetail = null;
      },
      detail ? 15000 : 5000,
    );
  }
  const actorOf = (c: OverwrittenChange) => [c.actor_kind, c.actor_id].join(" ");
  const clockOf = (ms: number) =>
    new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
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
      // On the POS the shortcut finds a product; elsewhere it finds a player.
      const field = document.querySelector<HTMLInputElement>(
        "[data-pos-search], [data-player-lookup]",
      );
      // On the POS with the phone cart sheet open there is no field; leaving would drop the cart.
      if (field) field.focus();
      else if (current?.id !== "pos") void goto("/players?lookup");
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
      // An ads setting changed while this read was in flight: it may have missed the new value.
      if (adsChangedDuringRefresh) {
        adsChangedDuringRefresh = false;
        void refresh();
      }
    }
  }

  let adsChangedDuringRefresh = false;
  function refreshForAds() {
    if (refreshing) adsChangedDuringRefresh = true;
    void refresh();
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
    if (restoring !== null || awaitingAndroid !== null) return;
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
    const shift = data.hub.ok ? data.hub.snapshot.shift : null;
    if (data.hub.ok)
      untrack(() => {
        setLocale(locale);
        setShift(shift);
        setUiSettings(ui);
        if (connectivity) takeLink(connectivity);
      });
  });

  /** Controls whose focus the on-screen keyboard follows (not checkboxes, radios or buttons). */
  const TEXT_FIELD =
    'input:not([type="checkbox"],[type="radio"],[type="button"],[type="submit"],[type="range"]),textarea,select,[contenteditable="true"]';

  const ruleChipList = $derived(
    data.hub.ok
      ? ruleChips(
          data.hub.snapshot.groups,
          data.hub.snapshot.locale ?? null,
          data.hub.snapshot.locale?.tz,
        )
      : [],
  );

  // A pricing rule begins or ends on the Hub's own clock: re-read the snapshot at the next
  // boundary, so the chip and every rate change on time with the WAN down.
  $effect(() => {
    const at = data.hub.ok && staff ? nextRuleChange(data.hub.snapshot.groups) : null;
    if (at === null) return;
    const wait = Math.min(Math.max(at - Date.now() + 500, 1000), 2_000_000_000);
    const timer = setTimeout(() => void refresh(), wait);
    return () => clearTimeout(timer);
  });

  // Open alerts and the branch conditions come from each (re)loaded snapshot; the sound loop and
  // the announcements follow them even while the screen is locked. A failed load keeps the last
  // ones, so an outage neither hides an open alert nor replays its sound on recovery.
  $effect(() => {
    if (!data.hub.ok) return;
    const snapshot = data.hub.snapshot;
    untrack(() => syncAlerts(snapshot));
  });

  $effect(() => {
    if (alertRows.length === 0 || gated) alertsOpen = false;
  });

  function closeAlerts() {
    alertsOpen = false;
    queueMicrotask(() => alertButton?.focus());
  }

  /** A list row: acknowledges that alert and opens its tile's sheet. */
  async function pickAlert(row: AlertRow) {
    alertsOpen = false;
    if (row.key === "move") {
      movePanel = true;
      return;
    }
    if (row.alertId && !staysOpen(row.alertId)) void ackAlerts([row.alertId]);
    if (!row.deviceId) {
      if (row.href) await goto(row.href);
      return;
    }
    requestSheet(row.deviceId);
    if (current?.id !== "floor") {
      await goto("/floor?" + new URLSearchParams({ device: row.deviceId }));
    }
  }

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

  /** Whether any chip has something to say (the phone shows them in a row under the header). */
  const hasChips = $derived(
    shownLink !== "online" ||
      (data.hub.ok &&
        (Boolean(data.hub.snapshot.lan?.no_lan) || data.hub.snapshot.pairing_revoked === true)) ||
      ruleChipList.length > 0,
  );

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

  // While someone is signed in the header keeps the backup status fresh (Story 9.16).
  $effect(() => {
    if (!streamToken) return;
    return untrack(() => watchBackup(() => void invalidate(SESSION_DEPENDENCY)));
  });

  // "Backup behind" and its return to normal are announced once each, in the polite region.
  let announcedBehind = false;
  $effect(() => {
    const now = backup.status?.cloud === true && backupBehind();
    untrack(() => {
      if (now === announcedBehind) return;
      announcedBehind = now;
      if (now) {
        const a = agoOf(backup.status?.last_upload_at ?? null, hubNow());
        announcement = t("hub.header.backup.announce_behind", { when: t(a.key, { n: a.n }) });
      } else if (backup.status?.cloud) {
        announcement = t("hub.header.backup.announce_ok");
      }
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
          // The rating and consent reach the ads plugin through the snapshot.
          if (e.keys.some((k) => k === KEY_ADS_RATING || k === KEY_ADS_CONSENT)) refreshForAds();
        },
        // Another window sharing this session locked or unlocked it.
        onSessionLocked: () => lockScreen(false),
        onSessionUnlocked: (e) => {
          if (locked && !unlocking) void finishUnlock(e);
        },
        onDevicesChanged: () => void refresh(),
        onSessionStarted: () => void refresh(),
        onOrderPlaced: () => void refresh(),
        onOrderLineChanged: () => void refresh(),
        onOrderVoided: () => void refresh(),
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
          showToast(t("hub.groups.owner_toast"));
        },
        onUpdatedByOwner: (e) => {
          const key = ownerUpdateKey(e.kind);
          if (key) showToast(t(key), e.overwritten ?? null);
        },
        onCatalogChanged: () => {
          catalog.version++;
          void refresh();
        },
        onLibraryChanged: () => {
          library.version++;
        },
        onConnectivityChanged: (e) => takeLink(e),
        onEntitlementChanged: (e) => ads.evaluate(e.tier),
        onAdminUnlockAck: (e) => noteAck(e.exit_id),
        onPcAck: (e) => noteAck(e.cmd_id),
        onPairingCodeChanged: (c) => notePairingCode(c),
        onResync: () => void refresh(),
        // Refused: find out whether the session is still there (sign-in otherwise).
        onClosed: () => void invalidate(SESSION_DEPENDENCY),
      });
    });
  });

  // The ads gate follows the Hub's effective tier on every snapshot (and, when signed in, on
  // `entitlement.changed`). An unreachable Hub keeps the gate as it was.
  $effect(() => {
    if (!data.hub.ok) return;
    const tier = data.hub.snapshot.paired === false ? null : data.hub.snapshot.tier;
    const setup = data.hub.snapshot.setup;
    untrack(() => {
      ads.configure({
        rating: adRatingOf(setup?.ad_rating),
        personalised: setup?.ad_consent === "given",
      });
      ads.evaluate(tier);
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
    // Taken before anything closes: the surfaces open now decide whether an ad may follow.
    interstitial.lock(openAdExclusions());
    const active = document.activeElement;
    lockedFrom = active instanceof HTMLElement && active !== document.body ? active : null;
    moreOpen = false;
    announcement = "";
    locked = true;
    if (tellHub) void tellHubLocked();
  }

  /** Uncovers the screen once the Hub has unlocked the session (here or in another window). */
  async function finishUnlock(result: UnlockOk, byPin = false) {
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
    // Only a PIN typed here after an idle lock can bring an ad, and never makes the Hub wait.
    if (byPin) {
      if (interstitial.unlock()) adReturnFocus = back?.isConnected ? back : null;
    } else interstitial.cancel();
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
      await finishUnlock(outcome, true);
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
  const tracking = $derived(!gated && !locked && !adShowing);
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
    onAlertsChanged(() => void invalidate(SNAPSHOT_DEPENDENCY));
    return () => {
      onSessionLocked(null);
      onHubNotPaired(null);
      onAlertsChanged(null);
      stopAlerts();
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
    // The on-screen keyboard shrinks the window: the focused field then scrolls into view (the
    // nearest scroll container's scroll padding keeps it clear of sticky bars). Focus alone does
    // not move the scroll: the browser already brings an unseen field into view.
    const reveal = () => {
      const field = document.activeElement;
      if (field instanceof HTMLElement && field.matches(TEXT_FIELD)) {
        field.scrollIntoView({ block: "nearest", inline: "nearest" });
      }
    };
    window.addEventListener("resize", reveal);
    window.visualViewport?.addEventListener("resize", reveal);
    window.addEventListener("focus", onFocus);
    window.addEventListener(HUB_READY, onReady);
    document.addEventListener("visibilitychange", onVisible);
    wide.addEventListener("change", onWide);
    return () => {
      clearTimeout(retryTimer);
      window.removeEventListener("resize", reveal);
      window.visualViewport?.removeEventListener("resize", reveal);
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

{#snippet chips()}
  {#if shownLink !== "online"}
    <p class="chip-offline type-hub-label" data-testid="hub-chip-offline" data-link={shownLink}>
      <span class="chip-glyph" aria-hidden="true">⚠</span>
      {t(LINK_LABELS[shownLink])}
    </p>
  {/if}
  {#if data.hub.ok && data.hub.snapshot.lan?.no_lan}
    <a
      class="no-lan ui-interactive type-hub-label"
      href="/settings/devices-display"
      data-testid="hub-no-lan"
    >
      <span class="chip-glyph" aria-hidden="true">⚠</span>
      {t("hub.connectivity.no_lan")}
    </a>
  {/if}
  {#if data.hub.ok && data.hub.snapshot.pairing_revoked}
    <p class="chip-offline type-hub-label" role="status" data-testid="hub-revoked">
      <span class="chip-glyph" aria-hidden="true">⚠</span>
      {t("hub.pairing.revoked")}
    </p>
  {/if}
  <RuleChips chips={ruleChipList} scope="hub" />
{/snippet}

{#if data.device.kind !== "ok"}
  <div class="pair-screen" data-theme="dark">
    <main class="hub-main" bind:this={mainEl}>
      <DeviceGate device={data.device} checking={refreshing} oncheck={refresh} />
    </main>
  </div>
{:else if restoring !== null}
  <div class="pair-screen" data-theme="dark">
    <main class="hub-main" bind:this={mainEl}>
      <RestoreBranch
        branchName={restoring}
        released={data.hub.ok && data.hub.snapshot.restore_key_released === true}
        tier={data.hub.ok ? (data.hub.snapshot.tier ?? null) : null}
      />
    </main>
  </div>
{:else if handingOver !== null && !(onSignIn && !staff && handingOver.phase !== "moved")}
  <div class="pair-screen" data-theme="dark">
    <main class="hub-main" bind:this={mainEl}>
      <HandoverStopped view={handingOver} signedIn={staff !== null} />
    </main>
  </div>
{:else if awaitingAndroid !== null}
  <div class="pair-screen" data-theme="dark">
    <main class="hub-main" bind:this={mainEl}>
      <AwaitAndroid branchName={awaitingAndroid} received={handover} />
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
  <div class="gate" class:has-ads={adsOn} inert={locked} aria-hidden={locked ? "true" : undefined}>
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
    {#if adsOn}
      <div class="ad-area"><AdBanner /></div>
    {/if}
  </div>
{:else}
  <div
    class="shell"
    class:has-chips={viewport.phone && hasChips}
    class:has-ads={adsOn}
    inert={moreOpen || locked}
    aria-hidden={locked ? "true" : undefined}
  >
    <header class="hub-header" inert={viewport.sheetFull}>
      <p class="branch type-hub-title" data-testid="hub-branch">{branchName}</p>
      <div class="header-end">
        {#if staff}
          <p class="staff type-hub-label" data-testid="hub-staff">
            {t("hub.header.signed_in", { name: staff.display_name, role: staff.role_name })}
          </p>
          <Button label="hub.header.sign_out" variant="secondary" onclick={onSignOut} />
        {/if}
        {#if !viewport.phone}{@render chips()}{/if}
        {#if !viewport.phone}<BackupChip />{/if}
        {#if alertRows.length > 0}
          <button
            type="button"
            class="alert-button ui-interactive"
            bind:this={alertButton}
            aria-expanded={alertsOpen}
            aria-controls={alertsOpen ? "hub-alert-list" : undefined}
            data-testid="hub-alerts"
            onclick={() => (alertsOpen = !alertsOpen)}
          >
            <span class="type-hub-label" class:visually-hidden={viewport.phone}
              >{t("hub.alerts.button")}</span
            >
            <AlertBadge count={alertRows.length} live={false} />
          </button>
        {/if}
        {#if backupFailed}
          <a
            class="backup-alert ui-interactive"
            href="/settings/system"
            data-testid="hub-backup-alert"
          >
            <span class="type-hub-label" class:visually-hidden={viewport.phone}
              >{t("hub.header.backup_alert")}</span
            >
            <AlertBadge count={1} />
          </a>
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
        {#if !viewport.phone}<Clock />{/if}
      </div>
      {#if alertsOpen}
        <AlertList
          id="hub-alert-list"
          rows={alertRows}
          names={deviceNames}
          onpick={pickAlert}
          onclose={closeAlerts}
        />
      {/if}
      {#if movePanel && data.hub.ok && data.hub.snapshot.move_notice}
        <MoveBranch
          pc={data.hub.snapshot.move_notice.target_hub_name}
          view={handover}
          addresses={data.hub.snapshot.lan?.addresses ?? []}
          onclose={() => (movePanel = false)}
        />
      {/if}
    </header>

    {#if viewport.phone && hasChips}
      <div class="chip-row" data-testid="hub-chip-row" inert={viewport.sheetFull}>
        {@render chips()}
      </div>
    {/if}

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

    {#if adsOn}
      <div class="ad-area"><AdBanner /></div>
    {/if}

    <nav class="hub-bottom-bar" aria-label={t("hub.nav.label")} inert={viewport.sheetFull}>
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
<div class="visually-hidden" aria-live="polite" data-testid="alert-announce">
  {#each announcements as a (a.id)}<p>{a.text}</p>{/each}
</div>
{#if toast}
  <div class="toast type-hub-label" role="status">
    <p data-testid="hub-toast">{toast}</p>
    {#if toastDetail}
      <details data-testid="hub-toast-detail">
        <summary>{t("hub.owner_update.details")}</summary>
        <ul>
          <li>
            {t("hub.owner_update.replaced", {
              actor: actorOf(toastDetail),
              time: clockOf(toastDetail.at),
            })}
          </li>
          <li>{t("hub.owner_update.change_id", { id: toastDetail.intent_id })}</li>
        </ul>
      </details>
    {/if}
  </div>
{/if}

<style>
  :global(body) {
    margin: 0;
    background-color: var(--hub-bg);
  }

  /* The ad-banner slot (Story 4.11). `--ad-slot-height` is the strip; `--ad-reserved` is how far
     its top edge is from the bottom of the window (on a phone the bottom bar is below it), which
     every overlay leaves free; `--ad-gap` is the clear space the slot keeps from a settle bar. All
     are 0 or unset while the gate is off. */
  :global(:root) {
    --ad-slot-height: 0px;
    --ad-reserved: 0px;
  }

  :global(:root[data-ads="on"]) {
    --ad-slot-height: 60px;
    --ad-gap: 24px;
    --ad-reserved: var(--ad-slot-height);
    /* What the slot takes from the height the wide tile sheet may use (header, strip and gap). */
    --ad-shrink: calc(var(--space-hub-header-height) + var(--ad-slot-height) + var(--ad-gap));
    /* A focused element is never left behind the slot, even when the document itself scrolls. */
    scroll-padding-bottom: var(--ad-reserved);
  }

  :global(:root[data-ads="on"]) .ad-area {
    padding-top: var(--ad-gap);
  }

  .ad-area {
    grid-area: ad;
    min-width: 0;
  }

  .gate .ad-area {
    grid-area: auto;
  }

  .toast {
    position: fixed;
    right: var(--space-4);
    /* Above the ad-banner slot, which nothing draws over (--ad-reserved is 0 without ads). */
    bottom: calc(var(--space-4) + var(--ad-reserved));
    z-index: 10;
    margin: 0;
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--hud-toast-border);
    border-radius: var(--radius-md);
    background-color: var(--hud-toast-bg);
    color: var(--hud-toast-text);
  }

  .toast p {
    margin: 0;
  }

  .toast summary {
    cursor: pointer;
    margin-top: var(--space-2);
  }

  .toast ul {
    margin: var(--space-2) 0 0;
    padding-left: var(--space-4);
    list-style: disc;
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

  /* Wide: the slot spans the bottom of the window under the main content and the tile-sheet
     column, never under the rail. */
  .shell.has-ads {
    grid-template-rows: var(--space-hub-header-height) minmax(0, 1fr) auto;
    grid-template-areas:
      "header header"
      "rail main"
      "rail ad";
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  /* ── Header ─────────────────────────────────────────────────────────────────── */
  .hub-header {
    position: relative;
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

  .no-lan {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    min-height: var(--space-touch-min);
    padding: 0 var(--space-3);
    border-radius: var(--radius-full);
    background-color: var(--chip-offline-background);
    color: var(--chip-offline-foreground);
    text-decoration: none;
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

  .backup-alert {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    min-height: var(--space-touch-min);
    min-width: var(--space-touch-min);
    justify-content: center;
    text-decoration: none;
  }

  .alert-button {
    --ui-bg: transparent;
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    box-sizing: border-box;
    min-height: var(--space-touch-min);
    padding: 0 var(--space-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-full);
    color: var(--text);
    font: inherit;
    cursor: pointer;
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

  .gate.has-ads {
    grid-template-rows: var(--space-hub-header-height) minmax(0, 1fr) auto;
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
    /* A focused field or a scrolled-to row clears a sticky bar (--sticky-bottom, in px, unitless). */
    scroll-padding-block-end: calc(var(--sticky-bottom, 0) * 1px);
  }

  .chip-row {
    grid-area: chips;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
    padding: var(--space-2) var(--space-hub-gutter-phone);
    border-bottom: 1px solid var(--hub-line);
    background-color: var(--hub-header-background);
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
    :global(:root[data-ads="on"]) {
      --ad-reserved: calc(var(--ad-slot-height) + var(--space-9));
    }

    /* The slot sits between the main content and the bottom bar, with no gap row. */
    :global(:root[data-ads="on"]) .ad-area {
      padding-top: 0;
    }

    .shell {
      grid-template-columns: minmax(0, 1fr);
      grid-template-rows: var(--space-9) minmax(0, 1fr) var(--space-9);
      grid-template-areas:
        "header"
        "main"
        "bar";
    }

    .shell.has-chips {
      grid-template-rows: var(--space-9) auto minmax(0, 1fr) var(--space-9);
      grid-template-areas:
        "header"
        "chips"
        "main"
        "bar";
    }

    .shell.has-ads {
      grid-template-rows: var(--space-9) minmax(0, 1fr) auto var(--space-9);
      grid-template-areas:
        "header"
        "main"
        "ad"
        "bar";
    }

    .shell.has-ads.has-chips {
      grid-template-rows: var(--space-9) auto minmax(0, 1fr) auto var(--space-9);
      grid-template-areas:
        "header"
        "chips"
        "main"
        "ad"
        "bar";
    }

    .gate {
      grid-template-rows: var(--space-9) minmax(0, 1fr);
    }

    .gate.has-ads {
      grid-template-rows: var(--space-9) minmax(0, 1fr) auto;
    }

    /* The phone header: 64px, branch, staff, alerts; the chips sit in a row below it. */
    .hub-header {
      gap: var(--space-3);
      padding: 0 var(--space-hub-gutter-phone);
    }

    .header-end {
      flex: 1 1 auto;
      justify-content: flex-end;
      gap: var(--space-2);
    }

    .branch {
      flex: 0 1 auto;
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

    /* The bar is 64px with a 1px top border: items taller than the rest would push the page
       1px past the window, which moves the ad-banner slot against the overlays. */
    .bar-item {
      width: 100%;
      height: 100%;
      min-height: calc(var(--space-9) - 1px);
      border-radius: 0;
    }
  }
</style>
