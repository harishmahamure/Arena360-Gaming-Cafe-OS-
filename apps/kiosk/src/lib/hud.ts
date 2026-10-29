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
  /** The catalogue key of its text. */
  key: string;
  kind: ToastKind;
  /** When an info toast goes away, on the monotonic clock (`null`: it stays). */
  until: number | null;
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
): HudToast[] {
  const ms = lifetime(kind);
  return [
    ...toasts.filter((x) => x.key !== key),
    { id, key, kind, until: ms === null ? null : now + ms },
  ];
}

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
