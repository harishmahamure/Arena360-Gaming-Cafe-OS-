// The branch locale every open Hub view formats with. It starts from the snapshot and is
// replaced live by `settings.changed` events from `/ui/events` (never polled).

import type { SnapshotLocale } from "@arena/ui/generated";

export const branchLocale: {
  /** The locale in effect now, or `null` before the branch is set up. */
  current: SnapshotLocale | null;
  /** Bumped on every `settings.changed` event, so open forms can refresh. */
  version: number;
  /** The setting keys the last event wrote. */
  changedKeys: string[];
} = $state({ current: null, version: 0, changedKeys: [] });

/** Takes the locale from a (re)loaded snapshot. */
export function setLocale(locale: SnapshotLocale | null) {
  branchLocale.current = locale;
}

/** Applies a `settings.changed` event. */
export function applySettingsChanged(locale: SnapshotLocale | null, keys: string[]) {
  branchLocale.current = locale;
  branchLocale.changedKeys = keys;
  branchLocale.version++;
}
