// The extend panel's rules (Story 6.21): how the service's answers read and which payment sources
// the panel lists. Pure and free of the UI package, so the tests run it in Node. The copy is in
// `extend-text.ts`.
//
// The Hub decides everything: it quotes the three steps at the live rate, knows what the player
// holds and applies (or refuses) the extend by the Story 3.28 rules. The kiosk names no session
// and no player, keeps nothing on disk, and has no manager override.

import type { ExtendSource, PlayerExtendOptions } from "@arena/ui/generated";

/** The steps a player can add, in minutes: the same three the Hub quotes and accepts. */
export const EXTEND_STEPS = [30, 60, 120] as const;

/** What asking for the options came to. */
export type ExtendOptionsOutcome =
  | { kind: "ok"; options: PlayerExtendOptions }
  /** The Hub did not answer: every option is disabled with "Counter is reconnecting". */
  | { kind: "unreachable" }
  /** The Hub answered that this session cannot be extended (a pass, a block, a guest). */
  | { kind: "unavailable" };

/** What an extend came to. */
export type ExtendOutcome =
  { kind: "ok" } | { kind: "refused"; code: string } | { kind: "unreachable" };

/** One way to pay for the extra time, as the panel lists it. */
export type ExtendSourceChoice = {
  /** A unique key: `wallet`, `credit` or `pack:<id>`. */
  key: string;
  source: ExtendSource;
  packId?: string;
};

const isObject = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null;

const isQuote = (v: unknown): boolean =>
  isObject(v) && typeof v.minutes === "number" && typeof v.price_minor === "number";

const isPack = (v: unknown): boolean =>
  isObject(v) &&
  typeof v.pack_id === "string" &&
  typeof v.name === "string" &&
  typeof v.minutes_left === "number";

/** The Hub link is down or slow rather than the Hub saying no. */
const isLinkError = (answer: Record<string, unknown>): boolean => answer.code === "HUB_SLOW";

/** The options in the service's answer to `player_extend_options` (`player.extend_options_result`). */
export function extendOptionsOf(answer: unknown): ExtendOptionsOutcome {
  if (!isObject(answer) || typeof answer.ok !== "boolean") return { kind: "unreachable" };
  if (!answer.ok) return isLinkError(answer) ? { kind: "unreachable" } : { kind: "unavailable" };
  const body = answer.body;
  if (
    !isObject(body) ||
    !Array.isArray(body.quotes) ||
    !body.quotes.every(isQuote) ||
    typeof body.wallet_minor !== "number" ||
    !Array.isArray(body.packs) ||
    !body.packs.every(isPack)
  ) {
    return { kind: "unreachable" };
  }
  return { kind: "ok", options: body as unknown as PlayerExtendOptions };
}

/** The outcome in the service's answer to `player_extend` (`player.extend_result`). */
export function extendOf(answer: unknown): ExtendOutcome {
  if (!isObject(answer) || typeof answer.ok !== "boolean") return { kind: "unreachable" };
  if (!answer.ok) {
    return isLinkError(answer)
      ? { kind: "unreachable" }
      : { kind: "refused", code: typeof answer.code === "string" ? answer.code : "" };
  }
  const body = answer.body;
  if (!isObject(body)) return { kind: "unreachable" };
  if (body.outcome === "ok") return { kind: "ok" };
  if (body.outcome === "refused") {
    return { kind: "refused", code: typeof body.code === "string" ? body.code : "" };
  }
  return { kind: "unreachable" };
}

/** The sources the player has: the wallet, each pack valid for this PC, then credit if enabled. */
export function sourcesOf(options: PlayerExtendOptions): ExtendSourceChoice[] {
  const sources: ExtendSourceChoice[] = [{ key: "wallet", source: "wallet" }];
  for (const p of options.packs) {
    sources.push({ key: `pack:${p.pack_id}`, source: "pack", packId: p.pack_id }); // i18n-ignore (a key)
  }
  if (options.credit_headroom_minor != null) sources.push({ key: "credit", source: "credit" });
  return sources;
}
