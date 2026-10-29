<script lang="ts">
  import { CodeField, TextField, t } from "@arena/ui";
  import type { KitchenBoard as BoardFeed } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { onDestroy, onMount } from "svelte";
  import {
    changes,
    hubNow,
    placeTickets,
    rememberColumns,
    stageLabel,
    ticketLabel,
    visibleTickets,
    type Column,
  } from "../lib/board";
  import {
    DEFAULT_NAME,
    clearToken,
    connect,
    fetchStatus,
    pairDisplay,
    readToken,
    storeToken,
  } from "../lib/display";
  import BoardScreen from "../lib/BoardScreen.svelte";

  // The kitchen tablet (Story 7.6): it pairs by the 6-digit code from the Hub, keeps the `kds`
  // token in `localStorage` and connects to the Hub with `hello{class: kds}`. A missing or
  // refused token shows the pairing screen again; on a Free branch the page says the kitchen
  // display is available on Basic. Once connected it shows the kitchen board (Story 7.7): the
  // Hub's tickets in four columns, with every elapsed time counted on the Hub's clock.

  type Mode = "checking" | "locked" | "pairing" | "connecting" | "connected" | "lost";

  const PAIR_ERRORS: Partial<Record<string, MessageKey>> = {
    wrong: "kitchen.pair.wrong",
    limit: "kitchen.pair.limit",
    limited: "kitchen.pair.limited",
    unreachable: "kitchen.pair.unreachable",
    failed: "kitchen.pair.failed",
  };

  let mode: Mode = $state("checking");
  let code = $state("");
  let name = $state(DEFAULT_NAME);
  let busy = $state(false);
  let error: MessageKey | null = $state(null);
  let field: CodeField | undefined = $state();
  let stop: (() => void) | null = null;

  let feed: BoardFeed | null = $state(null);
  let receivedAt = $state(0);
  let tick = $state(0);
  let lastColumn: Map<string, Column> = $state(new Map());
  let announcement = $state("");
  let ticker: ReturnType<typeof setInterval> | undefined;

  /** The Hub's clock now; `tick` makes it follow the page's own second hand. */
  const now = $derived.by(() => (feed ? hubNow(feed, receivedAt, tick) : 0));
  const columns = $derived.by(() =>
    placeTickets(feed ? visibleTickets(feed.tickets, now) : [], lastColumn),
  );
  const boardShowing = $derived.by(
    () => feed !== null && (mode === "connected" || mode === "lost"),
  );

  function setTheme(hc: boolean) {
    document.documentElement.dataset.theme = hc ? "hc" : "dark";
  }

  function takeBoard(board: BoardFeed) {
    const said = changes(feed?.tickets ?? null, board.tickets).map((c) => {
      const number = ticketLabel(c.ticket.number);
      if (c.kind === "new") return t("kitchen.announce.new", { number, seat: c.ticket.seat });
      if (c.kind === "cancelled") return t("kitchen.announce.cancelled", { number });
      const stage = t(stageLabel(c.ticket.stage));
      return t("kitchen.announce.stage", { number, stage });
    });
    if (said.length > 0) announcement = said.join(". ");
    lastColumn = rememberColumns(board.tickets, lastColumn);
    receivedAt = performance.now();
    tick = receivedAt;
    feed = board;
  }

  function start(token: string) {
    stop?.();
    mode = "connecting";
    stop = connect(token, {
      onReady: () => (mode = "connected"),
      onLost: () => (mode = "lost"),
      onHighContrast: setTheme,
      onBoard: takeBoard,
      onRevoked: () => {
        stop = null;
        feed = null;
        announcement = "";
        clearToken();
        setTheme(false);
        showPairing();
      },
    });
  }

  function showPairing() {
    code = "";
    error = null;
    mode = "pairing";
  }

  async function submit(entered: string) {
    if (busy) return;
    busy = true;
    error = null;
    const outcome = await pairDisplay(entered, name);
    busy = false;
    if (outcome.kind === "paired") {
      storeToken(outcome.token);
      start(outcome.token);
      return;
    }
    if (outcome.kind === "locked") {
      mode = "locked";
      return;
    }
    error = PAIR_ERRORS[outcome.kind] ?? "kitchen.pair.failed";
    code = "";
    field?.focus();
  }

  onMount(async () => {
    ticker = setInterval(() => (tick = performance.now()), 1000);
    const token = readToken();
    if (token) return start(token);
    const status = await fetchStatus();
    if (status.kind === "ok" && !status.status.available) mode = "locked";
    else showPairing();
  });

  onDestroy(() => {
    clearInterval(ticker);
    stop?.();
  });
</script>

<svelte:head>
  <title>{t("app.title", { surface: t("surface.kitchen") })}</title>
</svelte:head>

<main class="screen" class:board={boardShowing} data-testid="kitchen-screen" data-mode={mode}>
  {#if boardShowing && feed}
    <BoardScreen branch={feed.branch_name} {columns} {now} connected={mode === "connected"} />
    <p class="visually-hidden" role="status" aria-live="polite" data-testid="board-announce">
      {announcement}
    </p>
  {:else}
    <h1 class="title">{t("kitchen.title")}</h1>
    {#if mode === "checking"}
      <p class="note" role="status">{t("kitchen.checking")}</p>
    {:else if mode === "locked"}
      <p class="note" role="status" data-testid="kitchen-locked">{t("kitchen.locked")}</p>
    {:else if mode === "pairing"}
      <form
        class="pair"
        data-testid="kitchen-pair"
        novalidate
        onsubmit={(e) => {
          e.preventDefault();
          if (code.length === 6) void submit(code);
        }}
      >
        <p class="note">{t("kitchen.pair.hint")}</p>
        <CodeField
          bind:this={field}
          bind:value={code}
          label="kitchen.pair.code"
          {error}
          {busy}
          oncomplete={(entered) => void submit(entered)}
        />
        <TextField label="kitchen.pair.name" bind:value={name} autocomplete="off" />
      </form>
    {:else if mode === "connecting"}
      <p class="note" role="status" data-testid="kitchen-connecting">{t("kitchen.connecting")}</p>
    {:else if mode === "lost"}
      <p class="note" role="status" data-testid="kitchen-lost">{t("kitchen.reconnecting")}</p>
    {:else}
      <p class="note" role="status" data-testid="kitchen-connected">{t("kitchen.connected")}</p>
    {/if}
  {/if}
</main>

<style>
  .screen {
    display: grid;
    align-content: start;
    justify-items: start;
    gap: var(--space-4);
    min-height: 100vh;
    box-sizing: border-box;
    padding: var(--space-5);
  }

  .screen.board {
    display: block;
    min-height: 0;
    padding: 0;
  }

  .title,
  .note {
    margin: 0;
  }

  .title {
    font-size: var(--type-hub-title-size);
    font-weight: var(--type-hub-title-weight);
  }

  .note {
    font-size: var(--type-hub-body-size);
  }

  .pair {
    display: grid;
    gap: var(--space-4);
    max-width: 28rem;
  }
</style>
