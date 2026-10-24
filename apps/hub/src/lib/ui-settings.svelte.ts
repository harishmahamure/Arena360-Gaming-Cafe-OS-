// The Hub UI settings every open view follows (the snapshot's `ui` block), replaced live by
// `settings.changed` events from `/ui/events`.

import type { SnapshotUi } from "@arena/ui/generated";

/** The idle lock when the Hub does not say (the branch default). */
export const DEFAULT_IDLE_LOCK_MINUTES = 5;

export const uiSettings: { idleLockMinutes: number; hc: boolean } = $state({
  idleLockMinutes: DEFAULT_IDLE_LOCK_MINUTES,
  hc: false,
});

/**
 * Puts this Hub's theme on `<html>`: high contrast (`display.hc.hub`, set by a manager in
 * Settings → Devices & display) or the default dark. There is no light theme and no theme
 * control on the Hub, and the OS high-contrast setting is not read.
 */
export function applyTheme(hc: boolean) {
  if (typeof document === "undefined") return;
  const theme = hc ? "hc" : "dark";
  if (document.documentElement.dataset.theme !== theme) {
    document.documentElement.dataset.theme = theme;
  }
}

/** Takes the settings from a (re)loaded snapshot or a `settings.changed` event. */
export function setUiSettings(ui: SnapshotUi | null | undefined) {
  uiSettings.idleLockMinutes = ui?.idle_lock_minutes ?? DEFAULT_IDLE_LOCK_MINUTES;
  uiSettings.hc = ui?.hc === true;
  applyTheme(uiSettings.hc);
}
