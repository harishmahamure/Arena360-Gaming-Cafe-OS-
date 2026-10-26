import type { MessageKey } from "./messages/index.js";

export type Status =
  | "free"
  | "in-session"
  | "paused"
  | "reserved"
  | "order-pending"
  | "maintenance"
  | "offline"
  | "unassigned"
  | "warning";

export const STATUS_LABELS: Record<Status, MessageKey> = {
  free: "ui.status.free",
  "in-session": "ui.status.in_session",
  paused: "ui.status.paused",
  reserved: "ui.status.reserved",
  "order-pending": "ui.status.order_pending",
  maintenance: "ui.status.maintenance",
  offline: "ui.status.offline",
  unassigned: "ui.status.unassigned",
  warning: "ui.status.warning",
};
