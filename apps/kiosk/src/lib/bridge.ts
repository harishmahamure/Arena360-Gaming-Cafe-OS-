// The webview's only link to the outside: the Tauri shell, which relays the Kiosk service.
// The webview has no Hub socket and no token; it hears the Hub link state as the `hub-status`
// event and reports its own restorable state, which comes back as `restore-state` after a
// restart. Outside Tauri (a plain browser in dev) every call is a no-op.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { KioskHubState, KioskHubStatus } from "@arena/ui/generated";
// The event and command names, shared with the Rust shell (its tests read the same file).
import names from "../../src-tauri/ipc-names.json" with { type: "json" };

export const HUB_STATUS_EVENT = names.events.hubStatus;
export const RESTORE_STATE_EVENT = names.events.restoreState;
const CMD = names.commands;
/** How often the webview tells the shell it is alive (the shell answers pings only then). */
export const HEARTBEAT_MS = 1000;

const STATES: readonly KioskHubState[] = ["connected", "reconnecting", "token_rejected"];

const inTauri = (): boolean => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export function isHubStatus(v: unknown): v is KioskHubStatus {
  return (
    typeof v === "object" &&
    v !== null &&
    STATES.includes((v as { state?: unknown }).state as KioskHubState)
  );
}

/** The link state the shell already knows, if any. */
export async function currentHubStatus(): Promise<KioskHubStatus | null> {
  if (!inTauri()) return null;
  try {
    const v: unknown = await invoke(CMD.hubStatus);
    return isHubStatus(v) ? v : null;
  } catch {
    return null;
  }
}

/** Calls `cb` on every link change; resolves to the unsubscribe function. */
export async function onHubStatus(cb: (s: KioskHubStatus) => void): Promise<() => void> {
  if (!inTauri()) return () => {};
  return listen<unknown>(HUB_STATUS_EVENT, (e) => {
    if (isHubStatus(e.payload)) cb(e.payload);
  });
}

/** Calls `cb` with the state to restore whenever the service sends one. */
export async function onRestoreState(cb: (state: unknown) => void): Promise<() => void> {
  if (!inTauri()) return () => {};
  return listen<unknown>(RESTORE_STATE_EVENT, (e) => cb(e.payload));
}

/** A state to restore that arrived before this page listened, once. */
export async function takeRestoreState(): Promise<unknown> {
  if (!inTauri()) return null;
  try {
    return await invoke(CMD.takeRestoreState);
  } catch {
    return null;
  }
}

/** Sends a heartbeat every second; resolves to the stop function. */
export function startHeartbeat(): () => void {
  if (!inTauri()) return () => {};
  const beat = () => void invoke(CMD.heartbeat).catch(() => {});
  beat();
  const timer = setInterval(beat, HEARTBEAT_MS);
  return () => clearInterval(timer);
}

/** The app's restorable state: where the player is (path and query). */
export type KioskAppState = { route: string };

/** Sends the app's state to the service (kept there across an app restart). */
export function reportAppState(state: KioskAppState): void {
  if (!inTauri()) return;
  void invoke(CMD.reportAppState, { state }).catch(() => {});
}

/** The in-app route a restored state names, or `null` when it names none (or anything else). */
export function restoredRoute(v: unknown): string | null {
  if (typeof v !== "object" || v === null) return null;
  const route = (v as { route?: unknown }).route;
  if (typeof route !== "string" || !route.startsWith("/")) return null;
  // Never another origin: no `//host` or `/\host`.
  if (route.startsWith("//") || route.includes("\\")) return null;
  return route;
}
