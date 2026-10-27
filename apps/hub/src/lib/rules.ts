import { t } from "@arena/ui";
import type { RuleView } from "@arena/ui/generated";
import type { MessageKey } from "@arena/ui/messages";
import { authHeaders, getToken, setToken } from "./auth";
import { clock } from "./plans";

export const RULES_URL = "http://127.0.0.1:7409/ui/rules";

export type RulesOutcome =
  { kind: "ok"; rules: RuleView[] } | { kind: "signed_out" } | { kind: "unreachable" };

export async function loadRules(): Promise<RulesOutcome> {
  const token = getToken();
  if (!token) return { kind: "signed_out" };
  try {
    const res = await fetch(RULES_URL, {
      headers: authHeaders(token),
      signal: AbortSignal.timeout(5000),
      cache: "no-store",
    });
    if (res.status === 401) {
      setToken(null);
      return { kind: "signed_out" };
    }
    const body: unknown = res.ok ? await res.json() : null;
    return Array.isArray(body)
      ? { kind: "ok", rules: body as RuleView[] }
      : { kind: "unreachable" };
  } catch {
    return { kind: "unreachable" };
  }
}

/** The weekdays of a rule, Monday first, matching bit 0 of `days_mask`. */
export const DAYS: { bit: number; label: MessageKey }[] = [
  { bit: 0, label: "hub.rules.day.mon" },
  { bit: 1, label: "hub.rules.day.tue" },
  { bit: 2, label: "hub.rules.day.wed" },
  { bit: 3, label: "hub.rules.day.thu" },
  { bit: 4, label: "hub.rules.day.fri" },
  { bit: 5, label: "hub.rules.day.sat" },
  { bit: 6, label: "hub.rules.day.sun" },
];

export const maskOf = (bits: number[]) => bits.reduce((m, b) => m | (1 << b), 0);
export const bitsOf = (mask: number) => DAYS.map((d) => d.bit).filter((b) => (mask >> b) & 1);

/** A percentage such as `-20` or `12.5` as basis points, or `null`. */
export function parsePercent(text: string): number | null {
  const m = /^([+-])?(\d{1,4})(?:\.(\d{1,2}))?$/.exec(text.trim());
  if (!m) return null;
  const bp = Number(m[2]) * 100 + Number((m[3] ?? "").padEnd(2, "0") || 0);
  return m[1] === "-" ? -bp : bp;
}

/** Basis points as a signed percentage, e.g. `-20%` or `+12.5%`. */
export function percentText(bp: number): string {
  const value = Math.abs(bp) / 100;
  const digits = Number.isInteger(value) ? String(value) : value.toFixed(2).replace(/0$/, "");
  return `${bp < 0 ? "−" : "+"}${digits}%`; // i18n-ignore (a signed number)
}

/** Basis points as the text of an input, e.g. `-20` or `12.5`. */
export function percentInput(bp: number): string {
  const value = bp / 100;
  return Number.isInteger(value) ? String(value) : String(Number(value.toFixed(2)));
}

/** The days of a weekly rule in words, e.g. `Mon, Tue, Wed`. */
export function daysText(mask: number): string {
  return bitsOf(mask)
    .map((b) => t(DAYS[b].label))
    .join(", ");
}

/** When a rule applies, in words. */
export function scheduleText(r: RuleView): string {
  if (r.rule_kind === "weekly") {
    return t("hub.rules.schedule.weekly", {
      days: daysText(r.days_mask ?? 0),
      window: t("hub.plans.window", { start: clock(r.start_min ?? 0), end: clock(r.end_min ?? 0) }),
    });
  }
  const from = r.date_from ?? "";
  const to = r.date_to ?? from;
  return from === to
    ? t("hub.rules.schedule.day", { date: from })
    : t("hub.rules.schedule.range", { from, to });
}
