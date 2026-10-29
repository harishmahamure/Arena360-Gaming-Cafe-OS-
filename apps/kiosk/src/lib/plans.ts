// The plan picker's rules (Story 6.9): how the service's answers read, the card text, the refusal
// copy and how long the picker waits. Pure, so the picker and its tests share them.
//
// The Hub decides everything: it lists the options valid for this PC's group, starts the session
// and refuses with a typed code. The kiosk never names a player, never sends postpaid and holds
// no player context on disk.

import { formatDateTime, formatMoney, t } from "@arena/ui";
import type {
  ErrorCode,
  PlanOption,
  PlayerChoice,
  PlayerPlansResult,
  SnapshotLocale,
} from "@arena/ui/generated";

/** The picker forgets the player after this long without input. */
export const PICKER_IDLE_MS = 60_000;
/** After the Hub accepted a start, how long the picker waits for the session screen. */
export const START_WAIT_MS = 10_000;
/** How long the app waits for the service's answer before it calls the counter unreachable. */
export const PICKER_TIMEOUT_MS = 10_000;

/** What asking for the options came to. */
export type PlansOutcome =
  { kind: "ok"; result: PlayerPlansResult } | { kind: "login" } | { kind: "unreachable" };

/** What a start came to. */
export type StartOutcome =
  | { kind: "ok" }
  | { kind: "refused"; code: ErrorCode; message: string }
  | { kind: "login" }
  | { kind: "unreachable" };

const isObject = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null;

const isChoice = (v: unknown): v is PlayerChoice => {
  if (!isObject(v)) return false;
  switch (v.mode) {
    case "pack":
    case "pass":
      return typeof v.pack_id === "string";
    case "block":
      return typeof v.plan_id === "string";
    case "wallet":
      return true;
    default:
      return false;
  }
};

const isOption = (v: unknown): v is PlanOption =>
  isObject(v) &&
  isChoice(v.choice) &&
  typeof v.name === "string" &&
  typeof v.selectable === "boolean";

const isWallet = (v: unknown): boolean =>
  isObject(v) && typeof v.balance_minor === "number" && typeof v.rate_minor === "number";

/** `AUTH_FAILED` is the Hub saying it no longer holds this PC's login (it is the only source). */
const isLoginGone = (answer: Record<string, unknown>): boolean => answer.code === "AUTH_FAILED";

/**
 * The options in the service's answer to `player_plans`: `{ok, code?, body}` where an ok body is
 * the Hub's `player.plans_result`. `AUTH_FAILED` means log in again; any other error, a missing
 * answer or a body that is not a result is `unreachable`.
 */
export function plansOf(answer: unknown): PlansOutcome {
  const unreachable: PlansOutcome = { kind: "unreachable" };
  if (!isObject(answer)) return unreachable;
  if (answer.ok !== true) return isLoginGone(answer) ? { kind: "login" } : unreachable;
  const body = answer.body;
  if (!isObject(body) || !Array.isArray(body.options) || !body.options.every(isOption)) {
    return unreachable;
  }
  if (body.wallet != null && !isWallet(body.wallet)) return unreachable;
  return { kind: "ok", result: body as unknown as PlayerPlansResult };
}

/** The outcome in the service's answer to `player_start` (an ok body is `player.start_result`). */
export function startOf(answer: unknown): StartOutcome {
  const unreachable: StartOutcome = { kind: "unreachable" };
  if (!isObject(answer)) return unreachable;
  if (answer.ok !== true) return isLoginGone(answer) ? { kind: "login" } : unreachable;
  const body = answer.body;
  if (!isObject(body)) return unreachable;
  if (body.outcome === "ok") return { kind: "ok" };
  if (body.outcome === "refused" && typeof body.code === "string") {
    return {
      kind: "refused",
      code: body.code as ErrorCode,
      message: typeof body.message === "string" ? body.message : "",
    };
  }
  return unreachable;
}

/** Whether two choices are the same option. */
export function sameChoice(a: PlayerChoice, b: PlayerChoice): boolean {
  if (a.mode !== b.mode) return false;
  switch (a.mode) {
    case "pack":
    case "pass":
      return a.pack_id === (b as typeof a).pack_id;
    case "block":
      return a.plan_id === (b as typeof a).plan_id;
    default:
      return true;
  }
}

