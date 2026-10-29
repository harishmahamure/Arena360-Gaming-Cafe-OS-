import { floatText, parseFloat } from "./shift";

export type Field =
  "rounding" | "minimum" | "grace" | "mode" | "credit_pause" | "marks" | "low_balance";

export const FIELDS: readonly Field[] = [
  "rounding",
  "minimum",
  "grace",
  "mode",
  "credit_pause",
  "marks",
  "low_balance",
];

export const KEYS: Record<Field, string> = {
  rounding: "billing.rounding_min",
  minimum: "billing.minimum_min",
  grace: "billing.grace_min",
  mode: "billing.time_up_mode",
  credit_pause: "billing.credit_auto_pause",
  marks: "billing.warn_marks",
  low_balance: "billing.low_balance_minor",
};

/** Why a "Warn players at" entry is refused (each has its own field error). */
export type MarksProblem = "shape" | "range" | "duplicate";

/** The most marks a branch warns at, and the minutes each may be. */
export const MARKS_MAX = 3;
export const MARK_RANGE: readonly [number, number] = [1, 30];

/**
 * The marks typed into "Warn players at" (one to three whole minutes, 1–30, separated by commas
 * or spaces), longest first; or why they are refused.
 */
export function parseMarks(text: string): { marks: number[] } | { problem: MarksProblem } {
  const parts = text.split(/[\s,]+/).filter((p) => p !== "");
  if (parts.length === 0 || parts.length > MARKS_MAX || parts.some((p) => !/^\d{1,3}$/.test(p))) {
    return { problem: "shape" };
  }
  const marks = parts.map(Number);
  if (marks.some((m) => m < MARK_RANGE[0] || m > MARK_RANGE[1])) return { problem: "range" };
  if (new Set(marks).size !== marks.length) return { problem: "duplicate" };
  return { marks: marks.sort((a, b) => b - a) };
}

/** The value stored for the typed marks: `"10,5,1"`. `null` when they are refused. */
export function marksValue(text: string): string | null {
  const r = parseMarks(text);
  return "marks" in r ? r.marks.join(",") : null;
}

/** The low-balance amount typed in major units, as the minor units stored; `0` is off. */
export function lowBalanceValue(text: string, exponent: number): string | null {
  const minor = parseFloat(text, exponent);
  return minor !== null && minor <= 100_000_000 ? String(minor) : null;
}

export const RANGES: Record<"rounding" | "minimum" | "grace", readonly [number, number]> = {
  rounding: [1, 60],
  minimum: [0, 240],
  grace: [0, 10],
};

export const MODES = ["clean_up", "staff_handles"] as const;

export const SWITCH = ["true", "false"] as const;

export function validMinutes(field: keyof typeof RANGES, text: string): boolean {
  const [min, max] = RANGES[field];
  return /^(0|[1-9]\d{0,2})$/.test(text) && Number(text) >= min && Number(text) <= max;
}

/** Whether the form's `text` for `field` is acceptable (`exponent`: the branch currency's). */
export function validField(field: Field, text: string, exponent = 2): boolean {
  if (field === "mode") return (MODES as readonly string[]).includes(text);
  if (field === "credit_pause") return (SWITCH as readonly string[]).includes(text);
  if (field === "marks") return marksValue(text) !== null;
  if (field === "low_balance") return lowBalanceValue(text, exponent) !== null;
  return validMinutes(field, text);
}

/** The text the form shows for the stored `value` of `field`. */
export function formText(field: Field, value: string, exponent = 2): string {
  if (field === "marks") return value.split(",").join(", ");
  if (field === "low_balance") return floatText(Number(value), exponent);
  return value;
}

/** The value to store for the form's `text` of `field` (the text itself when it is refused). */
export function storedValue(field: Field, text: string, exponent = 2): string {
  if (field === "marks") return marksValue(text) ?? text;
  if (field === "low_balance") return lowBalanceValue(text, exponent) ?? text;
  return text;
}
