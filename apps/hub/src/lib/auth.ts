// Staff sign-in against the local Hub service over loopback (`/ui/auth/*`, `/ui/me`,
// `/ui/intent`, `/ui/settings`). The bearer token lives for this Hub process only; the UI keeps
// it in sessionStorage so a reload stays signed in until the Hub restarts or the staff member
// signs out.

import type {
  Intent,
  IntentPayload,
  IntentResult,
  Permission,
  SessionStaff,
  SettingEntry,
  SignInOk,
  UiError,
} from "@arena/ui/generated";
import { ulid } from "./ulid";

export const SIGN_IN_URL = "http://127.0.0.1:7409/ui/auth/sign-in";
export const SIGN_OUT_URL = "http://127.0.0.1:7409/ui/auth/sign-out";
export const ME_URL = "http://127.0.0.1:7409/ui/me";
export const INTENT_URL = "http://127.0.0.1:7409/ui/intent";
export const SETTINGS_URL = "http://127.0.0.1:7409/ui/settings";
/** The `depends()` key for the session, so sign-in and sign-out re-run the layout load. */
export const SESSION_DEPENDENCY = "hub:session";
const TOKEN_KEY = "arena.hub.token";
const TIMEOUT_MS = 5000;

export function getToken(): string | null {
  try {
    return sessionStorage.getItem(TOKEN_KEY);
  } catch {
    return null;
  }
}

export function setToken(token: string | null) {
  try {
    if (token) sessionStorage.setItem(TOKEN_KEY, token);
    else sessionStorage.removeItem(TOKEN_KEY);
  } catch {
    // Storage unavailable: the session lasts until the page reloads.
  }
}

/** Who is signed in: known, known to be nobody, or unknown (the Hub did not answer). */
export type Session =
  { state: "signed_in"; staff: SessionStaff } | { state: "signed_out" } | { state: "unknown" };

/**
 * Where sign-in returns to: a same-app path from `redirectTo`, never another origin (`//x`),
 * never the sign-in screen itself; `/floor` otherwise.
 */
export function safeRedirect(target: string | null): string {
  if (!target || !target.startsWith("/") || target.startsWith("//") || target.startsWith("/\\")) {
    return "/floor";
  }
  return target.startsWith("/sign-in") ? "/floor" : target;
}

const isObject = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v);

export function isSessionStaff(v: unknown): v is SessionStaff {
  return (
    isObject(v) &&
    typeof v.staff_id === "string" &&
    typeof v.display_name === "string" &&
    typeof v.role_name === "string" &&
    (v.permissions === undefined ||
      (Array.isArray(v.permissions) && v.permissions.every((p) => typeof p === "string")))
  );
}

/** Whether the member's role allows `permission` (the Hub still checks every intent). */
export function hasPermission(staff: SessionStaff | null, permission: Permission): boolean {
  return Array.isArray(staff?.permissions) && staff.permissions.includes(permission);
}

function authHeaders(token: string | null): HeadersInit {
  return token ? { authorization: `Bearer ${token}` } : {}; // i18n-ignore (an HTTP scheme)
}

/** Reads `GET /ui/me`. A 401 forgets the token. */
export async function loadSession(fetcher: typeof fetch): Promise<Session> {
  const token = getToken();
  if (!token) return { state: "signed_out" };
  try {
    const res = await fetcher(ME_URL, {
      headers: authHeaders(token),
      signal: AbortSignal.timeout(TIMEOUT_MS),
      cache: "no-store",
    });
    if (res.status === 401) {
      setToken(null);
      return { state: "signed_out" };
    }
    if (!res.ok) return { state: "unknown" };
    const staff: unknown = await res.json();
    return isSessionStaff(staff) ? { state: "signed_in", staff } : { state: "unknown" };
  } catch {
    return { state: "unknown" };
  }
}

export type SignInResult =
  | { kind: "ok"; staff: SessionStaff }
  | { kind: "failed" }
  | { kind: "locked"; retryAfterS: number }
  | { kind: "unreachable" };

async function errorOf(res: Response): Promise<UiError | null> {
  try {
    const body: unknown = await res.json();
    return isObject(body) && typeof body.code === "string" ? (body as UiError) : null;
  } catch {
    return null;
  }
}

