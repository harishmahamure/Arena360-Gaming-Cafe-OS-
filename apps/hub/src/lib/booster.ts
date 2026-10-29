// The game booster for Settings → Games (Story 6.25) and the launch times the PCs report. The
// three settings are written by `settings.set`; `GET /ui/games/launch-times` answers each PC's
// latest measured launch and each game's median over 7 days.

import type { DeviceLaunchTimes, LaunchTimesView } from "@arena/ui/generated";
import type { MessageKey } from "@arena/ui/messages";
import { authHeaders, getToken, setToken } from "./auth";

export const LAUNCH_TIMES_URL = "http://127.0.0.1:7409/ui/games/launch-times";

export type Field = "close" | "priority" | "power";

export const FIELDS: readonly Field[] = ["close", "priority", "power"];

export const KEYS: Record<Field, string> = {
  close: "kiosk.booster_close",
  priority: "kiosk.booster_priority",
  power: "kiosk.booster_power_plan",
};

export const PRIORITIES = ["normal", "above_normal", "high"] as const;

export const PRIORITY_LABELS: Record<(typeof PRIORITIES)[number], MessageKey> = {
  normal: "hub.games.booster.priority.normal",
  above_normal: "hub.games.booster.priority.above_normal",
  high: "hub.games.booster.priority.high",
};

/** The most programs the close list holds, and the longest name. */
export const PROGRAMS_MAX = 50;
export const PROGRAM_NAME_MAX = 64;

/** Why the close list is refused. */
export type ProgramsProblem = "shape" | "duplicate" | "too_many";

const PROGRAM = /^[A-Za-z0-9][A-Za-z0-9._\-+() ]*$/;

/**
 * The programs typed into the close list (an executable name on each line, or separated by
 * commas), trimmed; or why they are refused. Empty is an empty list.
 */
export function parsePrograms(text: string): { programs: string[] } | { problem: ProgramsProblem } {
  const parts = text
    .split(/[\n,]/)
    .map((p) => p.trim())
    .filter((p) => p !== "");
  const programs: string[] = [];
  for (const p of parts) {
    const lower = p.toLowerCase();
    if (
      !lower.endsWith(".exe") ||
      lower.length <= ".exe".length ||
      p.length > PROGRAM_NAME_MAX ||
      !PROGRAM.test(p)
    ) {
      return { problem: "shape" };
    }
    if (programs.some((q) => q.toLowerCase() === lower)) return { problem: "duplicate" };
    programs.push(p);
  }
  return programs.length > PROGRAMS_MAX ? { problem: "too_many" } : { programs };
}

/** The close list as the form shows it: a program on each line. */
export function closeText(value: string): string {
  return value
    .split(",")
    .map((p) => p.trim())
    .filter((p) => p !== "")
    .join("\n");
}

/** The value stored for the form's `text` of `field` (the text itself when it is refused). */
export function storedValue(field: Field, text: string): string {
  if (field !== "close") return text;
  const r = parsePrograms(text);
  return "programs" in r ? r.programs.join(",") : text;
}

/** Whether the form's `text` for `field` is acceptable. */
export function validField(field: Field, text: string): boolean {
  if (field === "close") return "programs" in parsePrograms(text);
  if (field === "priority") return (PRIORITIES as readonly string[]).includes(text);
  return text === "true" || text === "false";
}

/** A launch time as seconds with one decimal: `14200` is `"14.2"`. */
export function seconds(ms: number): string {
  const tenths = Math.round(ms / 100);
  return (tenths / 10).toFixed(1);
}

export type LaunchTimesOutcome =
  { kind: "ok"; devices: DeviceLaunchTimes[] } | { kind: "signed_out" } | { kind: "unreachable" };

/** Each PC's last measured launch and its games' 7-day medians. */
export async function loadLaunchTimes(): Promise<LaunchTimesOutcome> {
  const token = getToken();
  if (!token) return { kind: "signed_out" };
  try {
    const res = await fetch(LAUNCH_TIMES_URL, {
      headers: authHeaders(token),
      signal: AbortSignal.timeout(5000),
      cache: "no-store",
    });
    if (res.status === 401) {
      setToken(null);
      return { kind: "signed_out" };
    }
    const body: unknown = res.ok ? await res.json() : null;
    const view = body as LaunchTimesView | null;
    return view && Array.isArray(view.devices)
      ? { kind: "ok", devices: view.devices }
      : { kind: "unreachable" };
  } catch {
    return { kind: "unreachable" };
  }
}
