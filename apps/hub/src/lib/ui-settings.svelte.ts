// The Hub UI settings every open view follows (the snapshot's `ui` block), replaced live by
// `settings.changed` events from `/ui/events`.

import type { SnapshotUi } from "@arena/ui/generated";

/** The idle lock when the Hub does not say (the branch default). */
export const DEFAULT_IDLE_LOCK_MINUTES = 5;

export const uiSettings: { idleLockMinutes: number } = $state({
  idleLockMinutes: DEFAULT_IDLE_LOCK_MINUTES,
});

/** Takes the settings from a (re)loaded snapshot or a `settings.changed` event. */
export function setUiSettings(ui: SnapshotUi | null | undefined) {
  uiSettings.idleLockMinutes = ui?.idle_lock_minutes ?? DEFAULT_IDLE_LOCK_MINUTES;
}
