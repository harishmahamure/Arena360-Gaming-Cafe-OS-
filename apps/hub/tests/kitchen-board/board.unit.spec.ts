import { expect, test } from "@playwright/test";
import type { KitchenBoard, KitchenBoardTicket } from "@arena/ui/generated";
import {
  CANCELLED_KEEP_MS,
  SERVED_KEEP_MS,
  UNDO_MS,
  applyPending,
  changes,
  elapsed,
  hubNow,
  nextStage,
  openSummary,
  placeTickets,
  rememberColumns,
  secondsLeft,
  stillPending,
  ticketLabel,
  undoRev,
  visibleTickets,
  type Column,
} from "../../../kitchen/src/lib/board";

// Unit tests for the kitchen board rules (no browser page is used).

const NOW = 1_800_000_000_000;
const MIN = 60_000;

function ticket(
  number: number,
  stage: string,
  over: Partial<KitchenBoardTicket> = {},
): KitchenBoardTicket {
  return {
    id: `T${number}`,
    number,
    seat: "PC 07",
    stage,
    placed_at: NOW - number * MIN,
    stage_at: NOW - number * MIN,
    rev: 1,
    lines: [],
    ...over,
  };
}

test("tickets are newest first", () => {
  const shown = visibleTickets([ticket(3, "new"), ticket(1, "new"), ticket(2, "new")], NOW);
  expect(shown.map((t) => t.number)).toEqual([1, 2, 3]);
  // `placed_at` is the order: the lowest number here was placed most recently.
  const same = visibleTickets(
    [ticket(1, "new", { placed_at: NOW }), ticket(2, "new", { placed_at: NOW })],
    NOW,
  );
  expect(same.map((t) => t.number)).toEqual([2, 1]);
});

test("open tickets always stay; served ones leave after 30 minutes and cancelled ones after 60 s", () => {
  const served = ticket(1, "served", { stage_at: NOW - SERVED_KEEP_MS });
  const goneServed = ticket(2, "served", { stage_at: NOW - SERVED_KEEP_MS - 1 });
  const cancelled = ticket(3, "cancelled", { stage_at: NOW - CANCELLED_KEEP_MS });
  const goneCancelled = ticket(4, "cancelled", { stage_at: NOW - CANCELLED_KEEP_MS - 1 });
  const old = ticket(5, "preparing", { placed_at: NOW - 5 * 60 * MIN });
  const shown = visibleTickets([served, goneServed, cancelled, goneCancelled, old], NOW);
  expect(shown.map((t) => t.number).sort()).toEqual([1, 3, 5]);
});

test("the open count is New, Preparing and Ready, and oldest is the oldest of those", () => {
  const tickets = [
    ticket(9, "new", { placed_at: NOW - 2 * MIN }),
    ticket(8, "preparing", { placed_at: NOW - 7 * MIN - 30_000 }),
    ticket(7, "ready", { placed_at: NOW - 4 * MIN }),
    ticket(6, "served", { placed_at: NOW - 40 * MIN, stage_at: NOW - MIN }),
    ticket(5, "cancelled", { placed_at: NOW - 50 * MIN, stage_at: NOW - 1000 }),
  ];
  expect(openSummary(tickets, NOW)).toEqual({ open: 3, oldestMinutes: 7 });
  expect(openSummary([], NOW)).toEqual({ open: 0, oldestMinutes: 0 });
  expect(openSummary([tickets[3], tickets[4]], NOW)).toEqual({ open: 0, oldestMinutes: 0 });
});

test("elapsed time is m:ss, then h:mm:ss, and never negative", () => {
  expect(elapsed(NOW - 5_000, NOW)).toBe("0:05");
  expect(elapsed(NOW - (12 * MIN + 9_000), NOW)).toBe("12:09");
  expect(elapsed(NOW - (61 * MIN + 1_000), NOW)).toBe("1:01:01");
  expect(elapsed(NOW + 9_000, NOW)).toBe("0:00");
});

test("the Hub's clock runs on from the board and ignores the tablet's date", () => {
  const board: KitchenBoard = { branch_name: "Arena", now_ms: NOW, tickets: [] };
  expect(hubNow(board, 1_000, 1_000)).toBe(NOW);
  expect(hubNow(board, 1_000, 4_500)).toBe(NOW + 3_500);
  expect(hubNow(board, 4_000, 1_000)).toBe(NOW);
});

