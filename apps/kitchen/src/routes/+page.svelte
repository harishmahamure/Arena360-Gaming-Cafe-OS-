<script lang="ts">
  import { CodeField, TextField, UndoToast, t } from "@arena/ui";
  import type { KitchenBoard as BoardFeed, KitchenBoardTicket } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { onDestroy, onMount, tick as settled } from "svelte";
  import {
    COLUMN_LABELS,
    UNDO_MS,
    applyPending,
    changes,
    freshTickets,
    hubNow,
    nextStage,
    placeTickets,
    rememberColumns,
    secondsLeft,
    stageLabel,
    stillPending,
    ticketLabel,
    undoRev,
    unpend,
    visibleTickets,
    type Column,
    type Pending,
  } from "../lib/board";
  import {
    DEFAULT_NAME,
    clearToken,
    connect,
    fetchStatus,
    pairDisplay,
    readToken,
    storeToken,
    type Connection,
  } from "../lib/display";
  import { BURST_MS, createChime, readMuted, storeMuted } from "../lib/chime";
  import {
    QUEUE_TTL_MS,
    liveEntries,
    openQueueStore,
    predictedRev,
    without,
    type Queued,
  } from "../lib/queue";
  import { ulid } from "../lib/ulid";
  import BoardScreen from "../lib/BoardScreen.svelte";

  // The kitchen tablet (Story 7.6): it pairs by the 6-digit code from the Hub, keeps the `kds`
  // token in `localStorage` and connects to the Hub with `hello{class: kds}`. A missing or
  // refused token shows the pairing screen again; on a Free branch the page says the kitchen
  // display is available on Basic. Once connected it shows the kitchen board (Story 7.7): the
  // Hub's tickets in four columns, with every elapsed time counted on the Hub's clock. A tap on
  // a ticket moves it to the next stage at once and shows an Undo toast for 8 s (Story 7.8). A new
  // ticket holds a NEW badge until it is advanced and plays the bundled chime once per burst,
  // unless this tablet is muted (Story 7.9). With the Hub out of reach a tap still moves the ticket
  // at once and is queued in IndexedDB; the queue replays in order when the Hub is back, and the
  // board says "Connected" again only once it has been redrawn from the Hub's state (Story 7.10).

  const NOTICE_MS = 5000;

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
  let conn: Connection | null = null;

  let feed: BoardFeed | null = $state(null);
  let receivedAt = $state(0);
  let tick = $state(0);
  let lastColumn: Map<string, Column> = $state(new Map());
  let announcement = $state("");
  let ticker: ReturnType<typeof setInterval> | undefined;

  // The NEW badges, the mute choice and whether the browser still wants a first tap before it
  // lets the page make sound.
  const chime = createChime();
  let fresh: Set<string> = $state(new Set());
  let muted = $state(false);
  let needsTap = $state(false);
  let lastChime = -Infinity;

  // The moves tapped while the Hub was out of reach, oldest first, and the replay of them. After a
  // reconnect the display is "reconciling" (taps queue behind the replay) until the first board
  // that follows the last replayed move.
  const store = openQueueStore();
  let queue: Queued[] = $state([]);
  let reconciling = $state(false);
  let replaying = false;
  let finalBoard = false;
  let sending: string | null = null;

  // Moves sent and not yet shown back by the Hub, the ticket pinned to the top of its column and
  // the one toast (the last advance, until its 8 s are up or another advance replaces it).
  type Toast = {
    key: string;
    ticketId: string;
    seat: string;
    number: string;
    from: string;
    to: string;
    tappedRev: number;
    expiresAt: number;
    undoing: boolean;
    notice?: MessageKey;
    error?: MessageKey;
  };
  let pending: Map<string, Pending> = $state(new Map());
  let pinned: string | null = $state(null);
  let toast: Toast | null = $state(null);
  let toastTimer: ReturnType<typeof setTimeout> | undefined;

  /** The Hub's clock now; `tick` makes it follow the page's own second hand. */
  const now = $derived.by(() => (feed ? hubNow(feed, receivedAt, tick) : 0));
  const columns = $derived.by(() =>
    placeTickets(
      feed ? visibleTickets(applyPending(feed.tickets, pending, now), now) : [],
      lastColumn,
      pinned,
    ),
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
    const badges = freshTickets(feed?.tickets ?? null, board.tickets, fresh);
    fresh = badges.ids;
    if (badges.arrived > 0) void ring();
    lastColumn = rememberColumns(board.tickets, lastColumn);
    pending = stillPending(board.tickets, pending);
    if (finalBoard) {
      finalBoard = false;
      reconciling = false;
    }
    receivedAt = performance.now();
    tick = receivedAt;
    feed = board;
  }

  /** Plays the chime for new tickets, once for a burst; a muted tablet stays quiet. */
  async function ring() {
    const at = performance.now();
    if (muted || at - lastChime < BURST_MS) return;
    lastChime = at;
    if (!(await chime.play())) needsTap = true;
  }

  function toggleMute() {
    muted = !muted;
    storeMuted(muted);
    // The tap that unmutes is a gesture, so the browser lets the page make sound from here on.
    if (!muted) void unlock();
  }

  /** The first tap anywhere: the browser now allows sound, so the bar goes. */
  async function unlock() {
    if (await chime.unlock()) needsTap = false;
  }

  function firstTap() {
    if (needsTap) void unlock();
  }

  function showToast(next: Toast, lastsMs: number) {
    clearTimeout(toastTimer);
    toast = next;
    toastTimer = setTimeout(() => {
      if (toast?.key === next.key) toast = null;
    }, lastsMs);
  }

  /** Puts a ticket back to the Hub's state and asks the Hub for its board. */
  function snap(ticketId: string) {
    const rest = new Map(pending);
    rest.delete(ticketId);
    pending = rest;
    if (pinned === ticketId) pinned = null;
    conn?.refresh();
  }

  async function focusTicket(ticketId: string) {
    await settled();
    document.querySelector<HTMLElement>(`[data-ticket="${ticketId}"]`)?.focus();
  }

  const MOVE_NOTICES: Record<string, MessageKey> = {
    STALE: "kitchen.toast.stale",
    BAD_STAGE: "kitchen.toast.stale",
    NOT_FOUND: "kitchen.toast.stale",
    TIER_LOCKED: "kitchen.toast.stale",
  };

  /** True while a tap cannot go straight to the Hub: it is out of reach or still catching up. */
  const behind = () => mode !== "connected" || reconciling;

  function enqueue(entry: Queued) {
    queue = [...queue, entry];
    void store.put(entry);
    if (mode === "connected") void replay();
  }

  function dequeue(key: string) {
    queue = without(queue, key);
    void store.remove(key);
  }

  function clearBadge(ticketId: string) {
    const rest = new Set(fresh);
    rest.delete(ticketId);
    fresh = rest;
  }

  /**
   * Sends the queued moves in the order they were tapped, each with the key it was minted with
   * (so the Hub applies it once). A move the Hub refuses (the counter got there first, the ticket
   * was cancelled, it expired) is dropped without a word; the board then shows the Hub's state.
   * A lost link stops the replay and the moves stay queued for the next reconnect.
   */
  async function replay() {
    const link = conn;
    if (replaying || !link) return;
    replaying = true;
    const touched = new Set<string>();
    try {
      for (let next = queue[0]; next; next = queue[0]) {
        if (next.expiresAt <= Date.now()) {
          dequeue(next.key);
          touched.add(next.ticketId);
          continue;
        }
        sending = next.key;
        const result = await link.advance(
          next.ticketId,
          next.to,
          next.baseRev,
          next.key,
          next.issuedAt,
        );
        sending = null;
        if (!result.ok && result.code === "OFFLINE") return;
        dequeue(next.key);
        touched.add(next.ticketId);
        if (result.ok) clearBadge(next.ticketId);
      }
      // Everything is sent: show the Hub's state for those tickets and ask for the board.
      const rest = new Map(pending);
      for (const id of touched) rest.delete(id);
      pending = rest;
      if (pinned && touched.has(pinned)) pinned = null;
      finalBoard = true;
      link.refresh();
    } finally {
      replaying = false;
      sending = null;
    }
  }

  async function advance(ticket: KitchenBoardTicket) {
    const to = nextStage(ticket.stage);
    if (!to || !conn) return;
    const waiting = behind();
    // A move in flight blocks a second tap; behind the Hub, taps chain on the ticket as shown.
    if (pending.has(ticket.id) && !waiting) return;
    const key = ulid();
    const issuedAt = Date.now();
    tick = performance.now();
    const earlier = pending.get(ticket.id);
    pending = new Map(pending).set(ticket.id, { from: earlier?.from ?? ticket.stage, to });
    pinned = ticket.id;
    showToast(
      {
        key,
        ticketId: ticket.id,
        seat: ticket.seat,
        number: ticketLabel(ticket.number),
        from: ticket.stage,
        to,
        tappedRev: ticket.rev,
        expiresAt: tick + UNDO_MS,
        undoing: false,
      },
      UNDO_MS,
    );
    void focusTicket(ticket.id);
    const queued: Queued = {
      key,
      ticketId: ticket.id,
      to,
      baseRev: predictedRev(queue, ticket.id, ticket.rev),
      issuedAt,
      expiresAt: issuedAt + QUEUE_TTL_MS,
    };
    // Past New for good once the move is held or sent: Undo back to New does not bring the badge back.
    if (waiting) {
      clearBadge(ticket.id);
      enqueue(queued);
      return;
    }
    const result = await conn.advance(ticket.id, to, ticket.rev, key, issuedAt);
    if (result.ok) {
      clearBadge(ticket.id);
      return;
    }
    if (result.code === "OFFLINE") {
      // The Hub went away under the tap: keep the move and send it when the link is back.
      clearBadge(ticket.id);
      enqueue(queued);
      return;
    }
    snap(ticket.id);
    void focusTicket(ticket.id);
    if (toast?.key === key) {
      showToast(
        { ...toast, notice: MOVE_NOTICES[result.code] ?? "kitchen.toast.failed" },
        NOTICE_MS,
      );
    }
  }

  async function undo() {
    const cur = toast;
    if (!cur || cur.notice || cur.undoing || !conn) return;
    if (queue.some((e) => e.key === cur.key) && sending !== cur.key) {
      // Not sent yet: Undo just takes it out of the queue.
      dequeue(cur.key);
      pending = unpend(pending, cur.ticketId, cur.from);
      pinned = cur.ticketId;
      toast = null;
      clearTimeout(toastTimer);
      void focusTicket(cur.ticketId);
      return;
    }
    const rev = undoRev(feed?.tickets ?? [], cur.ticketId, cur.to, cur.tappedRev);
    toast = { ...cur, undoing: true, error: undefined };
    pending = new Map(pending).set(cur.ticketId, { from: cur.to, to: cur.from });
    pinned = cur.ticketId;
    void focusTicket(cur.ticketId);
    const result = await conn.revert(cur.ticketId, rev);
    if (result.ok) {
      if (toast?.key === cur.key) toast = null;
      return;
    }
    snap(cur.ticketId);
    void focusTicket(cur.ticketId);
    if (toast?.key !== cur.key) return;
    if (result.code === "OFFLINE" || result.code === "INTERNAL") {
      toast = { ...cur, undoing: false, error: "kitchen.toast.undo_failed" };
    } else {
      showToast({ ...cur, notice: "kitchen.toast.undo_late", undoing: false }, NOTICE_MS);
    }
  }

  function start(token: string) {
    conn?.close();
    mode = "connecting";
    conn = connect(token, {
      onReady: () => {
        if (mode === "connecting") needsTap = !muted && chime.needsTap();
        mode = "connected";
        // The first board after the queue has replayed ends the catching up.
        reconciling = true;
        finalBoard = queue.length === 0;
        if (queue.length > 0) void replay();
      },
      onLost: () => {
        mode = "lost";
        reconciling = false;
        finalBoard = false;
      },
      onHighContrast: setTheme,
      onBoard: takeBoard,
      onRevoked: () => {
        conn = null;
        for (const entry of queue) void store.remove(entry.key);
        queue = [];
        reconciling = false;
        finalBoard = false;
        feed = null;
        pending = new Map();
        pinned = null;
        fresh = new Set();
        toast = null;
        clearTimeout(toastTimer);
        lastColumn = new Map();
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
    muted = readMuted();
    ticker = setInterval(() => (tick = performance.now()), 1000);
    const token = readToken();
    if (token) {
      // Moves tapped before a reload wait in IndexedDB; they replay once the Hub answers.
      const kept = await store.load();
      const live = liveEntries(kept, Date.now());
      for (const gone of kept.filter((e) => !live.includes(e))) void store.remove(gone.key);
      queue = live;
      return start(token);
    }
    const status = await fetchStatus();
    if (status.kind === "ok" && !status.status.available) mode = "locked";
    else showPairing();
  });

  onDestroy(() => {
    clearInterval(ticker);
    clearTimeout(toastTimer);
    conn?.close();
  });
</script>

<svelte:window onpointerdown={firstTap} onkeydown={firstTap} />

<svelte:head>
  <title>{t("app.title", { surface: t("surface.kitchen") })}</title>
</svelte:head>

<main class="screen" class:board={boardShowing} data-testid="kitchen-screen" data-mode={mode}>
  {#if boardShowing && feed}
    <BoardScreen
      branch={feed.branch_name}
      {columns}
      {now}
      connected={mode === "connected" && !reconciling}
      {fresh}
      {muted}
      {needsTap}
      onadvance={advance}
      onmute={toggleMute}
      onunlock={() => void unlock()}
    />
    {#if toast}
      <div class="toast" data-testid="kitchen-toast">
        <UndoToast
          message="kitchen.toast.moved"
          params={{
            seat: toast.seat,
            number: toast.number,
            stage: t(COLUMN_LABELS[toast.to as Column]),
          }}
          seconds={secondsLeft(toast.expiresAt, tick)}
          onundo={() => void undo()}
          busy={toast.undoing}
          notice={toast.notice}
          refused={toast.notice !== undefined}
          error={toast.error}
        />
      </div>
    {/if}
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

  .toast {
    position: fixed;
    right: 0;
    bottom: var(--space-5);
    left: 0;
    display: flex;
    justify-content: center;
    pointer-events: none;
  }

  .pair {
    display: grid;
    gap: var(--space-4);
    max-width: 28rem;
  }
</style>
