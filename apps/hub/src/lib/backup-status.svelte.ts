// The backup status the header and System -> Backup share, refreshed while someone is signed in.

import type { BackupStatus } from "@arena/ui/generated";
import { isBehind, loadBackupStatus, type BackupStatusOutcome } from "./backup-status";

/** How often the Hub is asked again, in ms. */
export const REFRESH_MS = 30_000;
/** How often "4 min ago" and "behind" are worked out again, in ms. */
const TICK_MS = 15_000;

export const backup: { status: BackupStatus | null; offset: number; tick: number } = $state({
  status: null,
  offset: 0,
  tick: Date.now(),
});

/** The Hub's now, from the clock difference seen at the last read. */
export function hubNow(): number {
  return backup.tick + backup.offset;
}

/** Whether the last upload is more than twice the interval ago (reactive on the tick). */
export function backupBehind(): boolean {
  return backup.status !== null && isBehind(backup.status, hubNow());
}

/** Reads the status once and keeps it. The outcome tells the caller to sign in again. */
export async function refreshBackup(): Promise<BackupStatusOutcome> {
  const out = await loadBackupStatus();
  if (out.kind === "ok") {
    backup.status = out.status;
    backup.offset = out.status.as_of - Date.now();
    backup.tick = Date.now();
  }
  return out;
}

/** Forgets the status (signed out). */
export function clearBackup() {
  backup.status = null;
}

/**
 * Keeps the status fresh until the returned stop function is called: read now, again every
 * [`REFRESH_MS`], and the clock-driven text re-worked out every [`TICK_MS`]. `onSignedOut` is
 * called when the Hub says the session is gone.
 */
export function watchBackup(onSignedOut: () => void): () => void {
  let stopped = false;
  const read = async () => {
    const out = await refreshBackup();
    if (!stopped && out.kind === "signed_out") onSignedOut();
  };
  void read();
  const refresh = setInterval(() => void read(), REFRESH_MS);
  const tick = setInterval(() => (backup.tick = Date.now()), TICK_MS);
  return () => {
    stopped = true;
    clearInterval(refresh);
    clearInterval(tick);
    clearBackup();
  };
}
