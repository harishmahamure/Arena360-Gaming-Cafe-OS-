export type Field = "rounding" | "minimum" | "grace" | "mode" | "credit_pause";

export const FIELDS: readonly Field[] = ["rounding", "minimum", "grace", "mode", "credit_pause"];

export const KEYS: Record<Field, string> = {
  rounding: "billing.rounding_min",
  minimum: "billing.minimum_min",
  grace: "billing.grace_min",
  mode: "billing.time_up_mode",
  credit_pause: "billing.credit_auto_pause",
};

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

export function validField(field: Field, text: string): boolean {
  if (field === "mode") return (MODES as readonly string[]).includes(text);
  if (field === "credit_pause") return (SWITCH as readonly string[]).includes(text);
  return validMinutes(field, text);
}
