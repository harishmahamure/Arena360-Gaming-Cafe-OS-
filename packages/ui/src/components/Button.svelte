<script lang="ts">
  import type { HTMLButtonAttributes } from "svelte/elements";
  import { assertDisabledReason } from "../forms.js";
  import { t, type MessageKey } from "../messages/index.js";
  import type { ForceState } from "./types.js";

  type Common = {
    label: MessageKey;
    variant?: "primary" | "secondary" | "warning";
    type?: "button" | "submit" | "reset";
    onclick?: HTMLButtonAttributes["onclick"];
    forceState?: ForceState;
    /** Values for the `{placeholders}` of `disabledReason`. */
    reasonParams?: Record<string, string | number>;
  };
  /** A disabled button always says why, as inline text linked by `aria-describedby`. */
  type Props = Common &
    (
      | { disabled?: false; disabledReason?: undefined }
      | { disabled: boolean; disabledReason: MessageKey }
    );

  let {
    label,
    variant = "primary",
    type = "button",
    onclick,
    forceState,
    reasonParams,
    disabled = false,
    disabledReason,
  }: Props = $props();

  const id = $props.id();
  const reasonId = id + "-reason";

  // Runs on the server too; the effect re-checks when props change in the browser.
  // svelte-ignore state_referenced_locally
  assertDisabledReason(disabled, disabledReason, label);
  $effect.pre(() => assertDisabledReason(disabled, disabledReason, label));

  // `aria-disabled` keeps the button focusable so the reason is reachable; activation is
  // blocked here (Enter, Space and implicit form submission all arrive as a click).
  function handleClick(event: MouseEvent & { currentTarget: EventTarget & HTMLButtonElement }) {
    if (disabled) {
      event.preventDefault();
      event.stopImmediatePropagation();
      return;
    }
    onclick?.(event);
  }
</script>

<span class="button-wrap" data-component="button">
  <button
    class="ui-interactive button button--{variant}"
    {type}
    onclick={handleClick}
    aria-disabled={disabled ? "true" : undefined}
    aria-describedby={disabled && disabledReason ? reasonId : undefined}
    data-force-state={forceState}>{t(label)}</button
  >
  {#if disabled && disabledReason}
    <span class="reason" id={reasonId}>{t(disabledReason, reasonParams)}</span>
  {/if}
</span>

<style>
  .button-wrap {
    display: inline-flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-2);
    max-width: 100%;
  }

  .button {
    min-height: var(--space-touch-min);
    min-width: var(--space-touch-min);
    max-width: 100%;
    padding: var(--space-2) var(--space-5);
    border: 1px solid transparent;
    border-radius: var(--radius-full);
    font-family: var(--type-kiosk-label-family), system-ui, sans-serif;
    font-size: var(--type-hub-title-size);
    font-weight: var(--type-kiosk-cta-weight);
    line-height: var(--type-kiosk-cta-line-height);
    text-align: center;
    overflow-wrap: anywhere;
    cursor: pointer;
  }

  .button--primary {
    --ui-bg: var(--button-primary-background);
    color: var(--button-primary-foreground);
  }

  .button--secondary {
    --ui-bg: var(--button-secondary-background);
    color: var(--button-secondary-foreground);
    border-color: var(--border-strong);
  }

  .button--warning {
    --ui-bg: var(--button-warning-background);
    color: var(--button-warning-foreground);
  }

  :global([data-theme="light"]) .button--warning {
    border: var(--button-warning-border-light);
  }

  .button[aria-disabled="true"] {
    cursor: not-allowed;
  }

  .reason {
    color: var(--text);
    font-size: var(--type-hub-label-size);
    font-weight: var(--type-hub-label-weight);
    line-height: var(--type-hub-label-line-height);
  }
</style>
