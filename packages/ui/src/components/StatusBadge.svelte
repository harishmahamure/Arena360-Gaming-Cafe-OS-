<script lang="ts">
  import { t, type MessageKey } from "../messages/index.js";

  /** Hub-only: dark and HC themes. The state is always written out, never colour alone. */
  type State =
    | "free"
    | "in-session"
    | "paused"
    | "reserved"
    | "order-pending"
    | "maintenance"
    | "offline"
    | "unassigned";

  const LABELS: Record<State, MessageKey> = {
    free: "ui.status.free",
    "in-session": "ui.status.in_session",
    paused: "ui.status.paused",
    reserved: "ui.status.reserved",
    "order-pending": "ui.status.order_pending",
    maintenance: "ui.status.maintenance",
    offline: "ui.status.offline",
    unassigned: "ui.status.unassigned",
  };

  let { state, label }: { state: State; label?: MessageKey } = $props();
</script>

<span class="badge type-hub-micro state-{state}" data-component="status-badge"
  >{t(label ?? LABELS[state])}</span
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
  .state-unassigned {
    color: var(--state-unassigned);
    border-style: dashed;
  }
</style>
