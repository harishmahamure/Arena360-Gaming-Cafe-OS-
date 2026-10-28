// The Android Hub's Play update (Story 4.16). Play downloads a flexible update in the background;
// the page offers "Update ready" in System and installs only when no session at the branch is
// active, paused or in grace. A session that is not in one of those states (ended) never blocks.

import type { SnapshotSession } from "@arena/ui/generated";

export type UpdateStatus = "none" | "downloading" | "ready";

/** The session states that keep an install waiting. */
export const OPEN_SESSION_STATES = ["active", "paused", "grace"] as const;

/** How many sessions are open (active, paused or in grace). */
export function openSessions(sessions: readonly Pick<SnapshotSession, "state">[] | undefined) {
  return (sessions ?? []).filter((s) =>
    (OPEN_SESSION_STATES as readonly string[]).includes(s.state),
  ).length;
}

export function parseUpdateStatus(raw: unknown): UpdateStatus {
  return raw === "downloading" || raw === "ready" ? raw : "none";
}

/**
 * May the install start now? Only a downloaded update, and only with no open session. An unknown
 * count (`null`: no snapshot of the floor) waits too, since a session might be running.
 */
export function canInstall(status: UpdateStatus, open: number | null): boolean {
  return status === "ready" && open === 0;
}
