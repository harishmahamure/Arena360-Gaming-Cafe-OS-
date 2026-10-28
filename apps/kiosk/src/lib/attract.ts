// The attract screen's rules (Story 6.7): how long each curated video plays, the Hub media URL of
// an item, the rate and the rule chip as text, and when the login panel closes. Pure, so the
// screen and its tests share them.

import { formatMoney, formatTime, t } from "@arena/ui";
import type {
  AttractMedia,
  AttractView,
  SnapshotActiveRule,
  SnapshotLocale,
} from "@arena/ui/generated";

/** Each curated video plays this long, then the next one (a single item keeps playing). */
export const ATTRACT_VIDEO_MS = 30_000;
/** The login panel closes after this long without input. */
export const LOGIN_IDLE_MS = 60_000;

/** The Hub media URL of a stored file, by content hash (relative to the page). */
export const mediaUrl = (sha256: string): string => `/media/${sha256}`;

/** The index that follows `index` in a cycle of `count` items (a single item stays at 0). */
export function nextMedia(index: number, count: number): number {
  return count <= 1 ? 0 : (index + 1) % count;
}

/** The item to show for `index` (wrapped into range), or `null` when there is none. */
export function mediaAt(media: readonly AttractMedia[], index: number): AttractMedia | null {
  return media.length === 0 ? null : media[((index % media.length) + media.length) % media.length];
}

/** Whether the screen offers the login panel: the PC is ready, with a rate to show. */
export const canLogIn = (view: AttractView | null): boolean =>
  view !== null && view.state === "ready" && view.rate_minor != null;

/** "₹120.00 an hour" in the branch locale, or `null` when the PC shows no rate. */
export function rateText(view: AttractView | null): string | null {
  if (view === null || view.state !== "ready" || view.rate_minor == null) return null;
  return t("player.attract.rate", { rate: formatMoney(view.rate_minor, view.locale) });
}

/** A signed percentage, e.g. `−20%` or `+12.5%`. */
export function percentText(bp: number): string {
  const value = Math.abs(bp) / 100;
  const digits = Number.isInteger(value) ? String(value) : value.toFixed(2).replace(/0$/, "");
  return `${bp < 0 ? "−" : "+"}${digits}%`; // i18n-ignore (a signed number)
}

/** "Happy hour −20% · until 6:00 pm": the rule's name, what it does and when it ends. */
export function ruleChipText(rule: SnapshotActiveRule, locale: SnapshotLocale): string {
  const change =
    rule.adjust_kind === "percent_bp"
      ? percentText(rule.adjust_value)
      : t("player.attract.rule_rate", { rate: formatMoney(rule.adjust_value, locale) });
  if (rule.until == null) return t("player.attract.rule_open", { name: rule.name, change });
  return t("player.attract.rule_until", {
    name: rule.name,
    change,
    time: formatTime(rule.until, locale.tz),
  });
}
