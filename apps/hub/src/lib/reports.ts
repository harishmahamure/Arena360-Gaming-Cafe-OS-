import type {
  DailySales,
  GstExported,
  GstMonth,
  SessionsReport,
  SettingEntry,
  ShiftList,
  SnapshotLocale,
  ZReport,
} from "@arena/ui/generated";
import type { MessageKey } from "@arena/ui/messages";
import { authHeaders, getToken, newIdempotencyKey, setToken, submitIntent } from "./auth";
import { KEY_TAX, parseTax } from "./tax";

export const REPORTS_URL = "http://127.0.0.1:7409/ui/reports";

/** The most business days one report covers (the Hub refuses more). */
export const MAX_DAYS = 90;

/** One report on the list; `gstOnly` ones are offered only in a GST-registered branch. */
export type ReportEntry = {
  id: string;
  href: string;
  title: MessageKey;
  note: MessageKey;
  gstOnly?: boolean;
};

/** The reports on the list, in order. Later report stories add their entries here. */
export const REPORTS: readonly ReportEntry[] = [
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
  {
    id: "shifts",
    href: "/reports/shifts",
    title: "hub.reports.shifts",
    note: "hub.reports.shifts_note",
  },
  {
    id: "gst",
    href: "/reports/gst",
    title: "hub.reports.gst",
    note: "hub.reports.gst_note",
    gstOnly: true,
  },
];

/** Whether the branch settings describe a GST-registered Indian branch (the GST return applies). */
export function isGstBranch(settings: readonly SettingEntry[]): boolean {
  const row = settings.find((s) => s.key === KEY_TAX);
  const tax = row ? parseTax(row.value) : null;
  return !!tax && tax.country === "IN" && tax.gst;
}

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

export type ShiftReadOutcome<T> =
  | { kind: "ok"; view: T }
  | { kind: "not_found" }
  | { kind: "open" }
  | { kind: "denied" }
  | { kind: "signed_out" }
  | { kind: "unreachable" };

async function loadShiftRead<T>(
  path: string,
  isView: (body: unknown) => boolean,
): Promise<ShiftReadOutcome<T>> {
  const token = getToken();
  if (!token) return { kind: "signed_out" };
  try {
    const url = `${REPORTS_URL}/${path}`; // i18n-ignore
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
    if (res.status === 404) return { kind: "not_found" };
    if (res.status === 409) return { kind: "open" };
    const body: unknown = res.ok ? await res.json() : null;
    return isView(body) ? { kind: "ok", view: body as T } : { kind: "unreachable" };
  } catch {
    return { kind: "unreachable" };
  }
}

/** `GET /ui/reports/shifts`: the closed shifts, newest first. */
export const loadShiftList = () => loadShiftRead<ShiftList>("shifts", (b) => hasList(b, "shifts"));

/** `GET /ui/reports/shifts/{id}`: the Z-report of a closed shift, as stored at close. */
export const loadZReport = (shiftId: string) =>
  loadShiftRead<ZReport>(`shifts/${encodeURIComponent(shiftId)}`, (b) => hasList(b, "by_type")); // i18n-ignore

export type ReprintZOutcome = "queued" | "refused" | "signed_out" | "unreachable";

/**
 * Prints the Z-report of a closed shift again (`shift.reprint_z`, Story 5.28). The Hub queues it
 * on the print outbox, marked "Reprint"; the answer only says it is queued.
 */
export async function reprintZReport(shiftId: string): Promise<ReprintZOutcome> {
  const outcome = await submitIntent(
    { kind: "shift.reprint_z", shift_id: shiftId },
    newIdempotencyKey(),
  );
  if (outcome.kind === "applied") return "queued";
  if (outcome.kind === "signed_out") return "signed_out";
  return outcome.kind === "error" ? "refused" : "unreachable";
}

/** The month (`YYYY-MM`) of the business day at `nowMs` in the branch. */
export const businessMonthNow = (locale: SnapshotLocale, nowMs: number) =>
  businessDayNow(locale, nowMs).slice(0, 7);

export type GstOutcome<T> =
  | { kind: "ok"; view: T }
  | { kind: "invalid"; field: string; reason: string }
  | { kind: "denied" }
  | { kind: "signed_out" }
  | { kind: "unreachable" };

/** Reads a GST answer: the view on 200, the refused field on 422. */
async function gstCall<T>(
  url: string,
  init: RequestInit,
  isView: (body: unknown) => boolean,
): Promise<GstOutcome<T>> {
  const token = getToken();
  if (!token) return { kind: "signed_out" };
  try {
    const res = await fetch(url, {
      ...init,
      headers: { ...authHeaders(token), ...init.headers },
      signal: AbortSignal.timeout(30_000),
      cache: "no-store",
    });
    if (res.status === 401) {
      setToken(null);
      return { kind: "signed_out" };
    }
    if (res.status === 403) return { kind: "denied" };
    const body: unknown = await res.json().catch(() => null);
    if (res.status === 422 && typeof body === "object" && body !== null) {
      const e = body as { field?: unknown; message?: unknown };
      return {
        kind: "invalid",
        field: typeof e.field === "string" ? e.field : "",
        reason: typeof e.message === "string" ? e.message : "",
      };
    }
    return res.ok && isView(body) ? { kind: "ok", view: body as T } : { kind: "unreachable" };
  } catch {
    return { kind: "unreachable" };
  }
}

const isGstMonth = (b: unknown) => hasList(b, "sections");

/** `GET /ui/reports/gst?month=YYYY-MM`: the month's totals per GSTR-1 section. */
export const loadGstMonth = (month: string) =>
  gstCall<GstMonth>(`${REPORTS_URL}/gst?${new URLSearchParams({ month })}`, {}, isGstMonth); // i18n-ignore

/**
 * `POST /ui/reports/gst/export`: the Hub writes the month's ZIP of CSVs into `folder` (a path on
 * the Hub's own disk or a USB drive) and answers what it wrote. No network call is made.
 */
export const exportGst = (month: string, folder: string) =>
  gstCall<GstExported>(
    `${REPORTS_URL}/gst/export`, // i18n-ignore
    {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ month, folder }),
    },
    (b) => typeof b === "object" && b !== null && typeof (b as GstExported).path === "string",
  );

/** The message for a refused export folder (`reason` is the Hub's code). */
export function folderProblem(reason: string): MessageKey {
  switch (reason) {
    case "not_found":
      return "hub.settings.system.invalid.not_found";
    case "not_dir":
      return "hub.settings.system.invalid.not_dir";
    case "not_writable":
      return "hub.settings.system.invalid.not_writable";
    case "unsupported":
      return "hub.gst.unsupported";
    default:
      return "hub.gst.folder_required";
  }
}
