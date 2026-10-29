<script lang="ts">
  import { onMount, tick } from "svelte";
  import { t } from "@arena/ui";
  import { setVolume, volumeLevel } from "./bridge";
  import { VOLUME_STEP, clampVolume } from "./volume";

  let open = $state(false);
  let percent = $state(50);
  let root: HTMLDivElement | undefined = $state();
  let button: HTMLButtonElement | undefined = $state();
  let slider: HTMLInputElement | undefined = $state();

  /** One change in flight at a time; the newest value waits behind it. */
  let sending = false;
  let waiting: number | null = null;

  async function send(value: number) {
    if (sending) {
      waiting = value;
      return;
    }
    sending = true;
    try {
      await setVolume(value);
    } finally {
      sending = false;
    }
    if (waiting !== null) {
      const next = waiting;
      waiting = null;
      await send(next);
    }
  }

  async function sync() {
    const level = await volumeLevel();
    if (level !== null && !sending) percent = clampVolume(level);
  }

  async function toggle() {
    if (open) return close(false);
    open = true;
    void sync();
    await tick();
    slider?.focus();
  }

  function close(refocus: boolean) {
    open = false;
    if (refocus) button?.focus();
  }

  function onInput(event: Event) {
    percent = clampVolume(Number((event.currentTarget as HTMLInputElement).value));
    void send(percent);
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Escape" && open) {
      event.preventDefault();
      event.stopPropagation();
      close(true);
    }
  }

  function onFocusout(event: FocusEvent) {
    const next = event.relatedTarget as Node | null;
    if (open && next && root && !root.contains(next)) open = false;
  }

  onMount(() => {
    void sync();
    const away = (e: PointerEvent) => {
      if (open && root && !root.contains(e.target as Node)) open = false;
    };
    window.addEventListener("pointerdown", away);
    return () => window.removeEventListener("pointerdown", away);
  });
</script>

<div
  class="volume-control"
  data-testid="volume-control"
  bind:this={root}
  onkeydown={onKeydown}
  onfocusout={onFocusout}
  role="presentation"
>
  <button
    bind:this={button}
    type="button"
    class="speaker"
    aria-label={t("player.volume.label")}
    aria-expanded={open}
    aria-controls="volume-panel"
    data-testid="volume-button"
    onclick={toggle}
  >
    <svg viewBox="0 0 24 24" width="24" height="24" aria-hidden="true" focusable="false">
      <path d="M4 9v6h4l5 4V5L8 9H4z" fill="currentColor" />
      {#if percent === 0}
        <path
          d="M16 9l5 6M21 9l-5 6"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          fill="none"
        />
      {:else}
        <path
          d="M16 9.5a4 4 0 0 1 0 5M18.5 7a7.5 7.5 0 0 1 0 10"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          fill="none"
        />
      {/if}
    </svg>
  </button>

  {#if open}
    <div class="panel" id="volume-panel" data-testid="volume-panel">
      <input
        bind:this={slider}
        type="range"
        min="0"
        max="100"
        step={VOLUME_STEP}
        value={percent}
        style:--pct={percent + "%"}
        aria-label={t("player.volume.slider")}
        aria-valuetext={t("player.volume.value", { percent })}
        data-testid="volume-slider"
        oninput={onInput}
      />
      <output class="level type-kiosk-label tabular-nums" data-testid="volume-value">
        {t("player.volume.value", { percent })}
      </output>
    </div>
  {/if}
</div>

<style>
  .volume-control {
    position: relative;
  }

  .speaker {
    display: inline-grid;
    place-content: center;
    width: var(--space-touch-min);
    height: var(--space-touch-min);
    padding: 0;
    border: 0;
    border-radius: var(--radius-full);
    background-color: var(--volume-control-background);
    color: var(--volume-control-foreground);
    cursor: pointer;
  }

  .speaker:focus-visible,
  .speaker:focus {
    outline: 3px solid var(--focus-ring);
    outline-offset: 2px;
  }

  .panel {
    position: absolute;
    top: calc(100% + var(--space-2));
    right: 0;
    z-index: 5;
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-width: 280px;
    padding: var(--space-2) var(--space-4);
    border: 1px solid var(--line);
    border-radius: var(--radius-full);
    background-color: var(--volume-control-background);
    color: var(--volume-control-foreground);
  }

  .level {
    min-width: 4ch;
    text-align: right;
  }

  /* The visible thumb is a 20px disc inside a 44px hit area. */
  input[type="range"] {
    flex: 1;
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
</style>
