<script lang="ts">
  import { onMount } from "svelte";
  import { t, type MessageKey } from "../messages/index.js";

  /**
   * A one-time code: `length` visible digit cells over one numeric input
   * (`autocomplete="one-time-code"`, so the phone's SMS suggestion and a paste of the whole code
   * fill every cell at once). Only digits are kept. When every cell is filled `oncomplete` gets
   * the code. The caller clears the cells by setting the bindable `value` to `""` (after a
   * wrong code). `error` marks the field invalid and is announced; `busy` makes it read-only
   * while the code is checked.
   */
  let {
    value = $bindable(""),
    length = 6,
    label = "ui.code.label",
    error = null,
    errorParams,
    busy = false,
    autofocus = true,
    oncomplete,
  }: {
    value?: string;
    length?: number;
    /** The input's visible label. */
    label?: MessageKey;
    /** A catalogue message; when set the field is invalid and the message is announced. */
    error?: MessageKey | null;
    errorParams?: Record<string, string | number>;
    busy?: boolean;
    autofocus?: boolean;
    oncomplete?: (code: string) => void;
  } = $props();

  const id = $props.id();
  const inputId = id + "-input";
  const errorId = id + "-error";

  let input: HTMLInputElement | undefined = $state();
  /** The last code handed to `oncomplete`, so one full code is submitted once. */
  let submitted = "";

  const digits = $derived(value.replace(/\D/g, "").slice(0, length));

  function oninput() {
    if (!input) return;
    if (busy) {
      input.value = value;
      return;
    }
    const next = input.value.replace(/\D/g, "").slice(0, length);
    value = next;
    if (input.value !== next) input.value = next;
    if (next.length === length && next !== submitted) {
      submitted = next;
      oncomplete?.(next);
    }
    if (next.length < length) submitted = "";
  }

  /** Focus the input (the caller does this after clearing the cells). */
  export function focus() {
    input?.focus();
  }

  $effect(() => {
    // A cleared value (a wrong code) lets the same code be submitted again.
    if (value === "") submitted = "";
    if (input && input.value !== digits) input.value = digits;
  });

  onMount(() => {
    if (autofocus) input?.focus();
  });
</script>

<div class="code-field" data-component="code-field">
  <label class="label" for={inputId}>{t(label)}</label>
  <div class="cells-wrap">
    <div class="cells" aria-hidden="true">
      {#each { length }, i (i)}
        <span
          class="cell"
          class:filled={i < digits.length}
          class:current={i === Math.min(digits.length, length - 1)}
          class:invalid={!!error}>{digits[i] ?? ""}</span
        >
      {/each}
    </div>
    <input
      bind:this={input}
      class="entry"
      id={inputId}
      type="text"
      inputmode="numeric"
      autocomplete="one-time-code"
      pattern="[0-9]*"
      spellcheck="false"
      readonly={busy}
      aria-invalid={error ? "true" : undefined}
      aria-describedby={error ? errorId : undefined}
      {oninput}
    />
  </div>
  <p class="visually-hidden" aria-live="polite" aria-atomic="true">
    {t("ui.code.count", { count: digits.length, length })}
  </p>
  <p class="error" id={errorId} aria-live="assertive">
    {#if error}<span class="glyph" aria-hidden="true">⚠</span>
      <span>{t(error, errorParams)}</span>{/if}
  </p>
</div>

<style>
  .code-field {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    max-width: 100%;
  }

  .label {
    color: var(--text);
    font-size: var(--type-hub-label-size);
    font-weight: var(--type-hub-title-weight);
    line-height: var(--type-hub-label-line-height);
  }

  .cells-wrap {
    position: relative;
    align-self: flex-start;
    max-width: 100%;
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
    height: calc(var(--space-touch-min) + var(--space-2));
    border: var(--text-field-border);
    border-color: var(--border-strong);
    border-radius: var(--radius-sm);
    background-color: var(--text-field-background);
    color: var(--text-field-foreground);
    font-size: var(--type-hub-title-size);
    font-variant-numeric: tabular-nums;
  }

  .cell.filled {
    border-width: 2px;
  }

  .cell.invalid {
    border-width: 2px;
    border-color: var(--danger);
  }

  /* The digits live in this input, laid over the cells: it takes the taps, the paste and the
     focus ring but shows nothing of its own. */
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

  .error {
    display: flex;
    gap: var(--space-2);
    min-height: 1lh;
    margin: 0;
    color: var(--text-field-error-text);
    font-size: var(--type-hub-label-size);
    font-weight: var(--type-hub-title-weight);
    line-height: var(--type-hub-label-line-height);
  }
</style>
