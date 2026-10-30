// The "updated by owner" toast (Story 8.19): the Hub applies an owner's remote change and tells
// the shell with `updated_by_owner`. One table keys the toast line by command kind; a later kind
// adds a row here and a line to the catalogue.

import type { OverwrittenChange, UpdatedByOwnerEvent } from "@arena/ui/generated";
import type { MessageKey } from "@arena/ui/messages";

export const OWNER_UPDATE_COPY = {
  "rate.upsert": "hub.owner_update.rate",
  "plan.upsert": "hub.owner_update.plan",
  "rule.upsert": "hub.owner_update.rule",
  "coupon.upsert": "hub.owner_update.coupon",
  "product.upsert": "hub.owner_update.product",
  "staff.upsert": "hub.owner_update.staff",
  // A deactivation is a staff update (Story 8.26).
  "staff.deactivate": "hub.owner_update.staff",
  "role.upsert": "hub.owner_update.role",
} as const satisfies Record<string, MessageKey>;

const isObject = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v);

/** The catalogue key of the toast line for a command kind, or `null` for a kind with none. */
export function ownerUpdateKey(kind: string): MessageKey | null {
  return Object.hasOwn(OWNER_UPDATE_COPY, kind)
    ? OWNER_UPDATE_COPY[kind as keyof typeof OWNER_UPDATE_COPY]
    : null;
}

function isOverwritten(v: unknown): v is OverwrittenChange {
  return (
    isObject(v) &&
    typeof v.intent_id === "string" &&
    typeof v.actor_kind === "string" &&
    typeof v.actor_id === "string" &&
    typeof v.at === "number"
  );
}

export function isUpdatedByOwner(v: unknown): v is UpdatedByOwnerEvent {
  return (
    isObject(v) &&
    typeof v.kind === "string" &&
    typeof v.entity === "string" &&
    typeof v.row_id === "string" &&
    (v.overwritten === undefined || isOverwritten(v.overwritten))
  );
}
