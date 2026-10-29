// The extend panel's copy (Story 6.21): the steps, the sources and the line a refusal becomes.

import { formatMoney, t } from "@arena/ui";
import type { PlayerExtendOptions, SnapshotLocale } from "@arena/ui/generated";
import type { ExtendSourceChoice } from "./extend";
import { durationText } from "./plans";

/** "+30 min", "+1 h", "+2 h". */
export const stepText = (minutes: number): string =>
  minutes < 60
    ? t("player.extend.step_min", { n: minutes })
    : t("player.extend.step_hour", { n: minutes / 60 });

/** A source's line: "Wallet · ₹40.00", "10 hours · 1h 30m left" or "Credit · ₹200.00 left". */
export function sourceText(
  choice: ExtendSourceChoice,
  options: PlayerExtendOptions,
  locale: SnapshotLocale,
): string {
  if (choice.source === "wallet") {
    return t("player.extend.source_wallet", { balance: formatMoney(options.wallet_minor, locale) });
  }
  if (choice.source === "credit") {
    return t("player.extend.source_credit", {
      amount: formatMoney(options.credit_headroom_minor ?? 0, locale),
    });
  }
  const pack = options.packs.find((p) => p.pack_id === choice.packId);
  return t("player.extend.source_pack", {
    name: pack?.name ?? "",
    time: durationText(pack?.minutes_left ?? 0),
  });
}

/**
 * The line a refusal becomes. The panel stays open and the other options stay usable; there is no
 * manager override here, so a limit or a short balance only ever points at the counter.
 */
export function refusalText(
  code: string,
  choice: ExtendSourceChoice,
  options: PlayerExtendOptions,
  locale: SnapshotLocale,
): string {
  switch (code) {
    case "PAST_CLOSING":
      return t("player.extend.past_closing", { closing: options.closing_label ?? "" });
    case "CREDIT_LIMIT":
      return t("player.credit.limit_reached");
    case "CREDIT_PAUSED":
      return t("player.extend.credit_paused");
    case "INSUFFICIENT_BALANCE":
      if (choice.source === "pack") {
        const pack = options.packs.find((p) => p.pack_id === choice.packId);
        return t("player.extend.pack_short", { time: durationText(pack?.minutes_left ?? 0) });
      }
      return t("player.balance.insufficient", {
        balance: formatMoney(options.wallet_minor, locale),
      });
    default:
      return t("player.extend.failed");
  }
}
