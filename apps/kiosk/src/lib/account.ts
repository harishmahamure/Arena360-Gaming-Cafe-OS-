// The account menu's rules (Story 6.17): how the service's answers read and the lock screen's own
// count of wrong passwords. Pure (and free of the UI package, so the tests run it in Node) apart
// from the module-level lock counter. The copy is in `account-text.ts`.
//
// The Hub decides everything: it knows who is on this PC's session, what they hold, whether the
// password is right and when a pause starts. The kiosk names nobody and keeps nothing on disk.

import type {
  AccountOrder,
  AccountPack,
  AccountPackEntry,
  PlayerAccountPage,
} from "@arena/ui/generated";

/** What asking for the account came to. */
export type AccountOutcome =
  { kind: "ok"; balanceMinor: number; packs: AccountPack[] } | { kind: "unreachable" };

/** What Log out came to. */
export type LogoutOutcome =
  { kind: "ok" } | { kind: "refused"; message: string } | { kind: "unreachable" };

/** What asking for the Account page came to (Story 6.34). */
export type AccountPageOutcome = { kind: "ok"; page: PlayerAccountPage } | { kind: "unreachable" };

/** What the Change password form came to (Story 6.34). */
export type PasswordChangeOutcome =
  | { kind: "ok" }
  | { kind: "wrong_current" }
  | { kind: "invalid_new" }
  | { kind: "locked"; retryS: number }
  | { kind: "refused"; message: string }
  | { kind: "unreachable" };

/** The fewest characters a new password has. */
export const PASSWORD_MIN_CHARS = 6;
/** The most characters a password has. */
export const PASSWORD_MAX_CHARS = 128;

/** What a password on the lock screen came to. */
export type UnlockOutcome =
  | { kind: "ok" }
  | { kind: "rejected" }
  | { kind: "locked"; retryS: number }
  | { kind: "unreachable" };

/** Wrong passwords in a row after which the lock screen pauses itself. */
export const UNLOCK_MAX_FAILURES = 5;
/** How long the field stays disabled after the fifth wrong password. */
export const UNLOCK_LOCK_S = 60;

const isObject = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null;

const isPack = (v: unknown): v is AccountPack =>
  isObject(v) &&
  typeof v.name === "string" &&
  typeof v.expires_at === "number" &&
  (v.minutes_left === undefined || v.minutes_left === null || typeof v.minutes_left === "number");

/** The account in the service's answer to `player_account` (an ok body is `player.account_result`). */
export function accountOf(answer: unknown): AccountOutcome {
  const unreachable: AccountOutcome = { kind: "unreachable" };
  if (!isObject(answer) || answer.ok !== true || !isObject(answer.body)) return unreachable;
  const { balance_minor: balance, packs } = answer.body;
  if (typeof balance !== "number" || !Array.isArray(packs) || !packs.every(isPack)) {
    return unreachable;
  }
  return { kind: "ok", balanceMinor: balance, packs };
}

const isPackEntry = (v: unknown): v is AccountPackEntry =>
  isObject(v) &&
  typeof v.name === "string" &&
  Array.isArray(v.groups) &&
  v.groups.every((g) => typeof g === "string") &&
  typeof v.expires_at === "number" &&
  typeof v.expired === "boolean" &&
  (v.minutes_left === undefined || v.minutes_left === null || typeof v.minutes_left === "number") &&
  (v.window_min === undefined ||
    v.window_min === null ||
    (Array.isArray(v.window_min) &&
      v.window_min.length === 2 &&
      v.window_min.every((m) => typeof m === "number")));

const isOrder = (v: unknown): v is AccountOrder =>
  isObject(v) &&
  typeof v.order_id === "string" &&
  typeof v.at === "number" &&
  typeof v.total_minor === "number" &&
  typeof v.status === "string" &&
  (v.invoice_no === undefined || v.invoice_no === null || typeof v.invoice_no === "string") &&
  Array.isArray(v.items) &&
  v.items.every((i) => isObject(i) && typeof i.name === "string" && typeof i.qty === "number");

/** The page in the service's answer to `player_account_page` (an ok body is `player.account_page_result`). */
export function accountPageOf(answer: unknown): AccountPageOutcome {
  const unreachable: AccountPageOutcome = { kind: "unreachable" };
  if (!isObject(answer) || answer.ok !== true || !isObject(answer.body)) return unreachable;
  const b = answer.body;
  if (
    typeof b.name !== "string" ||
    typeof b.username !== "string" ||
    typeof b.phone_masked !== "string" ||
    typeof b.balance_minor !== "number" ||
    !Array.isArray(b.packs) ||
    !b.packs.every(isPackEntry) ||
    !Array.isArray(b.orders) ||
    !b.orders.every(isOrder)
  ) {
    return unreachable;
  }
  return {
    kind: "ok",
    page: {
      name: b.name,
      username: b.username,
      phone_masked: b.phone_masked,
      balance_minor: b.balance_minor,
      packs: b.packs,
      orders: b.orders,
    },
  };
}

