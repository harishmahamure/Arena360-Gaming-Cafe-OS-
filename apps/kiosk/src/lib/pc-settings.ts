// The PC settings panel's rules (Story 6.35): how the service's answers read. Pure and free of the
// UI package, so the tests run it in Node. The copy is in `PcSettings.svelte` and `en.json`.
//
// The Hub decides what is saved (a profiled player's choices, never a guest's) and the service
// applies each change on the PC; the kiosk only asks and renders. A display mode is applied
// first and reverts after `revertMs` unless the player keeps it.

import type { PcSettingsPanel } from "@arena/ui/generated";

/** The pointer speeds Windows has. */
export const MOUSE_MIN = 1;
export const MOUSE_MAX = 20;

/** What asking for the panel came to. */
export type PanelOutcome =
  /** `unavailable`: the service did not answer, or this PC has no PC settings control. */
  { kind: "ok"; panel: PcSettingsPanel } | { kind: "unavailable" };

/** What a change came to. */
export type ChangeOutcome =
  /** Applied. `saved`: the Hub stored it. `revertMs`: a display mode now waits for "Keep this". */
  | { kind: "ok"; saved: boolean; revertMs: number | null }
  /** Applied for the session but not saved: the counter is not reachable. */
  | { kind: "reconnecting" }
  /** This PC cannot use that choice. */
  | { kind: "not_offered" }
  | { kind: "failed" };

/** What "Keep this" came to. */
export type KeepOutcome =
  | { kind: "ok"; saved: boolean }
  | { kind: "reconnecting" }
  /** Nothing waited any more: the display already went back. */
  | { kind: "gone" }
  | { kind: "failed" };

const isObject = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null;

const isStrings = (v: unknown): v is string[] =>
  Array.isArray(v) && v.every((s) => typeof s === "string");

const isLayout = (v: unknown): boolean =>
  isObject(v) && typeof v.id === "string" && typeof v.name === "string";

const optString = (v: unknown): boolean => v === undefined || v === null || typeof v === "string";

/** The panel in the service's answer to `pc_settings` (an ok body is a `PcSettingsPanel`). */
export function panelOf(answer: unknown): PanelOutcome {
  const unavailable: PanelOutcome = { kind: "unavailable" };
  if (!isObject(answer) || answer.ok !== true || !isObject(answer.body)) return unavailable;
  const b = answer.body;
  if (
    typeof b.mouse_speed !== "number" ||
    typeof b.display_mode !== "string" ||
    !isStrings(b.display_modes) ||
    !isStrings(b.audio_outputs) ||
    !optString(b.audio_output) ||
    !Array.isArray(b.keyboard_layouts) ||
    !b.keyboard_layouts.every(isLayout) ||
    !optString(b.keyboard_layout) ||
    !(b.revert_ms === undefined || b.revert_ms === null || typeof b.revert_ms === "number")
  ) {
    return unavailable;
  }
  return { kind: "ok", panel: b as unknown as PcSettingsPanel };
}

/** The Hub link is down or slow rather than the Hub or the PC saying no. */
const isLinkError = (answer: Record<string, unknown>): boolean => answer.code === "HUB_SLOW";

/** The outcome in the service's answer to `pc_settings_set` (an ok body is a `PcSettingsChanged`). */
export function changeOf(answer: unknown): ChangeOutcome {
  if (!isObject(answer) || typeof answer.ok !== "boolean") return { kind: "failed" };
  if (!answer.ok) {
    if (isLinkError(answer)) return { kind: "reconnecting" };
    return answer.code === "VALIDATION_FAILED" ? { kind: "not_offered" } : { kind: "failed" };
  }
  const b = answer.body;
  if (!isObject(b) || typeof b.saved !== "boolean") return { kind: "failed" };
  const revert = typeof b.revert_ms === "number" && b.revert_ms > 0 ? b.revert_ms : null;
  return { kind: "ok", saved: b.saved, revertMs: revert };
}

/** The outcome in the service's answer to `pc_settings_keep`. */
export function keepOf(answer: unknown): KeepOutcome {
  if (!isObject(answer) || typeof answer.ok !== "boolean") return { kind: "failed" };
  if (!answer.ok) {
    if (isLinkError(answer)) return { kind: "reconnecting" };
    return answer.code === "NOT_FOUND" ? { kind: "gone" } : { kind: "failed" };
  }
  const b = answer.body;
  return isObject(b) && typeof b.saved === "boolean"
    ? { kind: "ok", saved: b.saved }
    : { kind: "failed" };
}

/** A `WxH@Hz` mode split into its numbers; `null` when it is not one. */
export function parseMode(mode: string): { width: number; height: number; hz: number } | null {
  const m = /^(\d{3,5})x(\d{3,5})@(\d{2,4})$/.exec(mode);
  return m ? { width: Number(m[1]), height: Number(m[2]), hz: Number(m[3]) } : null;
}

/** The modes to list: the service's, with the current one first if it is not among them. */
export function modesToList(panel: PcSettingsPanel): string[] {
  return panel.display_modes.includes(panel.display_mode)
    ? panel.display_modes
    : [panel.display_mode, ...panel.display_modes];
}

/** A pointer speed inside Windows' range. */
export const clampSpeed = (n: number): number =>
  Math.min(MOUSE_MAX, Math.max(MOUSE_MIN, Math.round(Number.isFinite(n) ? n : MOUSE_MIN)));
