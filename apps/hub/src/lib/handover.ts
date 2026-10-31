import type { HandoverView, UiError } from "@arena/ui/generated";
import { authHeaders, errorOf, getToken, noticeLocked } from "./auth";

/** The routes behind "Move this branch" on the Android Hub (Story 10.17): a manager's PIN confirms each. */
export const HANDOVER_URL = "http://127.0.0.1:7409/ui/handover";
/** The Windows Hub's join route: no session, loopback only. */
export const MIGRATION_JOIN_URL = "http://127.0.0.1:7409/ui/migration/join";
const TIMEOUT_MS = 120_000;

export type HandoverStep = "start" | "file" | "cancel";

export type HandoverResult =
  | { kind: "ok"; view: HandoverView | null }
  | { kind: "signed_out" }
  | { kind: "rejected" }
  | { kind: "locked"; retryAfterS: number }
  /** The Hub refused: `HUB_HANDING_OVER`, `VALIDATION_FAILED` (`folder`), … */
  | { kind: "refused"; error: UiError }
  | { kind: "unreachable" };

async function post(url: string, body: unknown, token: string | null): Promise<HandoverResult> {
  let res: Response;
  try {
    res = await fetch(url, {
      method: "POST",
      headers: { ...authHeaders(token), "content-type": "application/json" },
      body: JSON.stringify(body),
      signal: AbortSignal.timeout(TIMEOUT_MS),
    });
  } catch {
    return { kind: "unreachable" };
  }
  if (res.status === 401) {
    if (token) return { kind: "signed_out" };
  }
  if (res.ok) {
    try {
      return { kind: "ok", view: (await res.json()) as HandoverView | null };
    } catch {
      return { kind: "unreachable" };
    }
  }
  const err = await errorOf(res);
  noticeLocked(err);
  if (err?.code === "AUTH_LOCKED") {
    const s = err.retry_after_s;
    return { kind: "locked", retryAfterS: Number.isFinite(s) ? Math.max(1, Math.ceil(s!)) : 60 };
  }
  if (err?.code === "PIN_REJECTED") return { kind: "rejected" };
  return err ? { kind: "refused", error: err } : { kind: "unreachable" };
}

/** Start (a 6-digit code), write the snapshot to the backup folder, or cancel the move. */
export function handoverStep(step: HandoverStep, pin: string): Promise<HandoverResult> {
  const token = getToken();
  if (!token) return Promise.resolve({ kind: "signed_out" });
  return post(`${HANDOVER_URL}/${step}`, { pin }, token); // i18n-ignore (a URL)
}

/** The Windows Hub claims the code at the tablet's address and pulls the snapshot. */
export const joinMove = (address: string, code: string) =>
  post(MIGRATION_JOIN_URL, { address, code }, null);
