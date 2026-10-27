import type { StaffList } from "@arena/ui/generated";
import { authHeaders, errorOf, getToken, newIdempotencyKey, setToken } from "./auth";

export const STAFF_URL = "http://127.0.0.1:7409/ui/staff";
export const STAFF_DEACTIVATE_URL = "http://127.0.0.1:7409/ui/staff/deactivate";

export type StaffOutcome =
  | { kind: "ok"; list: StaffList }
  | { kind: "denied" }
  | { kind: "signed_out" }
  | { kind: "unreachable" };

/** `GET /ui/staff`: the active and invited members and the `users` limit in effect. */
export async function loadStaff(): Promise<StaffOutcome> {
  const token = getToken();
  if (!token) return { kind: "signed_out" };
  try {
    const res = await fetch(STAFF_URL, {
      headers: authHeaders(token),
      signal: AbortSignal.timeout(5000),
      cache: "no-store",
    });
    if (res.status === 401) {
      setToken(null);
      return { kind: "signed_out" };
    }
    if (res.status === 403) return { kind: "denied" };
    const body = res.ok ? ((await res.json()) as Partial<StaffList>) : null;
    return body && Array.isArray(body.staff)
      ? { kind: "ok", list: body as StaffList }
      : { kind: "unreachable" };
  } catch {
    return { kind: "unreachable" };
  }
}

export type DeactivateOutcome =
  | { kind: "done" }
  | { kind: "rejected" }
  | { kind: "locked"; retryAfterS: number }
  | { kind: "refused"; field: string | null }
  | { kind: "signed_out" }
  | { kind: "unreachable" };

/**
 * `POST /ui/staff/deactivate`: the signed-in Owner confirms with their own PIN. `key` is minted
 * once per user action, so a retry of the same tap is deduplicated by the Hub.
 */
export async function deactivateStaff(
  staffId: string,
  pin: string,
  key: string = newIdempotencyKey(),
): Promise<DeactivateOutcome> {
  const token = getToken();
  if (!token) return { kind: "signed_out" };
  let res: Response;
  try {
    res = await fetch(STAFF_DEACTIVATE_URL, {
      method: "POST",
      headers: { ...authHeaders(token), "content-type": "application/json" },
      body: JSON.stringify({ staff_id: staffId, pin, idempotency_key: key }),
      signal: AbortSignal.timeout(10000),
    });
  } catch {
    return { kind: "unreachable" };
  }
  if (res.status === 401) {
    setToken(null);
    return { kind: "signed_out" };
  }
  if (res.ok) return { kind: "done" };
  const err = await errorOf(res);
  if (err?.code === "PIN_REJECTED") return { kind: "rejected" };
  if (err?.code === "AUTH_LOCKED") {
    const s = err.retry_after_s;
    return { kind: "locked", retryAfterS: Number.isFinite(s) ? Math.max(1, Math.ceil(s!)) : 60 };
  }
  if (err?.code === "VALIDATION_FAILED" || err?.code === "PERMISSION_DENIED") {
    return { kind: "refused", field: err.field ?? null };
  }
  return { kind: "unreachable" };
}
