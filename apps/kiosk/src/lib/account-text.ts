// The account menu's copy for what the player holds (Story 6.17).

import { formatDateTime, formatMoney, t } from "@arena/ui";
import type { AccountPack, SnapshotLocale } from "@arena/ui/generated";
import { durationText } from "./plans";

/** "1h 42m": the time left in whole minutes, rounded up. */
export const timeLeftText = (seconds: number): string => durationText(Math.ceil(seconds / 60));

/** One line of the Balance view: a pack's hours left, or a pass's last day. */
export function packLine(pack: AccountPack, locale: SnapshotLocale): string {
  if (pack.minutes_left != null) {
    return t("player.account.pack_hours", {
      name: pack.name,
      time: durationText(pack.minutes_left),
    });
  }
  return t("player.account.pass_until", {
    name: pack.name,
    date: formatDateTime(pack.expires_at, locale.tz, { dateStyle: "medium" }),
  });
}

/** The wallet balance in the branch's money format. */
export const balanceText = (minor: number, locale: SnapshotLocale): string =>
  formatMoney(minor, locale);
