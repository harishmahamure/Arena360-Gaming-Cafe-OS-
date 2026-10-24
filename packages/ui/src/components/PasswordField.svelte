<script lang="ts">
  import { t, type MessageKey } from "../messages/index.js";

  /**
   * A password input with a Show/Hide toggle. The toggle is a real button with `aria-pressed`
   * (pressed: the password is shown) and an accessible name that says what it does. Paste is
   * allowed. `hint` (the rule) is always linked by `aria-describedby`; `error` marks the field
   * invalid and is announced.
   */
  let {
    label,
    value = $bindable(""),
    name,
    autocomplete = "new-password",
    required = false,
    hint,
    error = null,
    errorParams,
  }: {
    label: MessageKey;
    value?: string;
    name?: string;
    autocomplete?: "new-password" | "current-password";
    required?: boolean;
    /** A catalogue message shown under the field, such as the password rule. */
    hint?: MessageKey;
    /** A catalogue message; when set the field is marked invalid and the message is announced. */
    error?: MessageKey | null;
    errorParams?: Record<string, string | number>;
  } = $props();

  const id = $props.id();
  const inputId = id + "-input";
  const hintId = id + "-hint";
  const errorId = id + "-error";

  let shown = $state(false);
  let input: HTMLInputElement | undefined = $state();

  const describedBy = $derived(
    [hint ? hintId : null, error ? errorId : null].filter(Boolean).join(" ") || undefined,
  );

  /** Focus the input. */
  export function focus() {
    input?.focus();
  }
</script>

<div class="field" data-component="password-field">
  <label class="label" for={inputId}>{t(label)}</label>
  <div class="row">
    <input
      bind:this={input}
      class="ui-interactive input"
      class:invalid={!!error}
      id={inputId}
      type={shown ? "text" : "password"}
      {name}
      {required}
      {autocomplete}
      autocapitalize="off"
      spellcheck="false"
      bind:value
      aria-invalid={error ? "true" : undefined}
      aria-describedby={describedBy}
    />
    <button
      type="button"
      class="ui-interactive toggle"
      aria-controls={inputId}
      aria-pressed={shown ? "true" : "false"}
      onclick={() => (shown = !shown)}>{t(shown ? "ui.password.hide" : "ui.password.show")}</button
    >
  </div>
  {#if hint}
    <p class="hint" id={hintId}>{t(hint)}</p>
  {/if}
  <p class="error" id={errorId} aria-live="polite">
    {#if error}<span class="glyph" aria-hidden="true">⚠</span>
      <span>{t(error, errorParams)}</span>{/if}
  </p>
</div>

<style>
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-width: 0;
    max-width: 100%;
  }

  .label {
    color: var(--text);
    font-size: var(--type-hub-label-size);
    font-weight: var(--type-hub-title-weight);
    line-height: var(--type-hub-label-line-height);
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .input {
    --ui-bg: var(--text-field-background);
    flex: 1 1 12em;
    min-width: 0;
    min-height: var(--space-touch-min);
    box-sizing: border-box;
    padding: 0 var(--space-3);
    border: var(--text-field-border);
    border-color: var(--border-strong);
    border-radius: var(--radius-sm);
    color: var(--text-field-foreground);
    font: inherit;
    font-size: var(--type-web-body-size);
  }

  .input:focus-visible {
    border-width: 2px;
    border-color: var(--accent);
    padding: 0 calc(var(--space-3) - 1px);
  }

  .input.invalid {
    border-width: 2px;
    border-color: var(--danger);
    padding: 0 calc(var(--space-3) - 1px);
  }

  .toggle {
    --ui-bg: var(--button-secondary-background);
    min-height: var(--space-touch-min);
    min-width: var(--space-touch-min);
    padding: 0 var(--space-4);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    color: var(--button-secondary-foreground);
    font: inherit;
    font-size: var(--type-hub-label-size);
    font-weight: var(--type-hub-title-weight);
    cursor: pointer;
  }

  .hint {
    margin: 0;
    color: var(--text-muted);
    font-size: var(--type-hub-label-size);
    line-height: var(--type-hub-label-line-height);
  }

  .error {
    display: flex;
    gap: var(--space-2);
    margin: 0;
    color: var(--text-field-error-text);
    font-size: var(--type-hub-label-size);
    font-weight: var(--type-hub-title-weight);
    line-height: var(--type-hub-label-line-height);
  }
</style>
