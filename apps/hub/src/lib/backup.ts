import type { BackupExport, EverythingExported, ExportEntry } from "@arena/ui/generated";
import type { MessageKey } from "@arena/ui/messages";
import { authHeaders, errorOf, getToken, noticeLocked, setToken } from "./auth";

/** The setting that holds the last export that finished (JSON of a `BackupExport`, or empty). */
export const KEY_BACKUP_LAST_EXPORT = "backup.last_export";
export const BACKUP_EXPORT_URL = "http://127.0.0.1:7409/ui/backup/export";
export const BACKUP_EVERYTHING_URL = "http://127.0.0.1:7409/ui/backup/export-everything";
/** The intent kind a Counter's manager PIN approves. */
export const BACKUP_EXPORT_KIND = "backup.export";

/** The export `backup.last_export` names, or `null` before the first one. */
export function parseLastExport(value: string | undefined): BackupExport | null {
  if (!value) return null;
  try {
    const v: unknown = JSON.parse(value);
    if (
      typeof v === "object" &&
      v !== null &&
      typeof (v as BackupExport).file_name === "string" &&
      typeof (v as BackupExport).bytes === "number" &&
      typeof (v as BackupExport).exported_at === "number"
    ) {
      return v as BackupExport;
    }
  } catch {
    // An unreadable value reads as no export yet.
  }
  return null;
}

/** The last export and the ones before it, newest first: at most seven (Story 9.5). */
export function exportList(last: BackupExport | null): ExportEntry[] {
  if (!last) return [];
  const first: ExportEntry = {
    file_name: last.file_name,
    bytes: last.bytes,
    exported_at: last.exported_at,
    by: last.by ?? null,
  };
  return [first, ...(Array.isArray(last.recent) ? last.recent : [])].slice(0, 7);
}

/** Why the Hub could not write the file into the backup location. */
export type FolderProblem = "not_set" | "not_found" | "not_dir" | "not_writable" | "unsupported";

type Failure =
  | { kind: "folder"; problem: FolderProblem }
  | { kind: "range" }
  | { kind: "pin_required" }
  | { kind: "denied" }
  | { kind: "not_primary" }
  | { kind: "signed_out" }
  | { kind: "failed" };

export type ExportOutcome = { kind: "ok"; export: BackupExport } | Failure;

/** What "Export everything" answered: `range` is a start or end the Hub did not accept. */
export type EverythingOutcome = { kind: "ok"; done: EverythingExported } | Failure;

const PROBLEMS: readonly string[] = [
  "not_set",
  "not_found",
  "not_dir",
  "not_writable",
  "unsupported",
];

/** POSTs `payload` to an export route; the answer's JSON on success, else why not. */
async function postExport(
  url: string,
  payload: object,
): Promise<{ kind: "ok"; body: unknown } | Failure> {
  const token = getToken();
  if (!token) return { kind: "signed_out" };
  let res: Response;
  try {
    res = await fetch(url, {
      method: "POST",
      headers: { ...authHeaders(token), "content-type": "application/json" },
      body: JSON.stringify(payload),
      signal: AbortSignal.timeout(120_000),
      cache: "no-store",
    });
  } catch {
    return { kind: "failed" };
  }
  if (res.status === 401) {
    setToken(null);
    return { kind: "signed_out" };
  }
  if (res.ok) {
    try {
      return { kind: "ok", body: await res.json() };
    } catch {
      return { kind: "failed" };
    }
  }
  const error = await errorOf(res);
  noticeLocked(error);
  switch (error?.code) {
    case "VALIDATION_FAILED":
      if (error.field === "from" || error.field === "to") return { kind: "range" };
      return error.field === "folder" && PROBLEMS.includes(error.message)
        ? { kind: "folder", problem: error.message as FolderProblem }
        : { kind: "failed" };
    case "PIN_REQUIRED":
      return { kind: "pin_required" };
    case "PERMISSION_DENIED":
      return { kind: "denied" };
    case "NOT_PRIMARY":
      return { kind: "not_primary" };
    default:
      return { kind: "failed" };
  }
}

/**
 * `POST /ui/backup/export`: the Hub writes the whole database as a `.a360` file into the backup
 * location and records it. A Counter sends a manager's PIN `ticket` for `backup.export`. Making
 * the file takes a few seconds on a big database, so the wait is long.
 */
export async function exportDatabase(ticket?: string): Promise<ExportOutcome> {
  const out = await postExport(BACKUP_EXPORT_URL, ticket ? { elevation_ticket: ticket } : {});
  if (out.kind !== "ok") return out;
  const body = out.body as BackupExport;
  return typeof body.file_name === "string" && typeof body.bytes === "number"
    ? { kind: "ok", export: body }
    : { kind: "failed" };
}

/**
 * `POST /ui/backup/export-everything` (Story 9.6): the database file, every report for `from` to
 * `to` as CSV and a README, in one folder of the backup location.
 */
export async function exportEverything(
  from: string,
  to: string,
  ticket?: string,
): Promise<EverythingOutcome> {
  const out = await postExport(BACKUP_EVERYTHING_URL, {
    from,
    to,
    ...(ticket ? { elevation_ticket: ticket } : {}),
  });
  if (out.kind !== "ok") return out;
  const body = out.body as EverythingExported;
  return typeof body.folder_name === "string" && Array.isArray(body.reports)
    ? { kind: "ok", done: body }
    : { kind: "failed" };
}

/** The message for the cause the Hub named. */
export function problemMessage(problem: FolderProblem): MessageKey {
  switch (problem) {
    case "not_set":
      return "hub.backup.problem.not_set";
    case "not_found":
      return "hub.backup.problem.not_found";
    case "not_dir":
      return "hub.backup.problem.not_dir";
    case "not_writable":
      return "hub.backup.problem.not_writable";
    case "unsupported":
      return "hub.backup.problem.unsupported";
  }
}

/** Whether the cause can pass by itself (a drive put back), so "Try again" is worth offering. */
export function retryable(problem: FolderProblem): boolean {
  return problem === "not_found" || problem === "not_writable";
}

/** A moment as the panel shows it, in the branch's zone: `15 Jan 2027, 13:30`. */
export function exportedAt(ms: number, tz: string | undefined): string {
  const options: Intl.DateTimeFormatOptions = {
    day: "numeric",
    month: "short",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  };
  try {
    return new Intl.DateTimeFormat("en-GB", { ...options, timeZone: tz }).format(ms);
  } catch {
    return new Intl.DateTimeFormat("en-GB", { ...options, timeZone: "UTC" }).format(ms);
  }
}
