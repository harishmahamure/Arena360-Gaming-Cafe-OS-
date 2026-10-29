// The account menu's copy for what the player holds (Story 6.17).

import { formatDateTime, formatMoney, t } from "@arena/ui";
import type {
  AccountOrder,
  AccountPack,
  AccountPackEntry,
  SnapshotLocale,
} from "@arena/ui/generated";
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

/** A time of day from minutes since midnight: "18:00". */
export const clockText = (minutes: number): string => {
  const m = ((Math.floor(minutes) % 1440) + 1440) % 1440;
  return `${String(Math.floor(m / 60)).padStart(2, "0")}:${String(m % 60).padStart(2, "0")}`; // i18n-ignore (a clock)
};

/**
 * The lines under a pack or pass on the Wallet & packs tab (Story 6.34): where it is valid, its
 * hours left (or unused once expired) or daily window, and when it expires.
 */
export function packLines(pack: AccountPackEntry, locale: SnapshotLocale): string[] {
  const date = formatDateTime(pack.expires_at, locale.tz, { dateStyle: "medium" });
  const lines: string[] = [];
  if (pack.groups.length > 0) {
    lines.push(t("player.account.packs.valid_on", { groups: pack.groups.join(", ") }));
  }
  if (pack.minutes_left != null) {
    lines.push(
      t(pack.expired ? "player.account.packs.unused" : "player.account.packs.left", {
        time: durationText(pack.minutes_left),
      }),
    );
  } else if (pack.window_min) {
    lines.push(
      t("player.account.packs.window", {
        from: clockText(pack.window_min[0]),
        to: clockText(pack.window_min[1]),
      }),
    );
  }
  if (!pack.expired) lines.push(t("player.account.packs.expires", { date }));
  return lines;
}

/** "2× Cola, Chips": the order's items. */
export const orderItemsText = (order: AccountOrder): string =>
  order.items
    .map((i) =>
      i.qty > 1 ? t("player.account.orders.item", { qty: i.qty, name: i.name }) : i.name,
    )
    .join(", ");

/** The order's status as the player reads it. */
export function orderStatusText(status: string): string {
  switch (status) {
    case "settled":
      return t("player.account.orders.status.settled");
    case "void":
      return t("player.account.orders.status.void");
    default:
      return t("player.account.orders.status.open");
  }
}

/** The wallet balance in the branch's money format. */
export const balanceText = (minor: number, locale: SnapshotLocale): string =>
  formatMoney(minor, locale);
