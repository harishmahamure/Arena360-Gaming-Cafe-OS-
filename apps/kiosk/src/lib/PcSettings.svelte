<script lang="ts">
  import { Button, t } from "@arena/ui";
  import type { PcSettings, PcSettingsPanel } from "@arena/ui/generated";
  import { onDestroy, onMount, tick } from "svelte";
  import { pcSettingsKeep, pcSettingsSet, playerPcSettings } from "./bridge";
  import {
    MOUSE_MAX,
    MOUSE_MIN,
    clampSpeed,
    modesToList,
    parseMode,
    type ChangeOutcome,
  } from "./pc-settings";

  // The PC settings panel (Story 6.35), opened from the account menu: mouse speed (Windows 1 to
  // 20), display mode, sound output and keyboard layout. Every change is applied at once by the
  // Kiosk service; for a profiled player the Hub saves it to their profile, a guest's lasts this
  // session. A new display mode waits for "Keep this" and goes back by itself after 15 s: the
  // service owns that clock, this panel only shows its countdown. Nothing here decides anything.

  let {
    profiled,
    onback,
  }: {
    /** A profiled player: their choices are saved and follow them to any PC. */
    profiled: boolean;
    /** Back to the account menu's rows. */
    onback: () => void;
  } = $props();

  /** What the service said: `null` while it loads, `false` when it did not answer. */
  let panel: PcSettingsPanel | false | null = $state(null);
  /** The slider's value while it is being dragged. */
  let speed = $state(10);
  /** The line under the controls: why the last change did not stick, or that it was reverted. */
  let line: "reconnecting" | "not_offered" | "failed" | "reverted" | null = $state(null);
  /** When the new display mode goes back, as a `performance.now()` value; `null`: none waits. */
  let deadline: number | null = $state(null);
  let now = $state(performance.now());
  /** The mode that was picked and waits to be kept. */
  let waiting: string | null = $state(null);
  let busy = $state(false);
  let expiring = false;
  let mouse: HTMLInputElement | undefined = $state();
  let ticker: ReturnType<typeof setInterval> | undefined;

  const left = $derived(deadline === null ? 0 : Math.max(0, Math.ceil((deadline - now) / 1000)));
  const lineText = $derived(
    line === "reconnecting"
      ? t("player.hub.reconnecting_retry")
      : line === "not_offered"
        ? t("player.pcsettings.not_offered")
        : line === "failed"
          ? t("player.pcsettings.change_failed")
          : line === "reverted"
            ? t("player.pcsettings.reverted")
            : "",
  );

  function modeText(mode: string): string {
    const m = parseMode(mode);
    return m ? t("player.pcsettings.display_mode", m) : mode;
  }

  async function load() {
    const result = await playerPcSettings();
    if (result.kind !== "ok") {
      panel = false;
      return;
    }
    panel = result.panel;
    speed = clampSpeed(result.panel.mouse_speed);
    // The service owns the clock: a display change waits for as long as it says.
    const ms = result.panel.revert_ms;
    deadline = ms != null && ms > 0 ? performance.now() + ms : null;
  }

  /** The countdown ran out: the service puts the old mode back; show what it says. */
  async function expired() {
    if (expiring) return;
    expiring = true;
    const picked = waiting;
    try {
      // The service's own timer may fire a moment after this one.
      for (let tries = 0; tries < 8; tries++) {
        await load();
        if (panel === false || panel === null || panel.revert_ms == null) break;
        await new Promise((resolve) => setTimeout(resolve, 250));
      }
    } finally {
      expiring = false;
    }
    waiting = null;
    if (panel !== false && panel !== null && picked !== null && panel.display_mode !== picked) {
      line = "reverted";
    }
  }

  function onTick() {
    now = performance.now();
    if (deadline !== null && now >= deadline) void expired();
  }

  onMount(() => {
    // Focus goes to the first control once it is there.
    void load().then(async () => {
      await tick();
      mouse?.focus();
    });
    ticker = setInterval(onTick, 250);
  });

  onDestroy(() => clearInterval(ticker));

  /** Notes why a change did not stick; `true` when it was applied. */
  function took(outcome: ChangeOutcome): boolean {
    line = null;
    if (outcome.kind === "reconnecting") line = "reconnecting";
    else if (outcome.kind === "not_offered") line = "not_offered";
    else if (outcome.kind === "failed") line = "failed";
    return outcome.kind === "ok";
  }

  async function change(patch: PcSettings) {
    if (busy) return;
    busy = true;
    line = null;
    const outcome = await pcSettingsSet(patch);
    busy = false;
    if (took(outcome) && outcome.kind === "ok" && outcome.revertMs !== null) {
      waiting = patch.display_mode ?? null;
    }
    // Whatever came of it, show what the PC is really set to now (and any countdown).
    await load();
  }

  function onMouse(event: Event) {
    speed = clampSpeed(Number((event.currentTarget as HTMLInputElement).value));
    void change({ mouse_speed: speed });
  }

  function onDisplay(event: Event) {
    const mode = (event.currentTarget as HTMLSelectElement).value;
    if (panel && panel.display_mode !== mode) void change({ display_mode: mode });
  }

  function onAudio(event: Event) {
    void change({ audio_output: (event.currentTarget as HTMLSelectElement).value });
  }

  function onLayout(event: Event) {
    void change({ keyboard_layout: (event.currentTarget as HTMLSelectElement).value });
  }

  async function keep() {
    if (busy) return;
    busy = true;
    line = null;
    const outcome = await pcSettingsKeep();
    busy = false;
    waiting = null;
    if (outcome.kind === "reconnecting") line = "reconnecting";
    else if (outcome.kind === "gone") line = "reverted";
    else if (outcome.kind === "failed") line = "failed";
    await load();
  }
