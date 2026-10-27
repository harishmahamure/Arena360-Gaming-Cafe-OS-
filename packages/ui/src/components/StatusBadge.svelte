<script lang="ts">
  import { t, type MessageKey } from "../messages/index.js";
  import { STATUS_LABELS, type Status } from "../status.js";

  /** Hub-only: dark and HC themes. The state is always written out, never colour alone. */
  let { state, label }: { state: Status; label?: MessageKey } = $props();
</script>

<span class="badge type-hub-micro state-{state}" data-component="status-badge"
  >{#if state === "paused"}<span aria-hidden="true">⏸</span>&nbsp;{/if}{t(
    label ?? STATUS_LABELS[state],
  )}</span
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
