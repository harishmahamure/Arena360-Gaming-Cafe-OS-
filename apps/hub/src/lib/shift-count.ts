import type { ShiftCount, UiError } from "@arena/ui/generated";
import { authHeaders, errorOf, getToken, noticeLocked, setToken } from "./auth";

export const SHIFT_COUNT_URL = "http://127.0.0.1:7409/ui/shift/count";

export type CountReveal =
  | { kind: "ok"; count: ShiftCount }
  | { kind: "signed_out" }
  | { kind: "error"; error: UiError | null }
  | { kind: "unreachable" };

/**
 * `GET /ui/shift/count?counted_minor=`: the expected cash, the signed difference and the
 * breakdown for a stated count. The Hub never returns the expected cash without one.
 */
export async function revealCount(countedMinor: number): Promise<CountReveal> {
  const token = getToken();
  if (!token) return { kind: "signed_out" };
  const url = new URL(SHIFT_COUNT_URL);
  url.searchParams.set("counted_minor", String(countedMinor));
  let res: Response;
  try {
    res = await fetch(url.href, {
      headers: authHeaders(token),
      signal: AbortSignal.timeout(5000),
      cache: "no-store",
    });
  } catch {
    return { kind: "unreachable" };
  }
  if (res.status === 401) {
    setToken(null);
    return { kind: "signed_out" };
  }
  if (res.ok) {
    try {
      return { kind: "ok", count: (await res.json()) as ShiftCount };
    } catch {
      return { kind: "unreachable" };
    }
  }
  const error = await errorOf(res);
  noticeLocked(error);
  return { kind: "error", error };
}