test("tickets sit in the column of their stage and a cancelled one keeps its last column", () => {
  const seen = new Map<string, Column>([["T3", "preparing"]]);
  const placed = placeTickets(
    [
      ticket(1, "new"),
      ticket(2, "ready"),
      ticket(3, "cancelled"),
      ticket(4, "cancelled"),
      ticket(5, "served"),
    ],
    seen,
  );
  expect(placed.new.map((t) => t.number)).toEqual([1, 4]);
  expect(placed.preparing.map((t) => t.number)).toEqual([3]);
  expect(placed.ready.map((t) => t.number)).toEqual([2]);
  expect(placed.served.map((t) => t.number)).toEqual([5]);

  const before = rememberColumns([ticket(1, "ready"), ticket(2, "new")], new Map());
  const after = rememberColumns([ticket(1, "cancelled"), ticket(2, "preparing")], before);
  expect(after.get("T1")).toBe("ready");
  expect(after.get("T2")).toBe("preparing");
});

test("a new ticket, a stage change and a cancel are announced; the first board is not", () => {
  const first = [ticket(1, "new")];
  expect(changes(null, first)).toEqual([]);
  const next = [ticket(1, "preparing"), ticket(2, "new")];
  expect(changes(first, next).map((c) => [c.kind, c.ticket.number])).toEqual([
    ["stage", 1],
    ["new", 2],
  ]);
  const cancelled = [ticket(1, "cancelled"), ticket(2, "new")];
  expect(changes(next, cancelled).map((c) => [c.kind, c.ticket.number])).toEqual([
    ["cancelled", 1],
  ]);
  expect(changes(first, first)).toEqual([]);
});

test("the ticket number is printed as K-n", () => {
  expect(ticketLabel(12)).toBe("K-12");
});

test("only New, Preparing and Ready have a next stage", () => {
  expect(["new", "preparing", "ready", "served", "cancelled", "x"].map(nextStage)).toEqual([
    "preparing",
    "ready",
    "served",
    null,
    null,
    null,
  ]);
});

test("a pending move shows the ticket in its next stage until the Hub shows it off the old one", () => {
  const tickets = [ticket(1, "new"), ticket(2, "new")];
  const pending = new Map([["T1", { from: "new", to: "preparing" }]]);
  const shown = applyPending(tickets, pending, NOW);
  expect(shown.map((t) => t.stage)).toEqual(["preparing", "new"]);
  expect(shown[0].stage_at).toBe(NOW);
  expect(applyPending(tickets, new Map(), NOW)).toBe(tickets);

  // A board that still has T1 in New keeps the move; one that moved it (or lost it) drops it.
  expect([...stillPending(tickets, pending).keys()]).toEqual(["T1"]);
  expect(stillPending([ticket(1, "preparing")], pending).size).toBe(0);
  expect(stillPending([ticket(2, "new")], pending).size).toBe(0);
});

test("the ticket just moved is pinned to the top of its column", () => {
  const tickets = visibleTickets([ticket(1, "preparing"), ticket(2, "preparing")], NOW);
  expect(placeTickets(tickets, new Map()).preparing.map((t) => t.number)).toEqual([1, 2]);
  expect(placeTickets(tickets, new Map(), "T2").preparing.map((t) => t.number)).toEqual([2, 1]);
  expect(placeTickets(tickets, new Map(), "NOPE").preparing.map((t) => t.number)).toEqual([1, 2]);
});

test("an Undo is based on the rev the board shows at the new stage, else the tapped rev plus one", () => {
  expect(undoRev([ticket(1, "preparing", { rev: 4 })], "T1", "preparing", 3)).toBe(4);
  expect(undoRev([ticket(1, "new", { rev: 3 })], "T1", "preparing", 3)).toBe(4);
  expect(undoRev([], "T1", "preparing", 3)).toBe(4);
});

test("the Undo seconds count down from 8 and stop at 0", () => {
  expect(secondsLeft(NOW + UNDO_MS, NOW)).toBe(8);
  expect(secondsLeft(NOW + UNDO_MS, NOW + 1)).toBe(8);
  expect(secondsLeft(NOW + UNDO_MS, NOW + 7_001)).toBe(1);
  expect(secondsLeft(NOW + UNDO_MS, NOW + 9_000)).toBe(0);
});
