// The player shell's rules (Story 6.13): who the account chip names, what the time chip reads
// and when it warns. Pure, so the shell and its tests share them.

import { t } from "@arena/ui";

/** The pill nav items, in order. Shop (Story 6.30) is shown only when the Hub says orders are on. */
export const TABS = ["home", "games", "apps", "shop"] as const;
export type Tab = (typeof TABS)[number];

/** The items the pill nav shows: Shop after Apps on a branch with the kiosk-orders feature only. */
export const tabsFor = (shop: boolean): Tab[] => TABS.filter((id) => shop || id !== "shop");

/** At or under this many seconds left the chip shows its warning border. */
export const LOW_SECONDS = 10 * 60;

export type ShellSession = {
  /** The PC's name. */
  device: string;
  username: string | null;
  guestNo: number | null;
  /** The clock counts down (prepaid) rather than up (postpaid). */
  prepaid: boolean;
  /** The branch's own warning mark that has been reached, in minutes (Story 3.23). */
  warningMin: number | null;
};

/** The profiled player's username, or "Guest <n>". */
export function playerName(s: Pick<ShellSession, "username" | "guestNo">): string {
  if (s.username) return s.username;
  return t("player.shell.guest", { n: s.guestNo ?? 1 });
}

/** The account chip's letter: the first letter or digit of the name, upper-cased. */
export function initialOf(name: string): string {
  const first = [...name.trim()].find((c) => /[\p{L}\p{N}]/u.test(c));
  return (first ?? "?").toLocaleUpperCase();
}

/** Whole seconds shown as "H:MM:SS". */
export function chipText(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const ss = String(s % 60).padStart(2, "0");
  return `${h}:${String(m).padStart(2, "0")}:${ss}`; // i18n-ignore (a clock)
}

/** Whether the chip warns: 10 minutes or less left on a clock that counts down. */
export function isLow(s: Pick<ShellSession, "prepaid" | "warningMin">, seconds: number): boolean {
  if (s.warningMin !== null && s.warningMin <= LOW_SECONDS / 60) return true;
  return s.prepaid && seconds <= LOW_SECONDS;
}
