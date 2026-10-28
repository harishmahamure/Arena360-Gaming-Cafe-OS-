// Attaching the POS cart to a running session (Story 5.12): which sessions take an order and
// what the cart header calls them. Pure functions over the snapshot; the page owns the state.

import type { Product, SnapshotDevice, SnapshotSession } from "@arena/ui/generated";
import type { CartLine } from "./pos";

/** A session an order can go on: a running postpaid session outside a group bill, or a block. */
export function takesOrder(s: SnapshotSession): boolean {
  const running = s.state === "active" || s.state === "paused" || s.state === "grace";
  const mode = s.mode ?? "postpaid";
  return running && (mode === "block" || (mode === "postpaid" && !s.group_bill_id));
}

export type OrderSession = {
  session: SnapshotSession;
  /** The player's username; `null` for a guest. */
  player: string | null;
  device: string;
};

/** Every running session that takes an order, in start order, with its player and PC name. */
export function orderSessions(
  sessions: SnapshotSession[],
  devices: SnapshotDevice[],
): OrderSession[] {
  const names = new Map(devices.map((d) => [d.device_id, d.label]));
  return sessions
    .filter(takesOrder)
    .sort((a, b) => a.started_at - b.started_at)
    .map((session) => ({
      session,
      player: session.username ?? null,
      device: names.get(session.device_id) ?? "",
    }));
}

/** What an order adds to the session bill: each line at the product's price, no tax. */
export const cartAmount = (cart: CartLine[], byId: Map<string, Product>): number =>
  cart.reduce((sum, l) => sum + (byId.get(l.productId)?.price_minor ?? 0) * l.qty, 0);
