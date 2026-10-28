import type { PosSalesView } from "@arena/ui/generated";
import { authHeaders, getToken, setToken } from "./auth";

export const POS_SALES_URL = "http://127.0.0.1:7409/ui/pos/sales";

export type PosSalesOutcome =
  { kind: "ok"; view: PosSalesView } | { kind: "signed_out" } | { kind: "unreachable" };

/** `GET /ui/pos/sales`: the business day's counter sales, newest first. */
export async function loadPosSales(): Promise<PosSalesOutcome> {
  const token = getToken();
  if (!token) return { kind: "signed_out" };
  try {
    const res = await fetch(POS_SALES_URL, {
      headers: authHeaders(token),
      signal: AbortSignal.timeout(5000),
      cache: "no-store",
    });
    if (res.status === 401) {
      setToken(null);
      return { kind: "signed_out" };
    }
    const body: unknown = res.ok ? await res.json() : null;
    const ok =
      typeof body === "object" &&
      body !== null &&
      Array.isArray((body as { sales?: unknown }).sales);
    return ok ? { kind: "ok", view: body as PosSalesView } : { kind: "unreachable" };
  } catch {
    return { kind: "unreachable" };
  }
}
