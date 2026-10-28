/** The branch setting behind Settings → Inventory → "Take stock at". */
export const KEY_TAKE_STOCK_AT = "inventory.take_stock_at";

/** `served`: kitchen items hold their stock until marked served. `order`: stock leaves at placement. */
export const TAKE_STOCK_MODES = ["served", "order"] as const;
export type TakeStockAt = (typeof TAKE_STOCK_MODES)[number];

/** The stored value as a mode, or `null` for anything else. */
export function parseTakeStockAt(value: string | undefined): TakeStockAt | null {
  return TAKE_STOCK_MODES.find((m) => m === value) ?? null;
}
