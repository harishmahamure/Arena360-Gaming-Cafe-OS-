// The cloud API client: `${PUBLIC_API_BASE}/v1/...` with the session cookies. Every failure
// comes back as the flat `AppError` envelope; a network failure or a body that is not one is
// reported as `INTERNAL` with `offline` set.

import { PUBLIC_API_BASE } from "$app/env/public";
import type {
  AppError,
  ErrorCode,
  SessionInfo,
  SignupChallenge,
  SignupStart,
  SignupVerified,
} from "@arena/ui/generated";

/** The API origin (empty: same origin, through the dev/preview proxy). */
const base = () => (PUBLIC_API_BASE ?? "").replace(/\/$/, "");

export type ApiFailure = {
  ok: false;
  status: number;
  error: AppError;
  /** Seconds from `Retry-After` (`RATE_LIMITED`), when sent. */
  retryAfterS: number | null;
  /** The API could not be reached at all. */
  offline: boolean;
};
export type ApiResult<T> = { ok: true; status: number; data: T } | ApiFailure;

const CODES: ReadonlySet<string> = new Set<ErrorCode>([
  "VALIDATION_FAILED",
  "RATE_LIMITED",
  "OTP_INVALID",
  "OTP_EXPIRED",
  "AUTH_FAILED",
  "FORBIDDEN",
  "NOT_FOUND",
  "BAD_REQUEST",
  "INTERNAL",
]);

function isAppError(v: unknown): v is AppError {
  if (!v || typeof v !== "object") return false;
  const o = v as Record<string, unknown>;
  return typeof o.code === "string" && typeof o.message === "string";
}

function internal(status: number, offline: boolean): ApiFailure {
  return {
    ok: false,
    status,
    error: { code: "INTERNAL", message: "" },
    retryAfterS: null,
    offline,
  };
}

/** Seconds in a `Retry-After` header (delta-seconds or an HTTP date), or null. */
export function retryAfterSeconds(header: string | null, now = Date.now()): number | null {
  if (!header) return null;
  const n = Number(header.trim());
  if (Number.isFinite(n)) return Math.max(0, Math.ceil(n));
  const at = Date.parse(header);
  return Number.isNaN(at) ? null : Math.max(0, Math.ceil((at - now) / 1000));
}

async function call<T>(
  method: "GET" | "POST",
  path: string,
  body?: unknown,
): Promise<ApiResult<T>> {
  let res: Response;
  try {
    res = await fetch(base() + "/v1" + path, {
      method,
      credentials: "include",
      headers:
        body === undefined
          ? { accept: "application/json" }
          : { "content-type": "application/json", accept: "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
  } catch {
    return internal(0, true);
  }
  let parsed: unknown = null;
  try {
    parsed = await res.json();
  } catch {
    parsed = null;
  }
  if (res.ok) return { ok: true, status: res.status, data: parsed as T };
  if (!isAppError(parsed)) return internal(res.status, false);
  const error: AppError = CODES.has(parsed.code) ? parsed : { ...parsed, code: "INTERNAL" };
  return {
    ok: false,
    status: res.status,
    error,
    retryAfterS: retryAfterSeconds(res.headers.get("retry-after")),
    offline: false,
  };
}

export const startSignup = (form: SignupStart) =>
  call<SignupChallenge>("POST", "/signup/start", form);
export const resendCode = (challenge_id: string) =>
  call<SignupChallenge>("POST", "/signup/resend", { challenge_id });
export const verifyCode = (challenge_id: string, code: string) =>
  call<SignupVerified>("POST", "/signup/verify", { challenge_id, code });
export const getSession = () => call<SessionInfo>("GET", "/session");
export const refreshSession = () => call<SessionInfo>("POST", "/session/refresh");

/**
 * `GET /v1/session`; on a 401 (the 15-minute access cookie ran out) it rotates the session
 * once with `POST /v1/session/refresh` and asks again.
 */
export async function currentSession(): Promise<ApiResult<SessionInfo>> {
  const first = await getSession();
  if (first.ok || first.status !== 401) return first;
  const refreshed = await refreshSession();
  if (!refreshed.ok) return refreshed;
  return getSession();
}
