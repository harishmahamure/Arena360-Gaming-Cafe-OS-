// Staff sign-in against the local Hub service over loopback (`/ui/auth/*`, `/ui/me`,
// `/ui/elevate`, `/ui/intent`, `/ui/settings`). The bearer token lives for this Hub process only; the UI keeps
// it in sessionStorage so a reload stays signed in until the Hub restarts or the staff member
// signs out.

import type {
  ElevateOk,
  ElevationItem,
  Intent,
  IntentPayload,
  UiIntentResult,
  Permission,
  PlayerEditForm,
  PlayerForm,
  PlayerHit,
  PlayerPasswordForm,
  PlayerProfile,
  WalletView,
  ReasonCode,
  SessionStaff,
  SettingEntry,
  SignInOk,
  UiError,
  UnlockOk,
} from "@arena/ui/generated";
import { ulid } from "./ulid";

export const SIGN_IN_URL = "http://127.0.0.1:7409/ui/auth/sign-in";
export const SETUP_CODE_URL = "http://127.0.0.1:7409/ui/auth/setup-code";
export const SIGN_OUT_URL = "http://127.0.0.1:7409/ui/auth/sign-out";
export const ME_URL = "http://127.0.0.1:7409/ui/me";
export const INTENT_URL = "http://127.0.0.1:7409/ui/intent";
export const PLAYERS_URL = "http://127.0.0.1:7409/ui/players";
export const PLAYER_PROFILE_URL = "http://127.0.0.1:7409/ui/players/profile";
export const PLAYER_WALLET_URL = "http://127.0.0.1:7409/ui/players/wallet";
export const PLAYER_UPDATE_URL = "http://127.0.0.1:7409/ui/players/update";
export const PLAYER_PASSWORD_URL = "http://127.0.0.1:7409/ui/players/password";
export const ELEVATE_URL = "http://127.0.0.1:7409/ui/elevate";
export const ELEVATE_DROP_URL = "http://127.0.0.1:7409/ui/elevate/drop";
export const SETTINGS_URL = "http://127.0.0.1:7409/ui/settings";
export const LOCK_URL = "http://127.0.0.1:7409/ui/lock";
export const UNLOCK_URL = "http://127.0.0.1:7409/ui/unlock";
/** The `depends()` key for the session, so sign-in and sign-out re-run the layout load. */
export const SESSION_DEPENDENCY = "hub:session";
const TOKEN_KEY = "arena.hub.token";
/** Set while this window shows the idle lock, so a reload shows it again. */
const LOCKED_KEY = "arena.hub.locked";
const TIMEOUT_MS = 5000;

/** Whether this window last showed the idle lock (survives a reload). */
export function getLocalLocked(): boolean {
  try {
    return sessionStorage.getItem(LOCKED_KEY) === "1";
  } catch {
    return false;
  }
}

export function setLocalLocked(locked: boolean) {
  try {
    if (locked) sessionStorage.setItem(LOCKED_KEY, "1");
    else sessionStorage.removeItem(LOCKED_KEY);
  } catch {
    // Storage unavailable: the Hub still holds the session locked.
  }
}

let sessionLockedHandler: (() => void) | null = null;

/** Called whenever the Hub answers `SESSION_LOCKED` (the layout locks the screen). */
export function onSessionLocked(handler: (() => void) | null) {
  sessionLockedHandler = handler;
}

let notPairedHandler: (() => void) | null = null;

export function onHubNotPaired(handler: (() => void) | null) {
  notPairedHandler = handler;
}

export function noticeLocked(err: UiError | null) {
  if (err?.code === "SESSION_LOCKED") sessionLockedHandler?.();
  if (err?.code === "HUB_NOT_PAIRED") notPairedHandler?.();
}

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
    else {
      sessionStorage.removeItem(TOKEN_KEY);
      sessionStorage.removeItem(LOCKED_KEY);
    }
  } catch {
    // Storage unavailable: the session lasts until the page reloads.
  }
}

