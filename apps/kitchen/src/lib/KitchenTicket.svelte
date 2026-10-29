<script lang="ts">
  import { AlertBadge, StatusBadge, t } from "@arena/ui";
  import type { KitchenBoardTicket } from "@arena/ui/generated";
  import { COLUMN_LABELS, elapsed, nextStage, stageLabel, ticketLabel, type Column } from "./board";

  // One ticket on the board (Story 7.7): the seat, the ticket number, the time it has waited and
  // each line with its notes beneath. It names no player. Ready has a green edge and a READY tag;
  // Served is muted; Cancelled is muted with struck-through lines and a CANCELLED badge in place of
  // the stage tag, and cannot be acted on. A New, Preparing or Ready ticket is one big button:
  // a tap, Enter or Space asks to move it to the next stage (Story 7.8). A `fresh` ticket in New
  // carries the NEW badge and a danger edge until it is advanced (Story 7.9).
  let {
    ticket,
    column,
    now,
    fresh = false,
    onadvance,
  }: {
    ticket: KitchenBoardTicket;
    column: Column;
    now: number;
    fresh?: boolean;
    onadvance?: (ticket: KitchenBoardTicket) => void;
  } = $props();

  const isNew = $derived(fresh && column === "new" && ticket.stage === "new");

  const cancelled = $derived(ticket.stage === "cancelled");
  const waited = $derived(elapsed(ticket.placed_at, now));
  const stageName = $derived(t(stageLabel(ticket.stage)));
  const next = $derived(cancelled ? null : nextStage(ticket.stage));
  const actionable = $derived(next !== null && onadvance !== undefined);
  const label = $derived(
    t("kitchen.ticket.label", {
      number: ticketLabel(ticket.number),
      seat: ticket.seat,
      stage: stageName,
      elapsed: waited,
    }),
  );
  const hint = $derived(
    next ? t("kitchen.ticket.advance_hint", { stage: t(COLUMN_LABELS[next]) }) : "",
  );

  const hintId = $derived(`hint-${ticket.id}`); // i18n-ignore (an element id)

  function press(event: KeyboardEvent) {
    if (!actionable || event.repeat || event.target !== event.currentTarget) return;
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      onadvance?.(ticket);
    }
  }
</script>

<!-- A ticket is focusable so the board works by keyboard and screen reader; New, Preparing and Ready tickets are buttons (named by their content, with the move as a description) that move on. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_to_interactive_role -->
<div
  class="ticket"
  class:ready={column === "ready" && !cancelled}
  class:fresh={isNew}
  class:muted={column === "served" || cancelled}
  class:cancelled
  class:actionable
  tabindex="0"
  role={actionable ? "button" : "article"}
  data-component="kitchen-ticket"
  data-testid="kitchen-ticket"
  data-ticket={ticket.id}
  data-stage={ticket.stage}
  data-new={isNew ? "" : undefined}
  aria-label={actionable ? undefined : label}
  aria-describedby={actionable ? hintId : undefined}
  onclick={() => actionable && onadvance?.(ticket)}
  onkeydown={press}
>
  <header class="head">
    <span class="seat type-hub-numeric tabular-nums" data-testid="ticket-seat">{ticket.seat}</span>
    <span class="elapsed type-hub-label tabular-nums" data-testid="ticket-elapsed">{waited}</span>
  </header>
  <div class="meta type-hub-label">
    <span class="number" data-testid="ticket-number">{ticketLabel(ticket.number)}</span>
    {#if isNew}
      <span data-testid="ticket-new"><AlertBadge tag="new" /></span>
    {:else if cancelled}
      <StatusBadge state="offline" label="kitchen.ticket.cancelled" />
    {:else if column === "ready"}
      <span class="tag type-hub-micro" data-testid="ticket-ready">{t("kitchen.ticket.ready")}</span>
    {/if}
  </div>
  {#if actionable}<span class="visually-hidden" id={hintId}>{hint}</span>{/if}
  <ul class="lines">
    {#each ticket.lines as line (line.line_id)}
      <li class="line">
        <span class="what type-hub-body" class:struck={cancelled}
          >{t("kitchen.ticket.line", { qty: line.qty, name: line.label })}</span
        >
        {#each line.notes as note (note)}
          <span class="note type-hub-label" class:struck={cancelled}>{note}</span>
        {/each}
      </li>
    {/each}
  </ul>
</div>

<style>
  .ticket {
    position: relative;
    display: grid;
    gap: var(--space-2);
    box-sizing: border-box;
    min-height: var(--space-touch-min);
    padding: var(--space-kitchen-ticket-padding);
    border: 1px solid var(--border-strong);
    border-left-width: 4px;
    border-radius: var(--radius-md);
    background-color: var(--kitchen-ticket-background);
    color: var(--text);
  }

  .ticket.ready {
    border-left-color: var(--kitchen-ticket-ready-accent);
  }

  .ticket.fresh {
    border-left-color: var(--danger);
  }

  .ticket.actionable {
    cursor: pointer;
    user-select: none;
  }

  .ticket.muted,
  .ticket.cancelled {
    color: var(--text-muted);
  }

  .head,
  .meta {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-2);
  }

  .seat {
    overflow-wrap: anywhere;
  }

  .meta {
    color: var(--text-muted);
  }

  .tag {
    padding: var(--space-1) var(--space-2);
    border: 1px solid var(--kitchen-ticket-ready-accent);
    border-radius: var(--radius-xs);
    color: var(--kitchen-ticket-ready-accent);
    text-transform: uppercase;
  }

  .lines {
    display: grid;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .line {
    display: grid;
    gap: var(--space-1);
  }

  .note {
    margin-left: var(--space-4);
    color: var(--text-muted);
  }

  .struck {
    text-decoration: line-through;
  }

  :global([data-theme="hc"]) .ticket {
    border: 2px solid var(--text);
    border-left-width: 6px;
  }

  :global([data-theme="hc"]) .ticket.fresh {
    border-left-color: var(--danger);
  }

  :global([data-theme="hc"]) .ticket.ready {
    border-left-color: var(--kitchen-ticket-ready-accent);
  }
</style>
