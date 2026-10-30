import type { RestoreCheck } from "@arena/ui/generated";
import type { MessageKey } from "@arena/ui/messages";
import { authHeaders, errorOf, getToken, noticeLocked, setToken } from "./auth";

export const RESTORE_CHECK_URL = "http://127.0.0.1:7409/ui/backup/restore/check";
export const RESTORE_URL = "http://127.0.0.1:7409/ui/backup/restore";

/** Why a restore was not started or a file was not accepted. */
export type RestoreFailure =
  | { kind: "problem"; message: MessageKey }
  | { kind: "pin_rejected" }
  | { kind: "locked"; retryAfterS: number }
  | { kind: "signed_out" }
  | { kind: "failed" };

export type CheckOutcome = { kind: "ok"; check: RestoreCheck } | RestoreFailure;
export type RestoreOutcome = { kind: "ok" } | RestoreFailure;

const PATH_PROBLEMS: Readonly<Record<string, MessageKey>> = {
  not_set: "hub.restore.problem.not_set",
  not_found: "hub.restore.problem.not_found",
  not_file: "hub.restore.problem.not_file",
  not_readable: "hub.restore.problem.not_readable",
  not_absolute: "hub.restore.problem.not_absolute",
  unsupported: "hub.restore.problem.path_unsupported",
};

/** POSTs `payload` to a restore route; the answer's JSON on success, else why not. */
async function post(
  url: string,
  payload: object,
): Promise<{ kind: "ok"; body: unknown } | RestoreFailure> {
  const token = getToken();
  if (!token) return { kind: "signed_out" };
  let res: Response;
  try {
    res = await fetch(url, {
      method: "POST",
      headers: { ...authHeaders(token), "content-type": "application/json" },
      body: JSON.stringify(payload),
      // Checking a big file reads all of it.
      signal: AbortSignal.timeout(300_000),
      cache: "no-store",
    });
  } catch {
    return { kind: "failed" };
  }
  if (res.status === 401) {
    setToken(null);
    return { kind: "signed_out" };
  }
  if (res.ok) {
    try {
      return { kind: "ok", body: await res.json() };
    } catch {
      return { kind: "failed" };
    }
  }
  const error = await errorOf(res);
  noticeLocked(error);
  switch (error?.code) {
    case "RESTORE_CORRUPT":
      return { kind: "problem", message: "hub.restore.problem.corrupt" };
    case "RESTORE_WRONG_BRANCH":
      return { kind: "problem", message: "hub.restore.problem.wrong_branch" };
    case "SCHEMA_TOO_NEW":
      return { kind: "problem", message: "hub.restore.problem.too_new" };
    case "VALIDATION_FAILED": {
      if (error.field === "branch_code") {
        return { kind: "problem", message: "hub.restore.problem.branch_code" };
      }
      const message = error.field === "path" ? PATH_PROBLEMS[error.message] : undefined;
      return message ? { kind: "problem", message } : { kind: "failed" };
    }
    case "PIN_REJECTED":
      return { kind: "pin_rejected" };
    case "AUTH_LOCKED": {
      const s = error.retry_after_s;
      return { kind: "locked", retryAfterS: Number.isFinite(s) ? Math.max(1, Math.ceil(s!)) : 60 };
    }
    case "PERMISSION_DENIED":
      return { kind: "problem", message: "hub.restore.problem.denied" };
    case "UNSUPPORTED":
      return { kind: "problem", message: "hub.restore.problem.unsupported" };
    case "NOT_PRIMARY":
      return { kind: "problem", message: "hub.restore.problem.not_primary" };
    default:
      return { kind: "failed" };
  }
}

/**
 * `POST /ui/backup/restore/check`: what the file holds and what a restore would set aside. The
 * Owner checks with their session; anyone else gives the Owner's `pin`.
 */
export async function checkRestore(path: string, pin?: string): Promise<CheckOutcome> {
  const out = await post(RESTORE_CHECK_URL, pin ? { path, pin } : { path });
  if (out.kind !== "ok") return out;
  const body = out.body as RestoreCheck;
  return typeof body.branch_name === "string" && typeof body.exported_at === "number"
    ? { kind: "ok", check: body }
    : { kind: "failed" };
}

/**
 * `POST /ui/backup/restore`: with the Owner's PIN and the typed branch code the Hub stages the
 * file and restarts. The restore runs when it starts again.
 */
export async function confirmRestore(
  path: string,
  pin: string,
  branchCode: string,
): Promise<RestoreOutcome> {
  const out = await post(RESTORE_URL, { path, pin, branch_code: branchCode });
  if (out.kind !== "ok") return out;
  return (out.body as { restarting?: unknown }).restarting === true
    ? { kind: "ok" }
    : { kind: "failed" };
}
