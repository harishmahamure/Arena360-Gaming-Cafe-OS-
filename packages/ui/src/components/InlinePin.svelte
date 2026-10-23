<script lang="ts">
  import { onDestroy, onMount, tick } from "svelte";
  import { t, type MessageKey } from "../messages/index.js";
  import Button from "./Button.svelte";

  /** What the caller's check of a PIN came to. A refusal names a catalogue message. */
  type PinResult =
    | { ok: true }
    | {
        ok: false;
        message: MessageKey;
        /** Entry pauses for this long (`AUTH_LOCKED`), with a visible countdown. */
        retryAfterS?: number;
      };

  /**
   * In-place manager PIN entry for PINs of `minLength`–`maxLength` digits (4–6): `maxLength`
   * cells over a visually hidden numeric input, plus a keypad. Digits, Backspace and Enter work
   * from the keyboard; Enter or ✓ submits once `minLength` digits are in, and the last cell
   * submits on its own. Esc clears the PIN, calls `oncancel` and returns focus to
   * `returnFocusTo` (not while a PIN is being checked). The PIN lives only in this component's
   * state and is cleared the moment it is submitted, whatever the outcome. The live region
   * announces how many cells are filled, never the digits. `label` names the PIN input and
   * `submitLabel` the ✓ key; with `cancellable` off there is no Cancel button (Esc still clears
   * the PIN and calls `oncancel`).
   */
  let {
    minLength = 4,
    maxLength = 6,
    onsubmit,
    oncancel,
    message = "ui.pin.prompt",
    returnFocusTo = null,
    autofocus = true,
    label = "ui.pin.label",
    submitLabel = "ui.pin.key_submit",
    cancellable = true,
  }: {
    minLength?: 4 | 5 | 6;
    maxLength?: 4 | 5 | 6;
    onsubmit: (pin: string) => Promise<PinResult>;
    oncancel: () => void;
    /** The prompt shown above the cells. */
    message?: MessageKey;
    returnFocusTo?: HTMLElement | null;
    /** Focus the PIN as soon as it opens (the gallery's static samples turn this off). */
    autofocus?: boolean;
    /** The PIN input's accessible name. */
    label?: MessageKey;
    /** The ✓ key's accessible name. */
    submitLabel?: MessageKey;
    /** Show the Cancel button. */
    cancellable?: boolean;
  } = $props();

  const id = $props.id();
  const promptId = id + "-prompt";
  const feedbackId = id + "-feedback";
  const describedWithFeedback = [promptId, feedbackId].join(" ");

  const cells = $derived(Math.min(6, Math.max(4, maxLength)));
  const least = $derived(Math.min(cells, Math.max(4, minLength)));
  let pin = $state("");
  let busy = $state(false);
  let feedback: { message: MessageKey; params?: Record<string, number> } | null = $state(null);
  let lockedFor = $state(0);
  let timer: ReturnType<typeof setInterval> | null = null;
  let input: HTMLInputElement | undefined = $state();

  const locked = $derived(lockedFor > 0);

  function clearTimer() {
    if (timer !== null) clearInterval(timer);
    timer = null;
  }

  function lock(seconds: number) {
    clearTimer();
    lockedFor = Math.max(1, Math.ceil(seconds));
    feedback = { message: "ui.pin.locked", params: { seconds: lockedFor } };
    timer = setInterval(() => {
      lockedFor -= 1;
      if (lockedFor <= 0) {
        clearTimer();
        lockedFor = 0;
        feedback = null;
      } else {
        feedback = { message: "ui.pin.locked", params: { seconds: lockedFor } };
      }
    }, 1000);
  }

  function setPin(next: string) {
    pin = next.replace(/\D/g, "").slice(0, cells);
    if (input && input.value !== pin) input.value = pin;
  }

  /** Every cell filled: submit without waiting for Enter. */
  function submitWhenFull() {
    if (pin.length === cells) void submit();
  }

  function type(digit: string) {
    if (busy || locked) return;
    if (feedback?.message !== "ui.pin.locked") feedback = null;
    setPin(pin + digit);
    submitWhenFull();
  }

  function backspace() {
    if (busy || locked) return;
    setPin(pin.slice(0, -1));
  }

  async function submit() {
    if (busy || locked) return;
    if (pin.length < least) {
      feedback = { message: "ui.pin.incomplete", params: { length: least } };
      return;
    }
    // Hand the PIN over and forget it at once, whatever the outcome.
    let value = pin;
    setPin("");
    busy = true;
    feedback = null;
    let result: PinResult;
    try {
      result = await onsubmit(value);
    } catch {
      result = { ok: false, message: "ui.pin.unreachable" };
    } finally {
      value = "";
      busy = false;
    }
    if (result.ok) return;
    if (result.retryAfterS !== undefined) lock(result.retryAfterS);
    else feedback = { message: result.message };
    await tick();
    input?.focus();
  }

  function cancel() {
    // A PIN being checked cannot be taken back: wait for its answer.
    if (busy) return;
    setPin("");
    feedback = null;
    oncancel();
    returnFocusTo?.focus();
  }

  /** Keys typed anywhere in the panel (the input handles its own text through `oninput`). */
  function onkeydown(event: KeyboardEvent) {
    if (event.altKey || event.ctrlKey || event.metaKey) return;
    const fromInput = event.target === input;
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      cancel();
    } else if (event.key === "Enter") {
      // Never the surrounding form's implicit submit.
      if (fromInput || !(event.target instanceof HTMLButtonElement)) {
        event.preventDefault();
        void submit();
      }
    } else if (!fromInput && /^\d$/.test(event.key)) {
      event.preventDefault();
      type(event.key);
    } else if (!fromInput && event.key === "Backspace") {
      event.preventDefault();
      backspace();
    }
  }

  function oninput() {
    if (!input) return;
    if (busy || locked) {
      input.value = pin;
      return;
    }
    if (feedback?.message !== "ui.pin.locked") feedback = null;
    setPin(input.value);
    submitWhenFull();
  }

  onMount(() => {
    if (autofocus) input?.focus();
  });

  onDestroy(() => {
    clearTimer();
    pin = "";
  });

  const KEYS = ["1", "2", "3", "4", "5", "6", "7", "8", "9"];
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="inline-pin"
  data-component="inline-pin"
  role="group"
  aria-labelledby={promptId}
  {onkeydown}
