// The webview's only link to the outside: the Tauri shell, which relays the Kiosk service.
// The webview has no Hub socket and no token; it hears the Hub link state as the `hub-status`
// event and reports its own restorable state, which comes back as `restore-state` after a
// restart. Toasts the service raises (a blocked app was closed) arrive as `toast`, and the
// high-contrast setting a manager chose for this kiosk as `theme`. Outside Tauri (a plain browser in dev) every call is a no-op.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { KioskHubState, KioskHubStatus } from "@arena/ui/generated";
// The event and command names, shared with the Rust shell (its tests read the same file).
import names from "../../src-tauri/ipc-names.json" with { type: "json" };

export const HUB_STATUS_EVENT = names.events.hubStatus;
export const RESTORE_STATE_EVENT = names.events.restoreState;
export const TOAST_EVENT = names.events.toast;
export const ADMIN_EXIT_EVENT = names.events.adminExit;
export const ADMIN_EXIT_ENDED_EVENT = names.events.adminExitEnded;
export const THEME_EVENT = names.events.theme;
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

/** The toasts the service may raise (catalogue keys); anything else is ignored. */
export const TOAST_KEYS = ["player.app.blocked"] as const;
export type ToastKey = (typeof TOAST_KEYS)[number];

export function isToastKey(v: unknown): v is ToastKey {
  return typeof v === "string" && (TOAST_KEYS as readonly string[]).includes(v);
}

/** Calls `cb` with every toast the service raises (e.g. after it closed a blocked app). */
export async function onToast(cb: (key: ToastKey) => void): Promise<() => void> {
  if (!inTauri()) return () => {};
  return listen<unknown>(TOAST_EVENT, (e) => {
    if (isToastKey(e.payload)) cb(e.payload);
  });
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

/** What the service answered to an admin-exit PIN (Story 1.20). */
export type AdminExitResult =
  | { kind: "granted"; until: number }
  | { kind: "rejected" }
  | { kind: "locked"; retryAfterS: number }
  | { kind: "unreachable" };

type Answer = { ok: boolean; code?: string; body: unknown };

const isAnswer = (v: unknown): v is Answer =>
  typeof v === "object" && v !== null && typeof (v as { ok?: unknown }).ok === "boolean";

const field = (body: unknown, name: string): number | null => {
  const v =
    typeof body === "object" && body !== null ? (body as Record<string, unknown>)[name] : null;
  return typeof v === "number" && Number.isFinite(v) ? v : null;
};

/**
 * Sends a manager PIN for an admin exit to the Kiosk service (through the shell, never the
 * Hub directly). The caller drops its own copy of the PIN.
 */
export async function adminExit(pin: string): Promise<AdminExitResult> {
  if (!inTauri()) return { kind: "unreachable" };
  try {
    const answer: unknown = await invoke(CMD.adminExit, { pin });
    if (!isAnswer(answer)) return { kind: "unreachable" };
    if (answer.ok) {
      const until = field(answer.body, "until");
      return until === null ? { kind: "unreachable" } : { kind: "granted", until };
    }
    if (answer.code === "AUTH_LOCKED") {
      return {
        kind: "locked",
        retryAfterS: Math.max(1, field(answer.body, "retry_after_s") ?? 60),
      };
    }
    if (answer.code === "PIN_REJECTED") return { kind: "rejected" };
    return { kind: "unreachable" };
  } catch {
    return { kind: "unreachable" };
  }
}

/** Whether the admin-exit prompt is paused (seconds left) and whether an exit runs (until). */
export async function adminExitStatus(): Promise<{
  retryAfterS: number | null;
  until: number | null;
}> {
  const none = { retryAfterS: null, until: null };
  if (!inTauri()) return none;
  try {
    const answer: unknown = await invoke(CMD.adminExitStatus);
    if (!isAnswer(answer) || !answer.ok) return none;
    return { retryAfterS: field(answer.body, "retry_after_s"), until: field(answer.body, "until") };
  } catch {
    return none;
  }
}

/** Calls `cb` with `until` (UTC ms) whenever an admin exit starts. */
export async function onAdminExit(cb: (until: number) => void): Promise<() => void> {
  if (!inTauri()) return () => {};
  return listen<unknown>(ADMIN_EXIT_EVENT, (e) => {
    const until = field(e.payload, "until");
    if (until !== null) cb(until);
  });
}

/** Calls `cb` whenever the admin exit ends. */
export async function onAdminExitEnded(cb: () => void): Promise<() => void> {
  if (!inTauri()) return () => {};
  return listen<unknown>(ADMIN_EXIT_ENDED_EVENT, () => cb());
}

/** Whether `e` is the hidden admin-exit combination, Ctrl+Alt+Shift+A. */
export function isAdminExitCombo(e: KeyboardEvent): boolean {
  return e.ctrlKey && e.altKey && e.shiftKey && !e.metaKey && e.code === "KeyA";
}

/**
 * The theme on `<html>`: high contrast or the default dark. Set by a manager per device on the
 * Hub (Story 1.21); the shell already set it before the first paint, so this only follows
 * changes. There is no player control and no light theme.
 */
export function applyTheme(hc: boolean): void {
  document.documentElement.dataset.theme = hc ? "hc" : "dark";
}

const hcOf = (v: unknown): boolean | null => {
  const hc = typeof v === "object" && v !== null ? (v as { hc?: unknown }).hc : v;
  return typeof hc === "boolean" ? hc : null;
};

/** The theme the shell knows now (`null` outside Tauri or when it does not answer). */
export async function currentTheme(): Promise<boolean | null> {
  if (!inTauri()) return null;
  try {
    return hcOf(await invoke(CMD.theme));
  } catch {
    return null;
  }
}

/** Calls `cb` with `hc` whenever this kiosk's theme changes. */
export async function onTheme(cb: (hc: boolean) => void): Promise<() => void> {
  if (!inTauri()) return () => {};
  return listen<unknown>(THEME_EVENT, (e) => {
    const hc = hcOf(e.payload);
    if (hc !== null) cb(hc);
  });
}
