<script lang="ts">
  import { formatMoney, formatTime, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { SnapshotLocale } from "@arena/ui/generated";
  import { onMount, tick } from "svelte";
  import {
    applyTheme,
    currentAttract,
    currentHubStatus,
    currentPcScreen,
    currentTheme,
    hudCollapse,
    hudExpand,
    hudLayout,
    onAttract,
    onHubStatus,
    onHudExpand,
    onPcScreen,
    onStaffMessage,
    onTheme,
    onToast,
    playerEnd,
    TOAST_KINDS,
    type PcScreen,
    type ToastKey,
  } from "./bridge";
  import {
    EXTEND_KEYS,
    clock,
    dropWarning,
    graceMinutes,
    hasContent,
    live,
    LOW_BALANCE_KEY,
    raise,
    raiseWarning,
    staffToastKey,
    type HudToast,
  } from "./hud";
  import ExtendOptions from "./ExtendOptions.svelte";

  // The overlay HUD (Story 6.14). It is its own always-on-top window: solid, never translucent,
  // and it never takes keyboard focus except while the player has expanded it with Ctrl+Shift+E.
  // This page draws; the Rust shell places, shows and hides the window for the size it reports.

  type Running = {
    from: number;
    /** When prepaid time runs out, on the monotonic clock (`null`: it counts up). */
    until: number | null;
    balance: number | null;
    plan: string | null;
  };

  let session = $state<Running | null>(null);
  /** A lock (paused, time's up, staff) covers the PC: nothing of the session shows. */
  let locked = $state(false);
  let locale: SnapshotLocale | null = $state(null);
  let now = $state(performance.now());
  let expanded = $state(false);
  /** The Hub link as the service last said (`null`: not told yet). */
  let hub: string | null = $state(null);
  /** "I'm done" (Story 6.18): the in-place question is showing. */
  let confirming = $state(false);
  let ending = $state(false);
  /** Why the last "I'm done" did not end the session. */
  let doneLine: "reconnecting" | "failed" | null = $state(null);
  let toasts: HudToast[] = $state([]);
  let nextId = 1;
  /** The warning mark (minutes) last seen on this session, so each is raised once (Story 6.19). */
  let lastWarning: number | null = null;
  /** The low-balance warning is in effect: it is raised once per crossing (Story 6.20). */
  let lowBalance = false;
  let root: HTMLDivElement | undefined = $state();
  let panel: HTMLDivElement | undefined = $state();

  const shown = $derived(session !== null && !locked);
  const prepaid = $derived(session?.until != null);
  /** Extend is for a profiled player on prepaid time (Story 6.21); postpaid and guests never see it. */
  const canExtend = $derived(prepaid && session?.balance != null);
  /** The Hub link is up, or not told yet: the extend panel disables itself on anything else. */
  const linked = $derived(hub === null || hub === "connected");
  const seconds = $derived(
    session === null
      ? 0
      : session.until !== null
        ? Math.max(0, Math.ceil((session.until - now) / 1000))
        : Math.max(0, Math.floor((now - session.from) / 1000)),
  );
  const time = $derived(clock(seconds));
  /** Guests see time only; a profiled player's balance needs the branch's money format. */
  const balance = $derived(
    session !== null && session.balance !== null && locale !== null
      ? formatMoney(session.balance, locale)
      : null,
  );
  const plan = $derived(
    session?.plan ?? t(prepaid ? "player.hud.plan_wallet" : "player.hud.plan_payg"),
  );
  const timeLabel = $derived(
    t(prepaid ? "player.shell.chip_left" : "player.shell.chip_played", { time }),
  );
  const pillLabel = $derived(
    balance === null ? timeLabel : t("player.hud.pill_label", { time: timeLabel, balance }),
  );

  const follow = (s: PcScreen, fresh = true) => {
    now = performance.now();
    locked = s.locked;
    const v = s.session;
    session = v
      ? {
          from: now - v.elapsed_ms,
          until: v.remaining_ms === null ? null : now + v.remaining_ms,
          // A guest has no wallet: the pill shows time only.
          balance: v.username === null ? null : v.balance_minor,
          plan: v.plan_name,
        }
      : null;
    // A session that ends or locks closes the panel; the shell must make the window
    // non-focusable again and hand the focus back, or the next Ctrl+Shift+E would not.
    if ((session === null || locked) && expanded) collapse();
    followWarning(s, fresh);
    followLowBalance(s, fresh);
  };

  /**
   * The low-balance warning (Story 6.20): raised when a wallet-paid session first falls to the
   * branch's amount, kept until dismissed, and raised again only after the balance has risen above
   * the amount and fallen back. It says the amount at the crossing, not the falling figure.
   */
  function followLowBalance(s: PcScreen, fresh: boolean) {
    const left = s.locked ? null : (s.session?.low_balance_minor ?? null);
    if (left === null) {
      lowBalance = false;
      toasts = dropWarning(toasts, LOW_BALANCE_KEY);
      return;
    }
    if (lowBalance) return;
    // A snapshot read at start only records it; with the money format not here yet the next
    // tick tries again.
    if (!fresh) lowBalance = true;
    if (!fresh || locale === null) return;
    lowBalance = true;
    toasts = raiseWarning(
      toasts,
      t("player.low_balance.title", { amount: formatMoney(left, locale) }),
      t("player.low_balance.detail"),
      nextId++,
      LOW_BALANCE_KEY,
      "player.low_balance.hotkey",
    );
    chime("warning");
  }

  /**
   * A time warning (Story 6.19): raised when the service reports a new mark, kept until it is
   * dismissed or time-up, and dropped when the session ends, locks or is extended past the mark.
   * A snapshot read at start (`fresh` false) only records the mark: it is not a new warning.
   */
  function followWarning(s: PcScreen, fresh: boolean) {
    const mark = s.locked ? null : (s.session?.warning_min ?? null);
    if (mark === null) {
      lastWarning = null;
      toasts = dropWarning(toasts);
      return;
    }
    if (mark === lastWarning) return;
    lastWarning = mark;
    if (!fresh || !s.session) return;
    const left = s.session.remaining_ms;
    const grace = graceMinutes(s.session.grace_ms);
    const at = left === null ? null : formatTime(Date.now() + left, locale?.tz);
    const detail =
      at === null
        ? null
        : t(grace > 0 ? "player.warning.ends" : "player.warning.ends_no_grace", {
            time: at,
            grace,
          });
    toasts = raiseWarning(toasts, t("player.warning.title", { minutes: mark }), detail, nextId++);
    chime("warning");
  }

  /** A short tone for a new toast: a two-note chime for a warning, one note for info. */
  function chime(kind: "info" | "warning") {
    try {
      const ctx = new AudioContext();
      const notes = kind === "warning" ? [660, 880] : [880];
      notes.forEach((hz, i) => {
        const osc = ctx.createOscillator();
        const gain = ctx.createGain();
        osc.frequency.value = hz;
        gain.gain.value = 0.15;
        osc.connect(gain).connect(ctx.destination);
        osc.start(ctx.currentTime + i * 0.18);
        osc.stop(ctx.currentTime + i * 0.18 + 0.14);
      });
      setTimeout(() => void ctx.close(), 600);
    } catch {
      // No audio device: the toast still shows.
    }
  }

  function raiseToast(key: ToastKey) {
    const kind = TOAST_KINDS[key];
    toasts = raise(toasts, key, kind, performance.now(), nextId++);
    chime(kind);
  }

  /** A staff message: an info toast with its words (Story 6.16). A replay is not raised again. */
  function raiseMessage(id: number, text: string) {
    toasts = raise(toasts, staffToastKey(id), "info", performance.now(), nextId++, text);
    chime("info");
  }

  async function expand() {
    if (!shown) return;
    expanded = true;
    await tick();
    // On prepaid time the extend options take the focus themselves once the Hub has quoted them.
    panel?.focus();
  }

  /** The Hub added the time (Story 6.21): any warning is moot, say so, and close the panel. */
  function added() {
    toasts = dropWarning(dropWarning(toasts), LOW_BALANCE_KEY);
    toasts = raise(toasts, "player.extend.done", "info", performance.now(), nextId++);
    chime("info");
    collapse();
  }

  /** Asks "End your session?" in place, unless the counter cannot be reached. */
  async function askDone() {
    doneLine = hub !== null && hub !== "connected" ? "reconnecting" : null;
    confirming = doneLine === null;
    await tick();
    panel?.querySelector<HTMLElement>("[data-testid=hud-done-cancel]")?.focus();
  }

  async function endSession() {
    if (ending) return;
    ending = true;
    const outcome = await playerEnd();
    ending = false;
    if (outcome.kind === "ok") return;
    doneLine = outcome.kind === "refused" ? "failed" : "reconnecting";
    confirming = false;
  }

  function collapse() {
    if (!expanded) return;
    confirming = false;
    doneLine = null;
    expanded = false;
    hudCollapse();
  }

  function onKeyDown(e: KeyboardEvent) {
    if (e.key === "Escape" && expanded) {
      e.preventDefault();
      // Esc on the question is "Cancel": nothing changes and the panel stays.
      if (confirming) confirming = false;
      else collapse();
    }
  }

  function report() {
    if (!root || !hasContent(shown, toasts)) {
      hudLayout(0, 0);
      return;
    }
    const box = root.getBoundingClientRect();
    hudLayout(Math.ceil(box.width), Math.ceil(box.height));
  }

  onMount(() => {
    const timer = setInterval(() => {
      now = performance.now();
      const kept = live(toasts, now);
      if (kept.length !== toasts.length) toasts = kept;
    }, 250);
    // Tell the shell how big the content is, so it can show, size and hide the window.
    const measure = new ResizeObserver(() => report());
    if (root) measure.observe(root);
    window.addEventListener("keydown", onKeyDown);
    const stops: Array<() => void> = [
      () => clearInterval(timer),
      () => measure.disconnect(),
      () => window.removeEventListener("keydown", onKeyDown),
    ];
    let gone = false;
    void (async () => {
      const add = (stop: () => void) => (gone ? stop() : stops.push(stop));
      // An event is newer than the reply to the command asked after it is listened to.
      let themed = false;
      let screened = false;
      let attracted = false;
      add(
        await onTheme((hc) => {
          themed = true;
          applyTheme(hc);
        }),
      );
      add(
        await onPcScreen((s) => {
          screened = true;
          follow(s);
        }),
      );
      add(await onToast(raiseToast));
      add(
        await onStaffMessage((m) => {
          if (m.fresh) raiseMessage(m.id, m.text);
        }),
      );
      add(await onHudExpand(() => void expand()));
      add(await onHubStatus((s) => (hub = s.state)));
      add(
        await onAttract((s) => {
          attracted = true;
          locale = s.view?.locale ?? locale;
        }),
      );
      const link = await currentHubStatus();
      if (link && hub === null) hub = link.state;
      const screen = await currentPcScreen();
      if (screen && !screened) follow(screen, false);
      const known = await currentAttract();
      if (known && !attracted) locale = known.view?.locale ?? locale;
      const hc = await currentTheme();
      if (hc !== null && !themed) applyTheme(hc);
    })();
    return () => {
      gone = true;
      for (const stop of stops) stop();
    };
  });

  // Content appearing or going (a session, a lock, a toast) changes the size or empties it.
  $effect(() => {
    void shown;
    void toasts.length;
    void expanded;
    void tick().then(report);
  });
</script>

<svelte:head>
  <title>{t("app.title", { surface: t("surface.kiosk") })}</title>
</svelte:head>

<div class="hud" bind:this={root} data-testid="hud">
  <!-- Always in the page, so a screen reader announces a toast raised later. -->
  <div class="toasts" aria-live="polite" data-testid="toasts">
    {#each toasts as toast (toast.id)}
      <div class="toast" data-kind={toast.kind} data-testid="toast">
        <div class="words">
          <p class="type-kiosk-body" data-testid="toast-text">
            {toast.text ?? t(toast.key as MessageKey)}
          </p>
          {#if toast.detail}
            <p class="type-kiosk-body" data-testid="toast-detail">{toast.detail}</p>
          {/if}
          {#if toast.hotkey}
            <p
              class="keys"
              role="img"
              aria-label={t((toast.hotkeyLabel ?? "player.warning.hotkey") as MessageKey)}
              data-testid="toast-keys"
            >
              {#each EXTEND_KEYS as key, i (key)}
                {#if i > 0}<span aria-hidden="true">+</span>{/if}
                <kbd class="cap type-kiosk-label" aria-hidden="true">{key}</kbd>
              {/each}
            </p>
          {/if}
        </div>
        <div class="buttons">
          {#if toast.hotkey && canExtend}
            <button
              type="button"
              class="extend type-kiosk-label"
              data-testid="toast-extend"
              onclick={hudExpand}
            >
              {t("player.extend.button")}
            </button>
          {/if}
          <button
            type="button"
            class="dismiss type-kiosk-label"
            onclick={() => (toasts = toasts.filter((x) => x.id !== toast.id))}
          >
            {t("common.dismiss")}
          </button>
        </div>
      </div>
    {/each}
  </div>
  {#if shown && !expanded}
    <p class="pill" role="timer" aria-label={pillLabel} data-testid="hud-pill">
      <span class="num type-hud-numeric tabular-nums" data-testid="hud-time">{time}</span>
      {#if balance !== null}
        <span class="rule" aria-hidden="true"></span>
        <span class="num type-hud-numeric tabular-nums" data-testid="hud-balance">{balance}</span>
      {/if}
    </p>
  {/if}
  {#if shown && expanded}
    <div
      class="panel"
      role="dialog"
      aria-label={t("player.hud.panel_title")}
      tabindex="-1"
      bind:this={panel}
      data-testid="hud-panel"
    >
      <p class="title type-kiosk-label">{t("player.hud.panel_title")}</p>
      <dl>
        <div>
          <dt class="type-kiosk-body">
            {t(prepaid ? "player.hud.time_left" : "player.hud.time_played")}
          </dt>
          <dd class="type-hud-numeric tabular-nums" data-testid="hud-panel-time">{time}</dd>
        </div>
        {#if balance !== null}
          <div>
            <dt class="type-kiosk-body">{t("player.hud.balance")}</dt>
            <dd class="type-hud-numeric tabular-nums" data-testid="hud-panel-balance">
              {balance}
            </dd>
          </div>
        {/if}
        <div>
          <dt class="type-kiosk-body">{t("player.hud.plan")}</dt>
          <dd class="type-kiosk-body" data-testid="hud-panel-plan">{plan}</dd>
        </div>
      </dl>
      {#if canExtend}
        <ExtendOptions {locale} connected={linked} onadded={added} />
      {/if}
      {#if !prepaid}
        {#if confirming}
          <div class="ask" role="alert" data-testid="hud-done-confirm">
            <p class="type-kiosk-body">{t("player.account.done_confirm")}</p>
            <div class="foot">
              <button
                type="button"
                class="dismiss type-kiosk-label"
                aria-disabled={ending ? "true" : undefined}
                data-testid="hud-done-yes"
                onclick={endSession}
              >
                {t("player.account.done_yes")}
              </button>
              <button
                type="button"
                class="dismiss type-kiosk-label"
                data-testid="hud-done-cancel"
                onclick={() => (confirming = false)}
              >
                {t("player.account.done_cancel")}
              </button>
            </div>
          </div>
        {:else}
          <button
            type="button"
            class="dismiss type-kiosk-label"
            data-testid="hud-done"
            onclick={() => void askDone()}
          >
            {t("player.account.row.done")}
          </button>
        {/if}
        {#if doneLine}
          <p class="hint type-kiosk-body" role="alert" data-testid="hud-done-line">
            ⚠ {t(
              doneLine === "reconnecting"
                ? "player.hub.reconnecting_retry"
                : "player.account.logout_failed",
            )}
          </p>
        {/if}
      {/if}
      <div class="foot">
        <p class="hint type-kiosk-body">{t("player.hud.hint")}</p>
        <button type="button" class="dismiss type-kiosk-label" onclick={collapse}>
          {t("player.hud.close")}
        </button>
      </div>
    </div>
  {/if}
</div>

<style>
  :global(html),
  :global(body) {
    margin: 0;
    overflow: hidden;
    background-color: var(--bg);
    user-select: none;
  }

  /* The window is exactly this box: the shell sizes it to what it measures. */
  .hud {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    width: max-content;
    max-width: 640px;
    color: var(--text);
  }

  .pill {
    display: inline-flex;
    align-items: center;
    gap: var(--space-3);
    height: 56px;
    box-sizing: border-box;
    margin: 0;
    padding: 0 var(--space-5);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-full);
    background-color: var(--hud-pill-background);
    color: var(--hud-pill-foreground);
  }

  .rule {
    width: 1px;
    height: 24px;
    background-color: var(--border-strong);
  }

  .toasts:not(:empty) {
    margin-bottom: var(--space-2);
  }

  .toasts {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    width: 640px;
    max-width: 100%;
  }

  .toast,
  .panel {
    box-sizing: border-box;
    border: 2px solid var(--hud-toast-title);
    background-color: var(--hud-toast-background);
    color: var(--hud-toast-foreground);
  }

  .toast {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-5);
    padding: var(--space-4) var(--space-5);
    border-radius: var(--radius-xl);
  }

  .toast p {
    margin: 0;
    overflow-wrap: anywhere;
  }

  .words {
    display: grid;
    gap: var(--space-1);
    min-width: 0;
  }

  .keys {
    display: flex;
    align-items: center;
    gap: var(--space-1);
  }

  /* A key cap: solid, outlined in the toast colour, readable in HC without the accent. */
  .cap {
    padding: 0 var(--space-2);
    border: 2px solid var(--hud-toast-title);
    border-radius: var(--radius-md);
    background: transparent;
    color: var(--hud-toast-foreground);
  }

  .panel {
    display: grid;
    gap: var(--space-3);
    width: 360px;
    padding: var(--space-5);
    border-radius: var(--radius-xl);
  }

  .panel:focus-visible {
    outline: 3px solid var(--hud-toast-title);
    outline-offset: 2px;
  }

  .title {
    margin: 0;
    color: var(--hud-toast-title);
  }

  dl {
    display: grid;
    gap: var(--space-2);
    margin: 0;
  }

  dl div {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-4);
  }

  dt,
  dd {
    margin: 0;
  }

  .foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
  }

  .hint,
  .ask p {
    margin: 0;
  }

  .ask {
    display: grid;
    gap: var(--space-3);
  }

  .buttons {
    display: flex;
    flex: none;
    align-items: center;
    gap: var(--space-3);
  }

  .dismiss,
  .extend {
    flex: none;
    min-height: 44px;
    padding: var(--space-2) var(--space-5);
    border: 2px solid var(--hud-toast-title);
    border-radius: var(--radius-full);
    background: transparent;
    color: var(--hud-toast-foreground);
    cursor: pointer;
  }

  /* Extend is the warning button: solid, the one filled control on a warning toast. */
  .extend {
    background-color: var(--button-warning-background);
    color: var(--button-warning-foreground);
  }

  .dismiss:focus-visible,
  .extend:focus-visible {
    outline: 3px solid var(--hud-toast-title);
    outline-offset: 2px;
  }

  /* HC: the shared 4px focus ring and 2px white outline, never the toast's own colours. */
  :global([data-theme="hc"]) .dismiss,
  :global([data-theme="hc"]) .extend {
    outline: 2px solid var(--text);
    outline-offset: 0;
  }

  :global([data-theme="hc"]) .dismiss:focus-visible,
  :global([data-theme="hc"]) .extend:focus-visible {
    outline: 4px solid var(--focus-ring);
    outline-offset: 2px;
  }
</style>