/**
 * Who is signed in: known (and whether the Hub holds the session idle-locked), known to be
 * nobody, or unknown (the Hub did not answer).
 */
export type Session =
  | { state: "signed_in"; staff: SessionStaff; locked: boolean; hasPin: boolean }
  | { state: "signed_out" }
  | { state: "unknown" };

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

export function authHeaders(token: string | null): HeadersInit {
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
    const body: unknown = await res.json();
    if (!isSessionStaff(body)) return { state: "unknown" };
    const { locked, has_pin, ...staff } = body as SessionStaff & {
      locked?: unknown;
      has_pin?: unknown;
    };
    return { state: "signed_in", staff, locked: locked === true, hasPin: has_pin !== false };
  } catch {
    return { state: "unknown" };
  }
}

export type SignInResult =
  | { kind: "ok"; staff: SessionStaff }
  | { kind: "failed" }
  | { kind: "locked"; retryAfterS: number }
  | { kind: "unreachable" };

export async function errorOf(res: Response): Promise<UiError | null> {
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

export type SetupResult =
  | { kind: "ok"; staff: SessionStaff }
  | { kind: "invalid" }
  | { kind: "locked"; retryAfterS: number }
  | { kind: "weak"; field: "password" | "pin" }
  | { kind: "unreachable" };

/** `POST /ui/auth/setup-code`: spends a one-time code to set the password and PIN, then signs in. */
export async function useSetupCode(
  login: string,
  code: string,
  password: string,
  pin: string,
): Promise<SetupResult> {
  let res: Response;
  try {
    res = await fetch(SETUP_CODE_URL, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ login, code, password, pin }),
      signal: AbortSignal.timeout(TIMEOUT_MS * 4),
    });
  } catch {
    return { kind: "unreachable" };
  }
  if (res.ok) {
    try {
      const body = (await res.json()) as Partial<SignInOk>;
      if (typeof body.token === "string" && isSessionStaff(body.staff)) {
        setToken(body.token);
        return { kind: "ok", staff: body.staff };
      }
    } catch {
      // fall through
    }
    return { kind: "unreachable" };
  }
  const err = await errorOf(res);
  if (err?.code === "AUTH_LOCKED") {
    const s = err.retry_after_s;
    return { kind: "locked", retryAfterS: Number.isFinite(s) ? Math.max(1, Math.ceil(s!)) : 60 };
  }
  if (err?.code === "SETUP_CODE_INVALID") return { kind: "invalid" };
  if (err?.code === "PASSWORD_TOO_SHORT") return { kind: "weak", field: "password" };
  if (err?.code === "VALIDATION_FAILED" && (err.field === "password" || err.field === "pin")) {
    return { kind: "weak", field: err.field };
  }
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
  | { kind: "applied"; result: UiIntentResult }
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
  options: { elevationTicket?: string; reason?: ReasonCode } = {},
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
    reason: options.reason,
    payload,
  };
  return postIntent(INTENT_URL, token, TIMEOUT_MS, {
    intent,
    elevation_ticket: options.elevationTicket,
  });
}

export async function createPlayer(
  form: PlayerForm,
  elevationTicket?: string,
): Promise<IntentOutcome> {
  const token = getToken();
  if (!token) return { kind: "signed_out" };
  return postIntent(PLAYERS_URL, token, TIMEOUT_MS * 3, {
    form,
    idempotency_key: newIdempotencyKey(),
    elevation_ticket: elevationTicket,
  });
}

async function sendPlayerForm(
  url: string,
  form: PlayerEditForm | PlayerPasswordForm,
  elevationTicket?: string,
): Promise<IntentOutcome> {
  const token = getToken();
  if (!token) return { kind: "signed_out" };
  return postIntent(url, token, TIMEOUT_MS * 3, {
    form,
    idempotency_key: newIdempotencyKey(),
    elevation_ticket: elevationTicket,
  });
}

