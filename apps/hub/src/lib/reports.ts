import type { DailySales, SessionsReport, SnapshotLocale } from "@arena/ui/generated";
import { authHeaders, getToken, setToken } from "./auth";

export const REPORTS_URL = "http://127.0.0.1:7409/ui/reports";

/** The most business days one report covers (the Hub refuses more). */
export const MAX_DAYS = 90;

/** The reports on the list, in order. Later report stories add their entries here. */
export const REPORTS = [
  {
    id: "daily-sales",
    href: "/reports/daily-sales",
    title: "hub.reports.daily_sales",
    note: "hub.reports.daily_sales_note",
  },
  {
    id: "sessions",
    href: "/reports/sessions",
    title: "hub.reports.sessions",
    note: "hub.reports.sessions_note",
  },
] as const;

export type ReportOutcome<T> =
  | { kind: "ok"; view: T }
  | { kind: "invalid" }
  | { kind: "denied" }
  | { kind: "signed_out" }
  | { kind: "unreachable" };

const pad = (n: number) => String(n).padStart(2, "0");

/** A calendar date (`YYYY-MM-DD`) as a UTC day number, so day arithmetic ignores time zones. */
function dayNumber(day: string): number {
  const [y, m, d] = day.split("-").map(Number);
  return Date.UTC(y, m - 1, d) / 86_400_000;
}

function dayOf(n: number): string {
  const d = new Date(n * 86_400_000);
  return `${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())}`; // i18n-ignore
}

/** `day` moved by `delta` calendar days. */
export function addDays(day: string, delta: number): string {
  return dayOf(dayNumber(day) + delta);
}

/** The days from `from` to `to` inclusive (0 when the dates are not a range). */
export function daySpan(from: string, to: string): number {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(from) || !/^\d{4}-\d{2}-\d{2}$/.test(to)) return 0;
  const days = dayNumber(to) - dayNumber(from) + 1;
  return Number.isFinite(days) && days >= 1 ? days : 0;
}

/**
 * The business day at `nowMs` in the branch's time zone: the local date of the time minus the
 * day start (a 06:00 start keeps 05:59 in the previous day).
 */
export function businessDayNow(locale: SnapshotLocale, nowMs: number): string {
  const shifted = new Date(nowMs - locale.day_start_min * 60_000);
  const parts = new Intl.DateTimeFormat("en-CA", {
    timeZone: locale.tz,
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  }).formatToParts(shifted);
  const get = (type: string) => parts.find((p) => p.type === type)?.value ?? "";
  return `${get("year")}-${get("month")}-${get("day")}`; // i18n-ignore
}

/** The share of `part` in `whole` as a whole percentage (0 when there is no whole). */
export function percentOf(part: number, whole: number): number {
  return whole > 0 ? Math.max(0, Math.min(100, Math.round((part / whole) * 100))) : 0;
}

/** Basis points (10,000 = all) as a percentage text such as `62.5%`. */
export function bpText(bp: number): string {
  return `${Math.round(bp / 10) / 10}%`; // i18n-ignore
}

/** A local hour (0 to 23) as `18:00`. */
export function hourText(hour: number): string {
  return `${pad(hour)}:00`; // i18n-ignore
}

/** Minutes as hours and minutes: `135` becomes `2:15`. */
export function durationText(minutes: number): string {
  return `${Math.floor(minutes / 60)}:${pad(minutes % 60)}`; // i18n-ignore
}

async function loadReport<T>(
  kind: string,
  from: string,
  to: string,
  isReport: (body: unknown) => boolean,
): Promise<ReportOutcome<T>> {
  const token = getToken();
  if (!token) return { kind: "signed_out" };
  try {
    const query = new URLSearchParams({ from, to });
    const url = `${REPORTS_URL}/${kind}?${query}`; // i18n-ignore
    const res = await fetch(url, {
      headers: authHeaders(token),
      signal: AbortSignal.timeout(10_000),
      cache: "no-store",
    });
    if (res.status === 401) {
      setToken(null);
      return { kind: "signed_out" };
    }
    if (res.status === 403) return { kind: "denied" };
    if (res.status === 422) return { kind: "invalid" };
    const body: unknown = res.ok ? await res.json() : null;
    return isReport(body) ? { kind: "ok", view: body as T } : { kind: "unreachable" };
  } catch {
    return { kind: "unreachable" };
  }
}

const hasList = (body: unknown, key: string) =>
  typeof body === "object" &&
  body !== null &&
  Array.isArray((body as Record<string, unknown>)[key]);

/** `GET /ui/reports/daily-sales`. */
export const loadDailySales = (from: string, to: string) =>
  loadReport<DailySales>("daily-sales", from, to, (b) => hasList(b, "by_hour"));

/** `GET /ui/reports/sessions`. */
export const loadSessionsReport = (from: string, to: string) =>
  loadReport<SessionsReport>("sessions", from, to, (b) => hasList(b, "by_device"));