/** "0h 45m": whole minutes as hours and minutes. */
export function durationText(minutes: number): string {
  const m = Math.max(0, Math.floor(minutes));
  return t("player.picker.duration", { hours: Math.floor(m / 60), minutes: m % 60 });
}

/** The name on a card and in "Starting …": the plan's, or "Wallet". */
export const optionName = (o: PlanOption): string =>
  o.choice.mode === "wallet" || o.name === "" ? t("player.picker.kind.wallet") : o.name;

/** The small label above the name: Pack, Pass, Block or Wallet. */
export function kindLabel(o: PlanOption): string {
  switch (o.choice.mode) {
    case "pack":
      return t("player.picker.kind.pack");
    case "pass":
      return t("player.picker.kind.pass");
    case "block":
      return t("player.picker.kind.block");
    default:
      return t("player.picker.kind.wallet");
  }
}

/** The tag on an option that cannot be chosen now. */
export const tagLabel = (o: PlanOption): string | null =>
  o.tag === "expired"
    ? t("player.picker.tag.expired")
    : o.tag === "outside_window"
      ? t("player.picker.tag.outside_window")
      : null;

/** The lines under the name of a card, in order. */
export function detailLines(
  o: PlanOption,
  wallet: PlayerPlansResult["wallet"],
  locale: SnapshotLocale,
): string[] {
  const lines: string[] = [];
  const expires =
    o.expires_at == null
      ? null
      : t("player.picker.expires", {
          date: formatDateTime(o.expires_at, locale.tz, { dateStyle: "medium" }),
        });
  switch (o.choice.mode) {
    case "pack":
      if (o.minutes_left != null) {
        lines.push(
          t(o.tag === "expired" ? "player.picker.unused" : "player.picker.time_left", {
            time: durationText(o.minutes_left),
          }),
        );
      }
      if (expires) lines.push(expires);
      break;
    case "pass":
      if (o.window) lines.push(t("player.picker.window", { window: o.window }));
      if (expires) lines.push(expires);
      break;
    case "block":
      if (o.price_minor != null) lines.push(formatMoney(o.price_minor, locale));
      if (o.ends_label) lines.push(t("player.picker.block_ends", { time: o.ends_label }));
      else if (o.window) lines.push(t("player.picker.window", { window: o.window }));
      break;
    case "wallet":
      if (wallet) {
        lines.push(
          t("player.picker.balance", { balance: formatMoney(wallet.balance_minor, locale) }),
        );
        lines.push(t("player.attract.rate", { rate: formatMoney(wallet.rate_minor, locale) }));
      }
      break;
  }
  return lines;
}

/**
 * The catalogue line for a refusal. `INSUFFICIENT_BALANCE` names the balance from the options
 * last shown ("Balance ₹40. Top up at counter."); the rest read from their code.
 */
export function refusalText(
  code: ErrorCode,
  wallet: PlayerPlansResult["wallet"],
  locale: SnapshotLocale,
): string {
  switch (code) {
    case "INSUFFICIENT_BALANCE":
      return t("player.balance.insufficient", {
        balance: formatMoney(wallet?.balance_minor ?? 0, locale),
      });
    case "PACK_EXPIRED":
      return t("player.picker.refused.pack_expired");
    case "PLAN_NOT_FOR_GROUP":
      return t("player.picker.refused.plan_not_for_group");
    case "PASS_OUTSIDE_WINDOW":
      return t("player.picker.refused.pass_outside_window");
    case "PC_BUSY":
      return t("player.picker.refused.pc_busy");
    default:
      return t("player.picker.refused.other");
  }
}

/** The index of the next selectable option from `from` in direction `step` (wrapping), or -1. */
export function nextSelectable(options: readonly PlanOption[], from: number, step: 1 | -1): number {
  const n = options.length;
  for (let i = 1; i <= n; i++) {
    const at = (((from + step * i) % n) + n) % n;
    if (options[at].selectable) return at;
  }
  return -1;
}