</script>

<section class="pc-settings" aria-label={t("player.pcsettings.title")} data-testid="pc-settings">
  <h2 class="type-kiosk-label">{t("player.pcsettings.title")}</h2>

  {#if panel === null}
    <p class="type-kiosk-body" role="status" data-testid="pc-settings-loading">
      {t("player.pcsettings.loading")}
    </p>
  {:else if panel === false}
    <p class="type-kiosk-body" role="alert" data-testid="pc-settings-unavailable">
      {t("player.pcsettings.unavailable")}
    </p>
  {:else}
    <div class="field">
      <label class="type-kiosk-label" for="pc-settings-mouse">{t("player.pcsettings.mouse")}</label>
      <div class="slider">
        <input
          id="pc-settings-mouse"
          bind:this={mouse}
          data-first
          type="range"
          min={MOUSE_MIN}
          max={MOUSE_MAX}
          step="1"
          value={speed}
          style:--pct={((speed - MOUSE_MIN) / (MOUSE_MAX - MOUSE_MIN)) * 100 + "%"}
          aria-valuetext={t("player.pcsettings.mouse_value", { speed })}
          data-testid="pc-settings-mouse"
          oninput={(e) => (speed = clampSpeed(Number(e.currentTarget.value)))}
          onchange={onMouse}
        />
        <output class="level type-kiosk-label tabular-nums" data-testid="pc-settings-mouse-value">
          {t("player.pcsettings.mouse_value", { speed })}
        </output>
      </div>
    </div>

    <div class="field">
      <label class="type-kiosk-label" for="pc-settings-display">
        {t("player.pcsettings.display")}
      </label>
      <select
        id="pc-settings-display"
        class="type-kiosk-body"
        value={panel.display_mode}
        disabled={busy || deadline !== null}
        data-testid="pc-settings-display"
        onchange={onDisplay}
      >
        {#each modesToList(panel) as mode (mode)}
          <option value={mode}>{modeText(mode)}</option>
        {/each}
      </select>
    </div>

    {#if deadline !== null}
      <div class="keep" role="alert" data-testid="pc-settings-keep-box">
        <p class="type-kiosk-body" data-testid="pc-settings-countdown">
          {t("player.pcsettings.keep_prompt", { seconds: left })}
        </p>
        <Button
          label="player.pcsettings.keep"
          disabled={busy}
          disabledReason="player.unlock.checking"
          onclick={keep}
        />
      </div>
    {/if}

    <div class="field">
      <label class="type-kiosk-label" for="pc-settings-audio">{t("player.pcsettings.audio")}</label>
      {#if panel.audio_outputs.length === 0}
        <p class="type-kiosk-body none" data-testid="pc-settings-audio-none">
          {t("player.pcsettings.none")}
        </p>
      {:else}
        <select
          id="pc-settings-audio"
          class="type-kiosk-body"
          value={panel.audio_output ?? ""}
          disabled={busy}
          data-testid="pc-settings-audio"
          onchange={onAudio}
        >
          {#if !panel.audio_output}<option value="" disabled></option>{/if}
          {#each panel.audio_outputs as name (name)}
            <option value={name}>{name}</option>
          {/each}
        </select>
      {/if}
    </div>

    <div class="field">
      <label class="type-kiosk-label" for="pc-settings-layout"
        >{t("player.pcsettings.layout")}</label
      >
      {#if panel.keyboard_layouts.length === 0}
        <p class="type-kiosk-body none" data-testid="pc-settings-layout-none">
          {t("player.pcsettings.none")}
        </p>
      {:else}
        <select
          id="pc-settings-layout"
          class="type-kiosk-body"
          value={panel.keyboard_layout ?? ""}
          disabled={busy}
          data-testid="pc-settings-layout"
          onchange={onLayout}
        >
          {#if !panel.keyboard_layout}<option value="" disabled></option>{/if}
          {#each panel.keyboard_layouts as layout (layout.id)}
            <option value={layout.id}>{layout.name}</option>
          {/each}
        </select>
      {/if}
    </div>

    <p class="note type-kiosk-label" data-testid="pc-settings-note">
      {t(profiled ? "player.pcsettings.saved_note" : "player.pcsettings.guest_note")}
    </p>
  {/if}

  <p
    class="line type-kiosk-label"
    role={lineText ? "alert" : undefined}
    data-testid="pc-settings-line"
  >
    {#if lineText}<span aria-hidden="true">⚠</span> {lineText}{/if}
  </p>

  <div class="actions">
    <Button variant="secondary" label="player.pcsettings.back" onclick={onback} />
  </div>
</section>

<style>
  .pc-settings {
    display: grid;
    gap: var(--space-3);
  }

  .pc-settings h2,
  .pc-settings p {
    margin: 0;
  }

  .pc-settings h2 {
    color: var(--text-muted);
  }

  .field {
    display: grid;
    gap: var(--space-1);
  }

  .slider {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .level {
    min-width: 6ch;
    text-align: right;
  }

  select {
    box-sizing: border-box;
    width: 100%;
    min-height: var(--space-touch-min);
    padding: 0 var(--space-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    background-color: var(--surface-2);
    color: inherit;
  }

  select:disabled {
    opacity: 0.6;
  }

  select:focus-visible {
    outline: 3px solid var(--focus-ring);
    outline-offset: 2px;
  }

  .keep {
    display: grid;
    gap: var(--space-2);
    padding: var(--space-3);
    border: 2px solid var(--warning);
    border-radius: var(--radius-md);
  }

  .none,
  .note {
    color: var(--text-muted);
  }

  .line {
    min-height: 1.5em;
    color: var(--warning);
  }

  /* The visible thumb is a 20px disc inside a 44px hit area (as the volume slider). */
  input[type="range"] {
    flex: 1;
    min-width: 0;
    height: var(--space-touch-min);
    margin: 0;
    background: transparent;
    appearance: none;
    cursor: pointer;
  }

  input[type="range"]::-webkit-slider-runnable-track {
    height: 4px;
    border-radius: var(--radius-full);
    background: linear-gradient(
      to right,
      var(--volume-control-fill) var(--pct),
      var(--volume-control-track) var(--pct)
    );
  }

  input[type="range"]::-webkit-slider-thumb {
    box-sizing: border-box;
    width: var(--space-touch-min);
    height: var(--space-touch-min);
    margin-top: -20px;
    border: 12px solid transparent;
    border-radius: var(--radius-full);
    background-color: var(--volume-control-fill);
    background-clip: content-box;
    appearance: none;
  }

  input[type="range"]:focus-visible {
    outline: 3px solid var(--focus-ring);
    outline-offset: 2px;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }
</style>
