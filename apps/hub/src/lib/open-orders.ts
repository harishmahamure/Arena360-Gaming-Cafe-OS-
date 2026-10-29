// The POS "Open orders" list (Story 6.32): the unpaid counter orders a kiosk left at the
// counter. They are the snapshot's bills that have an order but no session; a session's own bill
// (even an ended one waiting to be settled) always has one.

import type { SnapshotBill } from "@arena/ui/generated";

/** The unpaid counter orders among `bills`, oldest first as the snapshot lists them. */
export const openCounterOrders = (bills: readonly SnapshotBill[]): SnapshotBill[] =>
  bills.filter((b) => b.order_id != null && b.session_id === "");
