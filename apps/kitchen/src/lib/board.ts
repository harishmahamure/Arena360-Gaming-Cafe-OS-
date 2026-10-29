// What the kitchen board shows (Story 7.7), kept apart from the page so it is plain functions:
// which tickets are still on the board, which column each sits in, the open count, the elapsed
// time and what a screen reader hears when the board changes. Every time here is the Hub's.

import type { KitchenBoard, KitchenBoardTicket } from "@arena/ui/generated";
import type { MessageKey } from "@arena/ui/messages";

/** The columns, left to right. */
export const COLUMNS = ["new", "preparing", "ready", "served"] as const;
export type Column = (typeof COLUMNS)[number];

/** The heading of each column. */
export const COLUMN_LABELS: Record<Column, MessageKey> = {
  new: "kitchen.col.new",
  preparing: "kitchen.col.preparing",
  ready: "kitchen.col.ready",
  served: "kitchen.col.served",
};

/** The element id of a column's heading. */
export const headingId = (column: Column): string => `col-${column}`; // i18n-ignore (an element id)

/** The column heading for a stage, or its Cancelled label. */
export function stageLabel(stage: string): MessageKey {
  const column = columnOf(stage);
  return column ? COLUMN_LABELS[column] : "kitchen.ticket.cancelled";
}

/** A Served ticket stays this long (milliseconds) after it was served. */
export const SERVED_KEEP_MS = 30 * 60_000;
/** A Cancelled ticket stays this long (milliseconds) after it was cancelled. */
export const CANCELLED_KEEP_MS = 60_000;

const MINUTE_MS = 60_000;

/** The stage name `stage` as a column, or `null` for a cancelled (or unknown) stage. */
export function columnOf(stage: string): Column | null {
  return (COLUMNS as readonly string[]).includes(stage) ? (stage as Column) : null;
}

/** Whether `ticket` is still open: New, Preparing or Ready. */
export function isOpen(ticket: KitchenBoardTicket): boolean {
  const column = columnOf(ticket.stage);
  return column !== null && column !== "served";
}

/** The Hub's clock now, from a board read at `board.now_ms` and received at `receivedAt`. */
export function hubNow(board: KitchenBoard, receivedAt: number, at: number): number {
  return board.now_ms + Math.max(0, at - receivedAt);
}

/** Whether `ticket` is still on the board at the Hub time `now`. */
export function onBoard(ticket: KitchenBoardTicket, now: number): boolean {
  if (isOpen(ticket)) return true;
  const age = now - ticket.stage_at;
  if (ticket.stage === "served") return age <= SERVED_KEEP_MS;
  if (ticket.stage === "cancelled") return age <= CANCELLED_KEEP_MS;
  return false;
}

/** The tickets on the board at `now`, newest first. */
export function visibleTickets(tickets: KitchenBoardTicket[], now: number): KitchenBoardTicket[] {
  return tickets
    .filter((t) => onBoard(t, now))
    .sort((a, b) => b.placed_at - a.placed_at || b.number - a.number);
}

/**
 * Where each ticket sits. A cancelled ticket stays in the column it was last seen in (the Hub
 * keeps only its reason), or New when this display never saw it before.
 */
export function placeTickets(
  tickets: KitchenBoardTicket[],
  lastColumn: ReadonlyMap<string, Column>,
): Record<Column, KitchenBoardTicket[]> {
  const columns: Record<Column, KitchenBoardTicket[]> = {
    new: [],
    preparing: [],
    ready: [],
    served: [],
  };
  for (const ticket of tickets) {
    const column = columnOf(ticket.stage) ?? lastColumn.get(ticket.id) ?? "new";
    columns[column].push(ticket);
  }
  return columns;
}

/** The column each live ticket is in now, kept so a cancelled ticket can stay put. */
export function rememberColumns(
  tickets: KitchenBoardTicket[],
  before: ReadonlyMap<string, Column>,
): Map<string, Column> {
  const next = new Map<string, Column>();
  for (const ticket of tickets) {
    const column = columnOf(ticket.stage) ?? before.get(ticket.id);
    if (column) next.set(ticket.id, column);
  }
  return next;
}

export type OpenSummary = { open: number; oldestMinutes: number };

/** The open count (New, Preparing, Ready) and the age in whole minutes of the oldest of those. */
export function openSummary(tickets: KitchenBoardTicket[], now: number): OpenSummary {
  const open = tickets.filter(isOpen);
  const oldest = open.reduce((max, t) => Math.max(max, now - t.placed_at), 0);
  return { open: open.length, oldestMinutes: Math.floor(oldest / MINUTE_MS) };
}

/** `m:ss` (or `h:mm:ss` from an hour) since `placedAt`, never negative. */
export function elapsed(placedAt: number, now: number): string {
  const total = Math.max(0, Math.floor((now - placedAt) / 1000));
  const s = String(total % 60).padStart(2, "0");
  const m = Math.floor(total / 60);
  if (m < 60) return `${m}:${s}`; // i18n-ignore (a clock)
  return `${Math.floor(m / 60)}:${String(m % 60).padStart(2, "0")}:${s}`; // i18n-ignore (a clock)
}

/** The ticket number as the Hub prints it: `K-12`. */
export function ticketLabel(number: number): string {
  return `K-${number}`; // i18n-ignore (the printed ticket number)
}

export type Change =
  | { kind: "new"; ticket: KitchenBoardTicket }
  | { kind: "stage"; ticket: KitchenBoardTicket }
  | { kind: "cancelled"; ticket: KitchenBoardTicket };

/**
 * What changed between two boards, for the polite live region: tickets that appeared and
 * tickets whose stage changed. The first board announces nothing.
 */
export function changes(
  before: KitchenBoardTicket[] | null,
  after: KitchenBoardTicket[],
): Change[] {
  if (before === null) return [];
  const known = new Map(before.map((t) => [t.id, t]));
  const out: Change[] = [];
  for (const ticket of after) {
    const was = known.get(ticket.id);
    if (!was) {
      if (isOpen(ticket)) out.push({ kind: "new", ticket });
    } else if (was.stage !== ticket.stage) {
      out.push({ kind: ticket.stage === "cancelled" ? "cancelled" : "stage", ticket });
    }
  }
  return out;
}
