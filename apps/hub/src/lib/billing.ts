export type Field = "rounding" | "minimum" | "grace" | "mode";

export const FIELDS: readonly Field[] = ["rounding", "minimum", "grace", "mode"];

export const KEYS: Record<Field, string> = {
  rounding: "billing.rounding_min",
  minimum: "billing.minimum_min",
  grace: "billing.grace_min",
  mode: "billing.time_up_mode",
};

export const RANGES: Record<Exclude<Field, "mode">, readonly [number, number]> = {
  rounding: [1, 60],
  minimum: [0, 240],
  grace: [0, 10],
};

export const MODES = ["clean_up", "staff_handles"] as const;

export function validMinutes(field: Exclude<Field, "mode">, text: string): boolean {
  const [min, max] = RANGES[field];
  return /^(0|[1-9]\d{0,2})$/.test(text) && Number(text) >= min && Number(text) <= max;
}

export const validField = (field: Field, text: string): boolean =>
  field === "mode" ? (MODES as readonly string[]).includes(text) : validMinutes(field, text);
