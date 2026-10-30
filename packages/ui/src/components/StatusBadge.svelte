<script lang="ts">
  import { t, type MessageKey } from "../messages/index.js";
  import { STATUS_LABELS, type Status } from "../status.js";

  /**
   * Hub-only: dark and HC themes. The state is always written out, never colour alone. The owner
   * app's command states take a `tone` and their own `text` instead of a Hub state.
   */
  let {
    state,
    tone,
    label,
    text,
  }: {
    state?: Status;
    tone?: "info" | "ok" | "danger" | "muted";
    label?: MessageKey;
    text?: string;
  } = $props();
  const TONES = {
    info: "tone-info",
    ok: "tone-ok",
    danger: "tone-danger",
    muted: "tone-muted",
  } as const;
  const cls = $derived(tone ? TONES[tone] : "state-" + state);
  const shown = $derived(text ?? (label ? t(label) : state ? t(STATUS_LABELS[state]) : ""));
</script>

<span class="badge type-hub-micro {cls}" data-component="status-badge"
  >{#if state === "paused"}<span aria-hidden="true">⏸</span>&nbsp;{/if}{shown}</span
>

<style>
  .badge {
    display: inline-flex;
    align-items: center;
    max-width: 100%;
    padding: var(--space-1) var(--space-2);
    border: 1px solid currentColor;
    border-radius: var(--radius-xs);
    background-color: var(--status-badge-background);
    text-transform: uppercase;
    overflow-wrap: anywhere;
  }

  .tone-info {
    color: var(--info);
  }
  .tone-ok {
    color: var(--ok);
  }
  .tone-danger {
    color: var(--danger);
  }
  .tone-muted {
    color: var(--text-muted);
  }

  .state-free {
    color: var(--state-free);
  }
  .state-in-session {
    color: var(--state-in-session);
  }
  .state-paused {
    color: var(--state-paused);
  }
  .state-reserved {
    color: var(--state-reserved);
  }
  .state-order-pending {
    color: var(--state-order-pending);
  }
  .state-maintenance {
    color: var(--state-maintenance);
  }
  .state-offline {
    color: var(--state-offline);
  }
  .state-warning {
    color: var(--state-order-pending);
  }
  .state-unassigned {
    color: var(--state-unassigned);
    border-style: dashed;
  }
</style>
