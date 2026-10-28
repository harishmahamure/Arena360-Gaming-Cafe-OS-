import type { LowStockItem, LowStockView } from "@arena/ui/generated";
import { authHeaders, getToken, setToken } from "./auth";

export const LOW_STOCK_URL = "http://127.0.0.1:7409/ui/stock/low";
/** The largest quantity (and so threshold) the Hub stores. */
export const MAX_STOCK_QTY = 100_000_000;

export type LowStockOutcome =
  { kind: "ok"; view: LowStockView } | { kind: "signed_out" } | { kind: "unreachable" };

const isItem = (i: unknown) =>
  typeof i === "object" &&
  i !== null &&
  typeof (i as LowStockItem).product_id === "string" &&
  typeof (i as LowStockItem).product === "string" &&
  typeof (i as LowStockItem).qty === "number" &&
  (typeof (i as LowStockItem).low_stock_qty === "number" ||
    (i as LowStockItem).low_stock_qty === null);

/** `GET /ui/stock/low`: the products at or under their threshold, and every tracked product. */
export async function loadLowStock(): Promise<LowStockOutcome> {
  const token = getToken();
  if (!token) return { kind: "signed_out" };
  try {
    const res = await fetch(LOW_STOCK_URL, {
      headers: authHeaders(token),
      signal: AbortSignal.timeout(5000),
      cache: "no-store",
    });
    if (res.status === 401) {
      setToken(null);
      return { kind: "signed_out" };
    }
    const body = (res.ok ? await res.json() : null) as LowStockView | null;
    return body &&
      Array.isArray(body.low) &&
      Array.isArray(body.tracked) &&
      body.low.every(isItem) &&
      body.tracked.every(isItem)
      ? { kind: "ok", view: body }
      : { kind: "unreachable" };
  } catch {
    return { kind: "unreachable" };
  }
}

/**
 * A threshold typed into the field: empty clears it (`null`), digits give a whole quantity from 0
 * to the maximum, anything else is `undefined`.
 */
export function parseThreshold(text: string): number | null | undefined {
  const t = text.trim();
  if (t === "") return null;
  if (!/^\d{1,9}$/.test(t)) return undefined;
  const n = Number(t);
  return n <= MAX_STOCK_QTY ? n : undefined;
}
