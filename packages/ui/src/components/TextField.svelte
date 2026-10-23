<script lang="ts">
  import { t, type MessageKey } from "../messages/index.js";
  import type { ForceState } from "./types.js";

  let {
    label,
    value = $bindable(""),
    type = "text",
    name,
    required = false,
    autocomplete,
    inputmode,
    error = null,
    forceState,
  }: {
    label: MessageKey;
    value?: string;
    type?: "text" | "password" | "email" | "tel" | "search" | "url";
    name?: string;
    required?: boolean;
    autocomplete?: import("svelte/elements").FullAutoFill;
    inputmode?: "text" | "numeric" | "tel" | "email" | "search" | "decimal" | "url";
    /** A catalogue message; when set the field is marked invalid and the message is announced. */
    error?: MessageKey | null;
    forceState?: ForceState;
  } = $props();

  const id = $props.id();
  const inputId = id + "-input";
  const errorId = id + "-error";

  let revealed = $state(false);
  const inputType = $derived(type === "password" && revealed ? "text" : type);
</script>

<div class="field" data-component="text-field">
  <label class="label" for={inputId}>{t(label)}</label>
  <div class="row">
    <input
      class="ui-interactive input"
      class:invalid={!!error}
      id={inputId}
      type={inputType}
      {name}
      {required}
      {autocomplete}
      {inputmode}
      bind:value
      aria-invalid={error ? "true" : undefined}
      aria-describedby={error ? errorId : undefined}
      data-force-state={forceState}
    />
    {#if type === "password"}
      <button
        type="button"
        class="ui-interactive toggle"
        aria-controls={inputId}
        onclick={() => (revealed = !revealed)}
        >{t(revealed ? "ui.password.hide" : "ui.password.show")}</button
      >
    {/if}
  </div>
  <p class="error" id={errorId} aria-live="polite">
    {#if error}<span class="glyph" aria-hidden="true">⚠</span>
      <span>{t(error)}</span>{/if}
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

  .input::placeholder {
    color: var(--text-field-placeholder);
  }

  /* Focus: a 2px accent border (padding shrinks by 1px so nothing shifts) plus the ring. */
  .input:focus-visible,
  .input[data-force-state~="focus"] {
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
