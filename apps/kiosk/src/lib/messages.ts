// The messages bell's rules (Story 6.16). Pure and free of the UI package so the tests can run it
// in Node.

import type { MessageKey } from "@arena/ui/messages";
import type { StaffMessage } from "./bridge";

/** The most messages the bell lists (the service keeps no more). */
export const MAX_MESSAGES = 50;

/** One row of the bell. */
export type Note = Pick<StaffMessage, "id" | "text" | "at" | "read"> &
  Partial<Pick<StaffMessage, "code" | "count">>;

/** The catalogue line of an order note (Story 6.33), or `null` for a staff message. */
export function orderCopy(
  n: Pick<StaffMessage, "text"> & Partial<Pick<StaffMessage, "code" | "count">>,
): { key: MessageKey; params: Record<string, string | number> } | null {
  switch (n.code) {
    case "order.placed":
      return (n.count ?? 0) === 1
        ? { key: "player.order.placed_one", params: {} }
        : { key: "player.order.placed", params: { count: n.count ?? 0 } };
    case "order.ready_seat":
      return { key: "player.order.ready_seat", params: {} };
    case "order.ready_counter":
      return { key: "player.order.ready_counter", params: {} };
    case "order.cancelled":
      return { key: "player.order.cancelled", params: { item: n.text } };
    default:
      return null;
  }
}

/** The words a note shows: an order note from the catalogue, a staff message as sent. */
export function noteText(
  n: Pick<StaffMessage, "text"> & Partial<Pick<StaffMessage, "code" | "count">>,
  translate: (key: MessageKey, params?: Record<string, string | number>) => string,
): string {
  const copy = orderCopy(n);
  return copy ? translate(copy.key, copy.params) : n.text;
}

/** `list` (newest first) with `m` added where its age puts it; a message already held is ignored. */
export function add(list: readonly Note[], m: Note): Note[] {
  if (list.some((n) => n.id === m.id)) return [...list];
  const next = [
    { id: m.id, text: m.text, at: m.at, read: m.read, code: m.code ?? null, count: m.count ?? 0 },
    ...list,
  ];
  next.sort((a, b) => b.id - a.id);
  return next.slice(0, MAX_MESSAGES);
}

/** How many messages the player has not seen. */
export function unread(list: readonly Note[]): number {
  return list.filter((n) => !n.read).length;
}

/** Every message marked read. */
export function markRead(list: readonly Note[]): Note[] {
  return list.map((n) => (n.read ? n : { ...n, read: true }));
}

/** The time of day a message came in, as the player's clock shows it ("14:05"). */
export function timeOf(at: number): string {
  return new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit" }).format(
    new Date(at),
  );
}
