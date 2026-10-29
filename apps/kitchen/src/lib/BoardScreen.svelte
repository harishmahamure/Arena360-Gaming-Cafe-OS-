<script lang="ts">
  import { StatusBadge, t } from "@arena/ui";
  import type { KitchenBoardTicket } from "@arena/ui/generated";
  import KitchenTicket from "./KitchenTicket.svelte";
  import { COLUMN_LABELS, COLUMNS, headingId, openSummary, type Column } from "./board";

  // The kitchen board (Story 7.7): the `kitchen-top-bar` over four columns, New, Preparing,
  // Ready and Served, each scrolling on its own. `columns` already holds the tickets on the board,
  // newest first; `now` is the Hub's clock. A tap on a ticket calls `onadvance` (Story 7.8).
  // `fresh` holds the tickets with the NEW badge; the top bar carries the chime toggle and, until
  // the browser lets the page make sound, the "Tap to turn on the chime" bar (Story 7.9).
  let {
    branch,
    columns,
    now,
    connected,
    fresh = new Set<string>(),
    muted = false,
    needsTap = false,
    onadvance,
    onmute,
    onunlock,
  }: {
    branch: string;
    columns: Record<Column, KitchenBoardTicket[]>;
    now: number;
    connected: boolean;
    fresh?: ReadonlySet<string>;
    muted?: boolean;
    needsTap?: boolean;
    onadvance?: (ticket: KitchenBoardTicket) => void;
    onmute?: () => void;
    onunlock?: () => void;
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
    <button
      type="button"
      class="mute type-hub-label"
      class:off={muted}
      data-testid="chime-toggle"
      onclick={() => onmute?.()}
    >
      <svg
        class="bell"
        viewBox="0 0 24 24"
        width="20"
        height="20"
        aria-hidden="true"
        focusable="false"
      >
        <path
          d="M12 3a6 6 0 0 0-6 6v4l-2 3h16l-2-3V9a6 6 0 0 0-6-6zm-2 15a2 2 0 0 0 4 0"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linejoin="round"
        />
        {#if muted}<path d="M4 4l16 16" stroke="currentColor" stroke-width="2" />{/if}
      </svg>
      <span>{t(muted ? "kitchen.chime.off" : "kitchen.chime.on")}</span>
    </button>
    {#if connected}
      <StatusBadge state="free" label="kitchen.board.connected" />
    {:else}
      <StatusBadge state="warning" label="kitchen.board.reconnecting" />
    {/if}
  </header>
  {#if needsTap}
    <button
      type="button"
      class="unlock type-hub-label"
      data-testid="chime-unlock"
      onclick={() => onunlock?.()}>{t("kitchen.chime.unlock")}</button
    >
  {/if}
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
            <KitchenTicket {ticket} {column} {now} fresh={fresh.has(ticket.id)} {onadvance} />
          {/each}
        </div>
      </section>
    {/each}
  </div>
</div>

<style>
  .board {
    display: grid;
    grid-template-rows: var(--space-kitchen-header-height) auto minmax(0, 1fr);
    height: 100vh;
    background-color: var(--hub-bg);
    color: var(--text);
  }

  .top {
    grid-row: 1;
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

  .mute,
  .unlock {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    box-sizing: border-box;
    min-height: var(--space-touch-min);
    padding: 0 var(--space-4);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    background-color: transparent;
    color: var(--text);
    font: inherit;
    cursor: pointer;
  }

  .mute.off {
    color: var(--text-muted);
  }

  .unlock {
    grid-row: 2;
    justify-self: stretch;
    margin: var(--space-2) var(--space-kitchen-gutter) 0;
    border-color: var(--accent);
  }

  .columns {
    grid-row: 3;
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
