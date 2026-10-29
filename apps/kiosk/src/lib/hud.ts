// The overlay HUD's rules (Story 6.14): what the pill reads, how long a toast stays and how big
// the window must be. Pure and free of the UI package so the tests can run it in Node.

/** An info toast goes by itself after this long; a warning stays until dismissed or time-up. */
export const INFO_TOAST_MS = 8000;

/** The HUD toast's width, and the pill's height, in logical px (the window follows them). */
export const TOAST_WIDTH = 640;
export const PILL_HEIGHT = 56;

export type ToastKind = "info" | "warning";

export type HudToast = {
  id: number;
  /** The catalogue key of its text, or a unique key of its own when it carries `text`. */
  key: string;
  /** The words themselves, for a staff message (Story 6.16); the catalogue has no line for it. */
  text?: string;
  kind: ToastKind;
  /** When an info toast goes away, on the monotonic clock (`null`: it stays). */
  until: number | null;
  /** A second line under the text (a time warning's end time and grace). */
  detail?: string;
  /** The toast shows the Ctrl+Shift+E key caps (a time or low-balance warning). */
  hotkey?: boolean;
  /** The catalogue key of what the key caps do, when it is not "Extend" (Story 6.20). */
  hotkeyLabel?: string;
};

/** How long a toast of this kind stays, in ms (`null`: until dismissed). */
export function lifetime(kind: ToastKind): number | null {
  return kind === "info" ? INFO_TOAST_MS : null;
}

/**
 * The toasts after `key` is raised at `now`. The same text again replaces the one showing
 * (another blocked app), so toasts never stack with a duplicate; the new one restarts its clock.
 */
export function raise(
  toasts: readonly HudToast[],
  key: string,
  kind: ToastKind,
  now: number,
  id: number,
  text?: string,
): HudToast[] {
  const ms = lifetime(kind);
  return [
    ...toasts.filter((x) => x.key !== key),
    {
      id,
      key,
      ...(text === undefined ? {} : { text }),
      kind,
      until: ms === null ? null : now + ms,
    },
  ];
}

/** Seconds left of the grace at `now` (never below zero); `from` is when it began. */
export function graceSecondsLeft(from: number, graceMs: number, now: number): number {
  return Math.max(0, Math.ceil((from + graceMs - now) / 1000));
}

/** "m:ss" for the grace countdown. */
export function graceClock(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`; // i18n-ignore (a clock)
}

/**
 * The milestone a screen reader is told about while the grace runs (Story 6.22): 30 and 10 seconds
 * left. `null` before the first one. The text changes only at a milestone, so a polite live region
 * speaks it once.
 */
export function graceMilestone(seconds: number): 30 | 10 | null {
  if (seconds <= 10) return 10;
  if (seconds <= 30) return 30;
  return null;
}

/** The key every time warning shares, so a newer one replaces the older (Story 6.19). */
export const WARNING_KEY = "time-warning"; // i18n-ignore (a toast key)

/** The key the low-balance warning has, so it is raised once per crossing (Story 6.20). */
export const LOW_BALANCE_KEY = "low-balance"; // i18n-ignore (a toast key)

/** The key caps shown beside the Extend hotkey, one per cap. */
export const EXTEND_KEYS = ["Ctrl", "Shift", "E"] as const; // i18n-ignore (key caps)

/**
 * A warning that stays until dismissed or time-up; it replaces any warning of the same `key`
 * already showing (a time warning by default, or the low-balance warning).
 */
export function raiseWarning(
  toasts: readonly HudToast[],
  text: string,
  detail: string | null,
  id: number,
  key: string = WARNING_KEY,
  hotkeyLabel?: string,
): HudToast[] {
  return [
    ...toasts.filter((x) => x.key !== key),
    {
      id,
      key,
      text,
      ...(detail === null ? {} : { detail }),
      kind: "warning",
      until: null,
      hotkey: true,
      ...(hotkeyLabel === undefined ? {} : { hotkeyLabel }),
    },
  ];
}

/** The toasts without the warning under `key` (time-up, the session ending, an extension). */
export function dropWarning(toasts: readonly HudToast[], key: string = WARNING_KEY): HudToast[] {
  return toasts.filter((x) => x.key !== key);
}

/** Whole minutes of grace to say aloud: a part of a minute counts as a minute. */
export function graceMinutes(graceMs: number): number {
  return graceMs > 0 ? Math.ceil(graceMs / 60_000) : 0;
}

/** The key of the toast for staff message `id`: every message has its own, so they stack. */
export const staffToastKey = (id: number): string => `staff:${id}`; // i18n-ignore (a toast key)

/** The toasts still showing at `now` (the info ones whose 8 s are up have gone). */
export function live(toasts: readonly HudToast[], now: number): HudToast[] {
  return toasts.filter((x) => x.until === null || x.until > now);
}

/** Whole seconds shown as "H:MM:SS". */
export function clock(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return `${h}:${String(m).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`; // i18n-ignore (a clock)
}

/** Whether the HUD draws anything: a session's pill, or a toast. */
export function hasContent(session: boolean, toasts: readonly HudToast[]): boolean {
  return session || toasts.length > 0;
}
