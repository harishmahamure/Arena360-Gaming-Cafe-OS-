import type { ExpiryView } from "@arena/ui/generated";
import { authHeaders, getToken, setToken } from "./auth";

export const EXPIRY_URL = "http://127.0.0.1:7409/ui/stock/expiry";
/** The branch setting behind "Warn before expiry (days)". */
export const KEY_EXPIRY_WARN_DAYS = "stock.expiry_warn_days";
export const WARN_DAYS_MAX = 30;

export type ExpiryOutcome =
  { kind: "ok"; view: ExpiryView } | { kind: "signed_out" } | { kind: "unreachable" };

const isLot = (l: unknown) =>
  typeof l === "object" &&
  l !== null &&
  typeof (l as { lot_id?: unknown }).lot_id === "string" &&
  typeof (l as { qty?: unknown }).qty === "number" &&
  typeof (l as { expiry?: unknown }).expiry === "string" &&
  typeof (l as { product?: unknown }).product === "string" &&
  typeof (l as { value_minor?: unknown }).value_minor === "number";

/** `GET /ui/stock/expiry`: the lots inside the warning window and the lots already expired. */
export async function loadExpiry(): Promise<ExpiryOutcome> {
  const token = getToken();
  if (!token) return { kind: "signed_out" };
  try {
    const res = await fetch(EXPIRY_URL, {
      headers: authHeaders(token),
      signal: AbortSignal.timeout(5000),
      cache: "no-store",
    });
    if (res.status === 401) {
      setToken(null);
      return { kind: "signed_out" };
    }
    const body = (res.ok ? await res.json() : null) as ExpiryView | null;
    return body &&
      Array.isArray(body.expiring) &&
      Array.isArray(body.expired) &&
      body.expiring.every(isLot) &&
      body.expired.every(isLot)
      ? { kind: "ok", view: body }
      : { kind: "unreachable" };
  } catch {
    return { kind: "unreachable" };
  }
}

/** A whole number of days from 0 to 30, or `null`. */
export function parseWarnDays(text: string): number | null {
  const t = text.trim();
  if (!/^\d{1,2}$/.test(t)) return null;
  const n = Number(t);
  return n <= WARN_DAYS_MAX ? n : null;
}