export const editPlayer = (form: PlayerEditForm, ticket?: string) =>
  sendPlayerForm(PLAYER_UPDATE_URL, form, ticket);

export const resetPlayerPassword = (form: PlayerPasswordForm, ticket?: string) =>
  sendPlayerForm(PLAYER_PASSWORD_URL, form, ticket);

export type ReadOutcome<T> =
  | { kind: "ok"; value: T }
  | { kind: "signed_out" }
  | { kind: "error"; error: UiError | null }
  | { kind: "unreachable" };

async function getJson<T>(url: string): Promise<ReadOutcome<T>> {
  const token = getToken();
  if (!token) return { kind: "signed_out" };
  let res: Response;
  try {
    res = await fetch(url, {
      headers: authHeaders(token),
      signal: AbortSignal.timeout(TIMEOUT_MS),
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
      return { kind: "ok", value: (await res.json()) as T };
    } catch {
      return { kind: "unreachable" };
    }
  }
  const error = await errorOf(res);
  noticeLocked(error);
  return { kind: "error", error };
}

const withQuery = (url: string, params: Record<string, string>) => {
  const u = new URL(url);
  u.search = new URLSearchParams(params).toString();
  return u.href;
};

export const findPlayers = (q: string) => getJson<PlayerHit[]>(withQuery(PLAYERS_URL, { q }));

export const loadPlayer = (id: string) =>
  getJson<PlayerProfile>(withQuery(PLAYER_PROFILE_URL, { id }));

export const loadWallet = (id: string) => getJson<WalletView>(withQuery(PLAYER_WALLET_URL, { id }));

async function postIntent(
  url: string,
  token: string,
  timeoutMs: number,
  body: unknown,
): Promise<IntentOutcome> {
  let res: Response;
  try {
    res = await fetch(url, {
      method: "POST",
      headers: { ...authHeaders(token), "content-type": "application/json" },
      body: JSON.stringify(body),
      signal: AbortSignal.timeout(timeoutMs),
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
      return { kind: "applied", result: (await res.json()) as UiIntentResult };
    } catch {
      return { kind: "unreachable" };
    }
  }
  const error = await errorOf(res);
  noticeLocked(error);
  return error ? { kind: "error", error } : { kind: "unreachable" };
}

export type ElevateOutcome =
  | { kind: "ticket"; ticket: string; items: ElevationItem[]; expiresAt: number }
  | { kind: "rejected" }
  | { kind: "locked"; retryAfterS: number }
  | { kind: "signed_out" }
  | { kind: "error"; error: UiError }
  | { kind: "unreachable" };

/**
 * `POST /ui/elevate`: asks the Hub to check a manager's PIN for exactly `items` (the intents
 * the signed-in member is about to submit, or one `{permission}` read item: a read ticket for
 * the `X-Arena-Elevation` header, valid 10 minutes). The PIN goes to this one loopback call only and is
 * not kept here; the caller clears its own copy. A match answers a short-lived ticket for
 * `submitIntent`; `expiresAt` is when it lapses, on this clock.
 */
export async function elevate(items: ElevationItem[], pin: string): Promise<ElevateOutcome> {
  const token = getToken();
  if (!token) return { kind: "signed_out" };
  let res: Response;
  try {
    res = await fetch(ELEVATE_URL, {
      method: "POST",
      headers: { ...authHeaders(token), "content-type": "application/json" },
      body: JSON.stringify({ items, pin }),
      signal: AbortSignal.timeout(TIMEOUT_MS * 2),
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
      const body = (await res.json()) as Partial<ElevateOk>;
      if (
        typeof body.ticket === "string" &&
        Array.isArray(body.items) &&
        typeof body.expires_in_s === "number"
      ) {
        return {
          kind: "ticket",
          ticket: body.ticket,
          items: body.items,
          expiresAt: Date.now() + body.expires_in_s * 1000,
        };
      }
    } catch {
      // fall through
    }
    return { kind: "unreachable" };
  }
  const err = await errorOf(res);
  noticeLocked(err);
  if (err?.code === "AUTH_LOCKED") {
    const s = err.retry_after_s;
    return { kind: "locked", retryAfterS: Number.isFinite(s) ? Math.max(1, Math.ceil(s!)) : 60 };
  }
  if (err?.code === "PIN_REJECTED") return { kind: "rejected" };
  return {
    kind: "error",
    error: err ?? { code: "INTERNAL", message: "the hub answered " + res.status }, // i18n-ignore (not shown)
  };
}

/** `POST /ui/elevate/drop`: gives up a ticket's unused approvals (best effort). */
export async function dropElevation(ticket: string): Promise<void> {
  const token = getToken();
  if (!token) return;
  try {
    await fetch(ELEVATE_DROP_URL, {
      method: "POST",
      headers: { ...authHeaders(token), "content-type": "application/json" },
      body: JSON.stringify({ ticket }),
      signal: AbortSignal.timeout(TIMEOUT_MS),
    });
  } catch {
    // The ticket lapses within a minute anyway.
  }
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

/** What one `POST /ui/lock` came to. */
export type LockResult = "locked" | "signed_out" | "failed";

/** `POST /ui/lock`: the Hub refuses this session's intents until it is unlocked. */
export async function lockSession(): Promise<LockResult> {
  const token = getToken();
  if (!token) return "signed_out";
  try {
    const res = await fetch(LOCK_URL, {
      method: "POST",
      headers: authHeaders(token),
      signal: AbortSignal.timeout(TIMEOUT_MS),
    });
    if (res.status === 401) {
      setToken(null);
      return "signed_out";
    }
    return res.ok ? "locked" : "failed";
  } catch {
    return "failed";
  }
}

export type UnlockOutcome =
  | { kind: "unlocked"; staff: SessionStaff; switched: boolean }
  | { kind: "rejected" }
  | { kind: "locked"; retryAfterS: number }
  | { kind: "signed_out" }
  /** The Hub does not hold this session locked (its lock call has not landed). */
  | { kind: "not_locked" }
  | { kind: "error"; error: UiError }
  | { kind: "unreachable" };

/**
 * `POST /ui/unlock`: the Hub checks the PIN against the signed-in member, then the other staff.
 * Another member's PIN re-binds this session to them (`switched`). The PIN goes to this one
 * loopback call only; the caller clears its own copy.
 */
export async function unlockSession(pin: string): Promise<UnlockOutcome> {
  const token = getToken();
  if (!token) return { kind: "signed_out" };
  let res: Response;
  try {
    res = await fetch(UNLOCK_URL, {
      method: "POST",
      headers: { ...authHeaders(token), "content-type": "application/json" },
      body: JSON.stringify({ pin }),
      signal: AbortSignal.timeout(TIMEOUT_MS * 2),
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
      const body = (await res.json()) as Partial<UnlockOk>;
      if (isSessionStaff(body.staff) && typeof body.switched === "boolean") {
        return { kind: "unlocked", staff: body.staff, switched: body.switched };
      }
    } catch {
      // fall through
    }
    return { kind: "unreachable" };
  }
  const err = await errorOf(res);
  if (err?.code === "AUTH_LOCKED") {
    const s = err.retry_after_s;
    return { kind: "locked", retryAfterS: Number.isFinite(s) ? Math.max(1, Math.ceil(s!)) : 60 };
  }
  if (err?.code === "PIN_REJECTED") return { kind: "rejected" };
  if (err?.code === "VALIDATION_FAILED" && err.field === "session") return { kind: "not_locked" };
  return {
    kind: "error",
    error: err ?? { code: "INTERNAL", message: "the hub answered " + res.status }, // i18n-ignore (not shown)
  };
}
