// The player login's rules (Story 6.8): how the service's answer reads, the kiosk's own count of
// wrong tries, the lock countdown as text, and how long the holding step waits. Pure apart from
// the two module-level counters, so the form and its tests share them.
//
// The Hub is the authority: it counts wrong tries per kiosk and answers `locked{retry_s}` on the
// fifth. The kiosk keeps its own count of the answers it saw as well (so a Hub that did not
// pause still cannot be hammered from this screen); the countdown always follows `retry_s`.

import type { PlayerContext } from "@arena/ui/generated";

/** Wrong tries in a row after which this screen pauses itself. */
export const LOGIN_MAX_FAILURES = 5;
/** How long the form stays disabled after the fifth wrong try. */
export const LOGIN_LOCK_S = 60;
/** How long the app waits for the service's answer before it calls the counter unreachable. */
export const LOGIN_TIMEOUT_MS = 10_000;

/** What a login attempt came to. */
export type LoginOutcome =
  | { kind: "ok"; player: PlayerContext }
  | { kind: "rejected" }
  | { kind: "locked"; retryS: number }
  | { kind: "busy"; deviceName: string }
  | { kind: "unreachable" };

const isObject = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null;

const isPlayer = (v: unknown): v is PlayerContext =>
  isObject(v) && typeof v.player_id === "string" && typeof v.name === "string";

/**
 * The outcome in the service's answer to `player_login`: `{ok, code?, body}` where an ok body
 * is the Hub's `player.auth_result`. An error, a missing answer or a body that is not one of
 * the four outcomes is `unreachable`.
 */
export function outcomeOf(answer: unknown): LoginOutcome {
  const unreachable: LoginOutcome = { kind: "unreachable" };
  if (!isObject(answer) || answer.ok !== true || !isObject(answer.body)) return unreachable;
  const body = answer.body;
  switch (body.outcome) {
    case "ok":
      return isPlayer(body.player) ? { kind: "ok", player: body.player } : unreachable;
    case "rejected":
      return { kind: "rejected" };
    case "locked":
      return typeof body.retry_s === "number" && Number.isFinite(body.retry_s)
        ? { kind: "locked", retryS: Math.min(Math.max(1, Math.ceil(body.retry_s)), 3600) }
        : { kind: "locked", retryS: LOGIN_LOCK_S };
    case "busy":
      return typeof body.device_name === "string"
        ? { kind: "busy", deviceName: body.device_name }
        : unreachable;
    default:
      return unreachable;
  }
}

/** "0:60", "0:09": the lock countdown in tabular figures (minutes only past a minute). */
export function countdownText(seconds: number): string {
  const s = Math.max(0, Math.ceil(seconds));
  if (s <= LOGIN_LOCK_S) return `0:${String(s).padStart(2, "0")}`; // i18n-ignore (a clock)
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`; // i18n-ignore (a clock)
}

let failures = 0;
let lockedUntil: number | null = null;

/** Whole seconds of lock left at `now` (a `performance.now()` value); 0 when open. */
export function lockLeftS(now: number): number {
  if (lockedUntil === null) return 0;
  if (now >= lockedUntil) {
    // The pause is over: the count starts again.
    lockedUntil = null;
    failures = 0;
    return 0;
  }
  return Math.ceil((lockedUntil - now) / 1000);
}

/** Takes one answer into account: the fifth wrong try in a row, or the Hub's `locked`, locks. */
export function recordOutcome(outcome: LoginOutcome, now: number): void {
  switch (outcome.kind) {
    case "ok":
      failures = 0;
      lockedUntil = null;
      break;
    case "busy":
      // The password was right, so the Hub clears its count; so does this screen.
      failures = 0;
      break;
    case "rejected":
      failures += 1;
      if (failures >= LOGIN_MAX_FAILURES) {
        failures = 0;
        lockedUntil = now + LOGIN_LOCK_S * 1000;
      }
      break;
    case "locked":
      failures = 0;
      lockedUntil = now + outcome.retryS * 1000;
      break;
    default:
      // Unreachable is not a wrong try.
      break;
  }
}
