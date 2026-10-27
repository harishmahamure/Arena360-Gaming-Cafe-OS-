<script lang="ts">
  import type { Snippet } from "svelte";
  import { t, type MessageKey } from "../messages/index.js";

  type Props = {
    /** What just happened, with the amount in `params` (shown in tabular figures). */
    message: MessageKey;
    params?: Record<string, string | number>;
    /** Whole seconds left in the Undo window. */
    seconds: number;
    onundo: () => void;
    /** The Undo was sent and is being answered. */
    busy?: boolean;
    /** Replaces the message and the Undo with a plain note (done, or refused). */
    notice?: MessageKey;
    /** The note is a refusal: a glyph marks it, never colour alone. */
    refused?: boolean;
    /** The last Undo did not get through; the Undo stays so it can be tried again. */
    error?: MessageKey;
    /** Room for an inline approval below the message. */
    children?: Snippet;
  };

  let {
    message,
    params,
    seconds,
    onundo,
    busy = false,
    notice,
    refused = false,
    error,
    children,
  }: Props = $props();
</script>

<div class="undo-toast" data-component="undo-toast" data-testid="undo-toast">
  {#if notice}
    <p class="type-hub-body note" role="status" data-testid="undo-notice">
      {#if refused}<span aria-hidden="true">⚠</span>{/if}
      <span>{t(notice)}</span>
    </p>
  {:else}
    <p class="type-hub-body message tabular-nums" role="status" data-testid="undo-message">
      {t(message, params)}
    </p>
    <span class="seconds type-hub-label tabular-nums" data-testid="undo-seconds"
      >{t("ui.undo.seconds", { s: seconds })}</span
    >
    <button
      type="button"
      class="ui-interactive action type-hub-body"
      data-testid="undo-action"
      aria-disabled={busy ? "true" : undefined}
      onclick={() => {
        if (!busy) onundo();
      }}>{t("ui.undo.action")}</button
    >
  {/if}
  {#if error}
    <p class="type-hub-label note" role="alert" data-testid="undo-error">
      <span aria-hidden="true">⚠</span>
      <span>{t(error)}</span>
    </p>
  {/if}
  {#if children}<div class="extra">{@render children()}</div>{/if}
</div>

<style>
  .undo-toast {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3) var(--space-4);
    box-sizing: border-box;
    max-width: min(40rem, calc(100vw - 2 * var(--space-4)));
    padding: var(--space-4) var(--space-5);
    border: var(--undo-toast-border);
    border-radius: var(--radius-md);
    background-color: var(--undo-toast-background);
    color: var(--undo-toast-foreground);
    pointer-events: auto;
  }

  :global([data-theme="hc"]) .undo-toast {
    border: var(--undo-toast-border-hc);
    background-color: var(--undo-toast-background-hc);
    color: var(--undo-toast-foreground-hc);
  }

  p {
    margin: 0;
  }

  .message {
    flex: 1 1 auto;
    overflow-wrap: anywhere;
  }

  .note {
    display: flex;
    gap: var(--space-2);
  }

  .seconds {
    color: var(--text-muted);
  }

  .action {
    min-width: var(--space-touch-min);
    min-height: var(--space-touch-min);
    padding: 0 var(--space-4);
    border: 2px solid var(--undo-toast-action);
    border-radius: var(--radius-md);
    background: none;
    color: var(--undo-toast-action);
    font-weight: var(--type-hub-title-weight);
    cursor: pointer;
  }

  .action[aria-disabled="true"] {
    cursor: progress;
    opacity: 0.7;
  }

  :global([data-theme="hc"]) .action {
    border-color: var(--undo-toast-action-hc);
    color: var(--undo-toast-action-hc);
  }

  .extra,
  .note:not(:first-child) {
    flex: 1 1 100%;
  }
</style>