>
  <p class="prompt" id={promptId}>{t(message)}</p>

  <div class="cells-wrap">
    <div class="cells" aria-hidden="true">
      {#each { length: cells }, i (i)}
        <span class="cell" class:filled={i < pin.length}>{i < pin.length ? "●" : ""}</span>
      {/each}
    </div>
    <input
      bind:this={input}
      class="entry"
      type="password"
      inputmode="numeric"
      autocomplete="off"
      maxlength={cells}
      aria-label={t(label)}
      aria-describedby={feedback ? describedWithFeedback : promptId}
      aria-invalid={feedback && feedback.message !== "ui.pin.incomplete" ? "true" : undefined}
      readonly={busy || locked}
      {oninput}
      data-1p-ignore
      data-lpignore="true"
    />
  </div>

  <p class="visually-hidden" aria-live="polite" aria-atomic="true">
    {t("ui.pin.count", { count: pin.length, length: cells })}
  </p>

  <p class="feedback" id={feedbackId} aria-live="assertive">
    {#if busy}
      <span>{t("ui.pin.checking")}</span>
    {:else if feedback}
      <span class="glyph" aria-hidden="true">⚠</span>
      <span>{t(feedback.message, feedback.params)}</span>
    {/if}
  </p>

  <div class="keypad">
    {#each KEYS as key (key)}
      <button
        type="button"
        class="ui-interactive key"
        aria-disabled={locked ? "true" : undefined}
        aria-describedby={locked ? feedbackId : undefined}
        onclick={() => type(key)}>{key}</button
      >
    {/each}
    <button
      type="button"
      class="ui-interactive key"
      aria-label={t("ui.pin.key_delete")}
      aria-disabled={locked ? "true" : undefined}
      aria-describedby={locked ? feedbackId : undefined}
      onclick={backspace}><span aria-hidden="true">⌫</span></button
    >
    <button
      type="button"
      class="ui-interactive key"
      aria-disabled={locked ? "true" : undefined}
      aria-describedby={locked ? feedbackId : undefined}
      onclick={() => type("0")}>0</button
    >
    <button
      type="button"
      class="ui-interactive key key--submit"
      aria-label={t(submitLabel)}
      aria-disabled={locked ? "true" : undefined}
      aria-describedby={locked ? feedbackId : undefined}
      onclick={() => void submit()}><span aria-hidden="true">✓</span></button
    >
  </div>

  {#if cancellable}
    <div class="actions">
      <Button
        label="ui.pin.cancel"
        variant="secondary"
        onclick={cancel}
        disabled={busy}
        disabledReason="ui.pin.checking"
      />
    </div>
  {/if}
</div>

<style>
  .inline-pin {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    max-width: 20rem;
    padding: var(--space-4);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background-color: var(--bg);
    color: var(--text);
  }

  .prompt,
  .feedback {
    margin: 0;
    font-size: var(--type-hub-label-size);
    line-height: var(--type-hub-label-line-height);
  }

  .prompt {
    font-weight: var(--type-hub-title-weight);
  }

  .cells-wrap {
    position: relative;
    align-self: flex-start;
  }

  .cells {
    display: flex;
    gap: var(--space-2);
  }

  .cell {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    box-sizing: border-box;
    width: var(--space-touch-min);
    height: var(--space-touch-min);
    border: var(--inline-pin-border);
    border-radius: var(--radius-sm);
    background-color: var(--inline-pin-background);
    color: var(--inline-pin-foreground);
    font-size: var(--type-hub-title-size);
  }

  .cell.filled {
    border-width: 2px;
  }

  /* The digits live in this input, laid over the cells: it takes the taps and the focus ring
     but shows nothing of its own. */
  .entry {
    position: absolute;
    inset: 0;
    box-sizing: border-box;
    width: 100%;
    height: 100%;
    margin: 0;
    padding: 0;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: transparent;
    caret-color: transparent;
    font-size: 1px;
    letter-spacing: 0;
    cursor: text;
  }

  .entry::selection {
    background: transparent;
    color: transparent;
  }

  .feedback {
    display: flex;
    gap: var(--space-2);
    min-height: 1lh;
    color: var(--text-field-error-text);
    font-weight: var(--type-hub-title-weight);
  }

  .keypad {
    display: grid;
    grid-template-columns: repeat(3, minmax(var(--space-touch-min), 1fr));
    gap: var(--space-2);
  }

  .key {
    --ui-bg: var(--button-secondary-background);
    min-width: var(--space-touch-min);
    min-height: var(--space-touch-min);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    color: var(--button-secondary-foreground);
    font: inherit;
    font-size: var(--type-hub-title-size);
    font-variant-numeric: tabular-nums;
    cursor: pointer;
  }

  .key--submit {
    --ui-bg: var(--button-primary-background);
    color: var(--button-primary-foreground);
    border-color: transparent;
  }

  .key[aria-disabled="true"] {
    cursor: not-allowed;
  }

  .actions {
    display: flex;
  }
</style>
