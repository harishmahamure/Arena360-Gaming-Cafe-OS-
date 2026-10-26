// The Hub's destinations: the rail (every screen, in order), the phone bottom bar and its More
// sheet, and the Ctrl+1–4 shortcuts.

import type { MessageKey } from "@arena/ui/messages";

export type ScreenId =
  | "floor"
  | "pos"
  | "players"
  | "sessions"
  | "inventory"
  | "shift"
  | "reports"
  | "settings"
  | "system";

export type Screen = {
  id: ScreenId;
  href: `/${ScreenId}`;
  /** The rail label, which is also the screen's `h1`. */
  label: MessageKey;
  /** The shorter bottom-bar label, where it differs. */
  short?: MessageKey;
};

const screen = (id: ScreenId, label: MessageKey, short?: MessageKey): Screen => ({
  id,
  href: `/${id}`,
  label,
  short,
});

/** Every screen, in rail order. Unbuilt screens open a placeholder; none are hidden. */
export const SCREENS: readonly Screen[] = [
  screen("floor", "hub.nav.floor", "hub.nav.floor_short"),
  screen("pos", "hub.nav.pos"),
  screen("players", "hub.nav.players"),
  screen("sessions", "hub.nav.sessions"),
  screen("inventory", "hub.nav.inventory"),
  screen("shift", "hub.nav.shift"),
  screen("reports", "hub.nav.reports"),
  screen("settings", "hub.nav.settings"),
  screen("system", "hub.nav.system"),
];

const byId = (id: ScreenId) => SCREENS.find((s) => s.id === id) as Screen;

/** The bottom bar's direct items (More follows them). */
export const BOTTOM_BAR: readonly Screen[] = (["floor", "pos", "players", "shift"] as const).map(
  byId,
);

/** The More sheet's rows, Sessions first. */
export const MORE: readonly Screen[] = (
  ["sessions", "inventory", "reports", "settings", "system"] as const
).map(byId);

/** Ctrl+1–4. */
export const SHORTCUTS: Readonly<Record<string, ScreenId>> = {
  "1": "floor",
  "2": "pos",
  "3": "players",
  "4": "shift",
};

/** Whether `el` takes typed text (an input, textarea, select or contenteditable). */
export function isTextEntry(el: Element | null): boolean {
  if (!el) return false;
  const tag = el.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return true;
  return (el as HTMLElement).isContentEditable === true;
}

type KeyLike = Pick<KeyboardEvent, "key" | "ctrlKey" | "altKey" | "metaKey" | "repeat">;

/**
 * The screen a key press opens, or `null`: Ctrl+1–4 only, never with Alt or Meta held, never
 * for a held key's auto-repeat, and never while a text field has focus.
 */
export function shortcutTarget(event: KeyLike, active: Element | null): Screen | null {
  if (!event.ctrlKey || event.altKey || event.metaKey || event.repeat) return null;
  const id = SHORTCUTS[event.key];
  if (!id || isTextEntry(active)) return null;
  return byId(id);
}

/** The screen a path belongs to, if any. */
export function screenOf(pathname: string): Screen | undefined {
  const first = pathname.split("/").filter(Boolean)[0];
  return SCREENS.find((s) => s.id === first);
}

export function isSearchKey(event: KeyLike, active: Element | null): boolean {
  return (
    event.ctrlKey &&
    !event.altKey &&
    !event.metaKey &&
    !event.repeat &&
    event.key.toLowerCase() === "k" &&
    !isTextEntry(active)
  );
}
