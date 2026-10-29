<script lang="ts">
  import { formatMoney, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { SnapshotLocale } from "@arena/ui/generated";
  import { onMount, tick } from "svelte";
  import {
    applyTheme,
    currentAttract,
    currentPcScreen,
    currentTheme,
    hudCollapse,
    hudLayout,
    onAttract,
    onHudExpand,
    onPcScreen,
    onTheme,
    onToast,
    TOAST_KINDS,
    type PcScreen,
    type ToastKey,
  } from "./bridge";
  import { clock, hasContent, live, raise, type HudToast } from "./hud";

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
  let toasts: HudToast[] = $state([]);
  let nextId = 1;
  let root: HTMLDivElement | undefined = $state();
  let panel: HTMLDivElement | undefined = $state();

  const shown = $derived(session !== null && !locked);
  const prepaid = $derived(session?.until != null);
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

  const follow = (s: PcScreen) => {
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
  };

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

  async function expand() {
    if (!shown) return;
    expanded = true;
    await tick();
    panel?.focus();
  }

  function collapse() {
    if (!expanded) return;
    expanded = false;
    hudCollapse();
  }

  function onKeyDown(e: KeyboardEvent) {
    if (e.key === "Escape" && expanded) {
      e.preventDefault();
      collapse();
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
      add(await onHudExpand(() => void expand()));
      add(
        await onAttract((s) => {
          attracted = true;
          locale = s.view?.locale ?? locale;
        }),
      );
      const screen = await currentPcScreen();
      if (screen && !screened) follow(screen);
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
        <p class="type-kiosk-body">{t(toast.key as MessageKey)}</p>
        <button
          type="button"
          class="dismiss type-kiosk-label"
          onclick={() => (toasts = toasts.filter((x) => x.id !== toast.id))}
        >
          {t("common.dismiss")}
        </button>
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

  .hint {
    margin: 0;
  }

  .dismiss {
    flex: none;
    min-height: 44px;
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
</style>