/** `POST /ui/auth/sign-in`. On success the token is stored. */
export async function signIn(login: string, password: string): Promise<SignInResult> {
  let res: Response;
  try {
    res = await fetch(SIGN_IN_URL, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ login, password }),
      signal: AbortSignal.timeout(TIMEOUT_MS * 2),
    });
  } catch {
    return { kind: "unreachable" };
  }
  if (res.ok) {
    let body: Partial<SignInOk> | null = null;
    try {
      body = (await res.json()) as Partial<SignInOk>;
    } catch {
      return { kind: "unreachable" };
    }
    if (typeof body?.token === "string" && isSessionStaff(body.staff)) {
      setToken(body.token);
      return { kind: "ok", staff: body.staff };
    }
    // A malformed answer that still carries a token: end that session rather than leave it.
    if (typeof body?.token === "string") {
      setToken(body.token);
      await signOut();
    }
    return { kind: "unreachable" };
  }
  const err = await errorOf(res);
  if (err?.code === "AUTH_LOCKED") {
    const s = err.retry_after_s;
    return { kind: "locked", retryAfterS: Number.isFinite(s) ? Math.max(1, Math.ceil(s!)) : 60 };
  }
  if (err?.code === "AUTH_FAILED") return { kind: "failed" };
  return { kind: "unreachable" };
}

/** `POST /ui/auth/sign-out`; the token is forgotten whatever the Hub answers. */
export async function signOut() {
  const token = getToken();
  setToken(null);
  if (!token) return;
  try {
    await fetch(SIGN_OUT_URL, {
      method: "POST",
      headers: authHeaders(token),
      signal: AbortSignal.timeout(TIMEOUT_MS),
    });
  } catch {
    // The session ends with the Hub process anyway.
  }
}

export type IntentOutcome =
  | { kind: "applied"; result: IntentResult }
  | { kind: "signed_out" }
  | { kind: "error"; error: UiError }
  | { kind: "unreachable" };

/** A fresh idempotency key: mint one per user action and reuse it when that action is retried. */
export const newIdempotencyKey = (): string => ulid();

/**
 * `POST /ui/intent`. The Hub replaces the actor with the signed-in staff member, so the actor
 * sent here is only a placeholder. `idempotencyKey` comes from the caller, minted once per user
 * action, so a retry of the same action is deduplicated by the Hub. A 401 forgets the token
 * (the caller goes to sign-in).
 */
export async function submitIntent(
  payload: IntentPayload,
  idempotencyKey: string,
  baseRev?: number,
): Promise<IntentOutcome> {
  const token = getToken();
  if (!token) return { kind: "signed_out" };
  const now = Date.now();
  const intent: Intent = {
    intent_id: ulid(now),
    kind: payload.kind,
    idempotency_key: idempotencyKey,
    issued_at: now,
    base_rev: baseRev,
    actor: { kind: "staff", id: "session" },
    payload,
  };
  let res: Response;
  try {
    res = await fetch(INTENT_URL, {
      method: "POST",
      headers: { ...authHeaders(token), "content-type": "application/json" },
      body: JSON.stringify({ intent }),
      signal: AbortSignal.timeout(TIMEOUT_MS),
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
      return { kind: "applied", result: (await res.json()) as IntentResult };
    } catch {
      return { kind: "unreachable" };
    }
  }
  const error = await errorOf(res);
  return error ? { kind: "error", error } : { kind: "unreachable" };
}

export type SettingsOutcome =
  { kind: "ok"; settings: SettingEntry[] } | { kind: "signed_out" } | { kind: "unreachable" };

/** `GET /ui/settings`: every branch setting with its `rev` (the `base_rev` of an edit). */
export async function loadSettings(): Promise<SettingsOutcome> {
  const token = getToken();
  if (!token) return { kind: "signed_out" };
  try {
    const res = await fetch(SETTINGS_URL, {
      headers: authHeaders(token),
      signal: AbortSignal.timeout(TIMEOUT_MS),
      cache: "no-store",
    });
    if (res.status === 401) {
      setToken(null);
      return { kind: "signed_out" };
    }
    if (!res.ok) return { kind: "unreachable" };
    const body: unknown = await res.json();
    if (
      !Array.isArray(body) ||
      !body.every(
        (s) =>
          isObject(s) &&
          typeof s.key === "string" &&
          typeof s.value === "string" &&
          typeof s.rev === "number",
      )
    ) {
      return { kind: "unreachable" };
    }
    return { kind: "ok", settings: body as SettingEntry[] };
  } catch {
    return { kind: "unreachable" };
  }
}
