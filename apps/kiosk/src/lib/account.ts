// The account menu's rules (Story 6.17): how the service's answers read and the lock screen's own
// count of wrong passwords. Pure (and free of the UI package, so the tests run it in Node) apart
// from the module-level lock counter. The copy is in `account-text.ts`.
//
// The Hub decides everything: it knows who is on this PC's session, what they hold, whether the
// password is right and when a pause starts. The kiosk names nobody and keeps nothing on disk.

import type { AccountPack } from "@arena/ui/generated";

/** What asking for the account came to. */
export type AccountOutcome =
  { kind: "ok"; balanceMinor: number; packs: AccountPack[] } | { kind: "unreachable" };

/** What Log out came to. */
export type LogoutOutcome =
  { kind: "ok" } | { kind: "refused"; message: string } | { kind: "unreachable" };

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