/** The outcome in the service's answer to `player_password_change` (an ok body is `player.password_change_result`). */
export function passwordChangeOf(answer: unknown): PasswordChangeOutcome {
  const unreachable: PasswordChangeOutcome = { kind: "unreachable" };
  if (!isObject(answer) || answer.ok !== true || !isObject(answer.body)) return unreachable;
  const body = answer.body;
  switch (body.outcome) {
    case "ok":
      return { kind: "ok" };
    case "wrong_current":
      return { kind: "wrong_current" };
    case "invalid_new":
      return { kind: "invalid_new" };
    case "locked":
      return typeof body.retry_s === "number" && Number.isFinite(body.retry_s)
        ? { kind: "locked", retryS: Math.min(Math.max(1, Math.ceil(body.retry_s)), 3600) }
        : { kind: "locked", retryS: UNLOCK_LOCK_S };
    case "refused":
      return { kind: "refused", message: typeof body.message === "string" ? body.message : "" };
    default:
      return unreachable;
  }
}

/** Whether `password` is long enough and not too long to be a new password. */
export const validNewPassword = (password: string): boolean => {
  const n = [...password].length;
  return n >= PASSWORD_MIN_CHARS && n <= PASSWORD_MAX_CHARS;
};

/** The outcome in the service's answer to `player_end` (an ok body is `player.end_result`). */
export function endOf(answer: unknown): LogoutOutcome {
  const unreachable: LogoutOutcome = { kind: "unreachable" };
  if (!isObject(answer) || answer.ok !== true || !isObject(answer.body)) return unreachable;
  const body = answer.body;
  if (body.outcome === "ok") return { kind: "ok" };
  if (body.outcome === "refused") {
    return { kind: "refused", message: typeof body.message === "string" ? body.message : "" };
  }
  return unreachable;
}

/** The outcome in the service's answer to `player_unlock` (an ok body is `player.unlock_result`). */
export function unlockOf(answer: unknown): UnlockOutcome {
  const unreachable: UnlockOutcome = { kind: "unreachable" };
  if (!isObject(answer) || answer.ok !== true || !isObject(answer.body)) return unreachable;
  const body = answer.body;
  switch (body.outcome) {
    case "ok":
      return { kind: "ok" };
    case "rejected":
      return { kind: "rejected" };
    case "locked":
      return typeof body.retry_s === "number" && Number.isFinite(body.retry_s)
        ? { kind: "locked", retryS: Math.min(Math.max(1, Math.ceil(body.retry_s)), 3600) }
        : { kind: "locked", retryS: UNLOCK_LOCK_S };
    default:
      return unreachable;
  }
}

let failures = 0;
let lockedUntil: number | null = null;
let passwordLockedUntil: number | null = null;

/** Whole seconds the Change password form is paused for at `now`; 0 when open. */
export function passwordLeftS(now: number): number {
  if (passwordLockedUntil === null) return 0;
  if (now >= passwordLockedUntil) {
    passwordLockedUntil = null;
    return 0;
  }
  return Math.ceil((passwordLockedUntil - now) / 1000);
}

/** Takes one answer into account: the Hub pauses the form on the fifth wrong current password. */
export function recordPasswordChange(outcome: PasswordChangeOutcome, now: number): void {
  if (outcome.kind === "locked") passwordLockedUntil = now + outcome.retryS * 1000;
  else if (outcome.kind === "ok") passwordLockedUntil = null;
}

/** Whole seconds of pause left at `now` (a `performance.now()` value); 0 when open. */
export function unlockLeftS(now: number): number {
  if (lockedUntil === null) return 0;
  if (now >= lockedUntil) {
    lockedUntil = null;
    failures = 0;
    return 0;
  }
  return Math.ceil((lockedUntil - now) / 1000);
}

/** Takes one answer into account: the fifth wrong password in a row, or the Hub's `locked`, pauses. */
export function recordUnlock(outcome: UnlockOutcome, now: number): void {
  switch (outcome.kind) {
    case "ok":
      failures = 0;
      lockedUntil = null;
      break;
    case "rejected":
      failures += 1;
      if (failures >= UNLOCK_MAX_FAILURES) {
        failures = 0;
        lockedUntil = now + UNLOCK_LOCK_S * 1000;
      }
      break;
    case "locked":
      failures = 0;
      lockedUntil = now + outcome.retryS * 1000;
      break;
    default:
      // Unreachable is not a wrong password.
      break;
  }
}
