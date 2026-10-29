<script lang="ts">
  import { StatusBadge, t } from "@arena/ui";
  import type { KitchenBoardTicket } from "@arena/ui/generated";
  import { elapsed, stageLabel, ticketLabel, type Column } from "./board";

  // One ticket on the board (Story 7.7): the seat, the ticket number, the time it has waited and
  // each line with its notes beneath. It names no player. Ready has a green edge and a READY tag;
  // Served is muted; Cancelled is muted with struck-through lines and a CANCELLED badge in place of
  // the stage tag, and cannot be acted on. Nothing here moves a ticket yet (Story 7.8).
  let { ticket, column, now }: { ticket: KitchenBoardTicket; column: Column; now: number } =
    $props();

  const cancelled = $derived(ticket.stage === "cancelled");
  const waited = $derived(elapsed(ticket.placed_at, now));
  const stageName = $derived(t(stageLabel(ticket.stage)));
</script>

<!-- A ticket is focusable so the board works by keyboard and screen reader; it takes its tap action in Story 7.8. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<article
  class="ticket"
  class:ready={column === "ready" && !cancelled}
  class:muted={column === "served" || cancelled}
  class:cancelled
  tabindex="0"
  data-component="kitchen-ticket"
  data-testid="kitchen-ticket"
  data-ticket={ticket.id}
  data-stage={ticket.stage}
  aria-label={t("kitchen.ticket.label", {
    number: ticketLabel(ticket.number),
    seat: ticket.seat,
    stage: stageName,
    elapsed: waited,
  })}
>
  <header class="head">
    <span class="seat type-hub-numeric tabular-nums" data-testid="ticket-seat">{ticket.seat}</span>
    <span class="elapsed type-hub-label tabular-nums" data-testid="ticket-elapsed">{waited}</span>
  </header>
  <div class="meta type-hub-label">
    <span class="number" data-testid="ticket-number">{ticketLabel(ticket.number)}</span>
    {#if cancelled}
      <StatusBadge state="offline" label="kitchen.ticket.cancelled" />
    {:else if column === "ready"}
      <span class="tag type-hub-micro" data-testid="ticket-ready">{t("kitchen.ticket.ready")}</span>
    {/if}
  </div>
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
</article>

<style>
  .ticket {
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

  :global([data-theme="hc"]) .ticket.ready {
    border-left-color: var(--kitchen-ticket-ready-accent);
  }
</style>
