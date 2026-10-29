<script lang="ts">
  import { StatusBadge, t } from "@arena/ui";
  import type { KitchenBoardTicket } from "@arena/ui/generated";
  import KitchenTicket from "./KitchenTicket.svelte";
  import { COLUMN_LABELS, COLUMNS, headingId, openSummary, type Column } from "./board";

  // The kitchen board (Story 7.7): the `kitchen-top-bar` over four columns, New, Preparing,
  // Ready and Served, each scrolling on its own. `columns` already holds the tickets on the board,
  // newest first; `now` is the Hub's clock.
  let {
    branch,
    columns,
    now,
    connected,
  }: {
    branch: string;
    columns: Record<Column, KitchenBoardTicket[]>;
    now: number;
    connected: boolean;
  } = $props();

  const summary = $derived(openSummary(Object.values(columns).flat(), now));
  const clock = $derived(
    new Date(now).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }),
  );
</script>

<div class="board" data-testid="kitchen-board">
  <header class="top" data-component="kitchen-top-bar" data-testid="kitchen-top-bar">
    <h1 class="branch type-hub-title" data-testid="board-branch">
      {branch || t("kitchen.title")}
    </h1>
    <time class="clock type-hub-label tabular-nums" datetime={new Date(now).toISOString()}
      ><span class="visually-hidden">{t("kitchen.board.clock")} </span>{clock}</time
    >
    <p class="open type-hub-label tabular-nums" data-testid="board-open">
      {t("kitchen.board.open", { open: summary.open, minutes: summary.oldestMinutes })}
    </p>
    {#if connected}
      <StatusBadge state="free" label="kitchen.board.connected" />
    {:else}
      <StatusBadge state="offline" label="kitchen.board.offline" />
    {/if}
  </header>
  <div class="columns">
    {#each COLUMNS as column (column)}
      <section class="column" aria-labelledby={headingId(column)} data-column={column}>
        <h2 class="col-head type-hub-title" id={headingId(column)}>
          <span>{t(COLUMN_LABELS[column])}</span>
          <span class="count type-hub-numeric tabular-nums" data-count={column}
            ><span class="visually-hidden"
              >{t("kitchen.col.count", { count: columns[column].length })}</span
            ><span aria-hidden="true">{columns[column].length}</span></span
          >
        </h2>
        <div class="stack">
          {#each columns[column] as ticket (ticket.id)}
            <KitchenTicket {ticket} {column} {now} />
          {/each}
        </div>
      </section>
    {/each}
  </div>
</div>

<style>
  .board {
    display: grid;
    grid-template-rows: var(--space-kitchen-header-height) minmax(0, 1fr);
    height: 100vh;
    background-color: var(--hub-bg);
    color: var(--text);
  }

  .top {
    display: flex;
    align-items: center;
    gap: var(--space-5);
    box-sizing: border-box;
    min-height: var(--space-kitchen-header-height);
    padding: 0 var(--space-kitchen-gutter);
    border-bottom: 1px solid var(--hub-line);
    background-color: var(--kitchen-top-bar-background);
  }

  .top > * {
    margin: 0;
  }

  .branch {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .clock,
  .open {
    color: var(--text-muted);
  }

  .columns {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: var(--space-kitchen-column-gap);
    box-sizing: border-box;
    min-height: 0;
    padding: var(--space-kitchen-gutter);
  }

  .column {
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    gap: var(--space-kitchen-ticket-gap);
    min-height: 0;
  }

  .col-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    margin: 0;
  }

  .stack {
    display: grid;
    align-content: start;
    gap: var(--space-kitchen-ticket-gap);
    min-height: 0;
    overflow-y: auto;
  }

  :global([data-theme="hc"]) .top {
    border-bottom: 2px solid var(--text);
  }
</style>
