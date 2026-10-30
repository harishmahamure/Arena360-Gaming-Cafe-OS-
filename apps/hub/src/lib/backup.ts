import type { BackupExport } from "@arena/ui/generated";
import type { MessageKey } from "@arena/ui/messages";
import { authHeaders, errorOf, getToken, noticeLocked, setToken } from "./auth";

/** The setting that holds the last export that finished (JSON of a `BackupExport`, or empty). */
export const KEY_BACKUP_LAST_EXPORT = "backup.last_export";
export const BACKUP_EXPORT_URL = "http://127.0.0.1:7409/ui/backup/export";
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

/** Why the Hub could not write the file into the backup location. */
export type FolderProblem = "not_set" | "not_found" | "not_dir" | "not_writable" | "unsupported";

export type ExportOutcome =
  | { kind: "ok"; export: BackupExport }
  | { kind: "folder"; problem: FolderProblem }
  | { kind: "pin_required" }
  | { kind: "denied" }
  | { kind: "not_primary" }
  | { kind: "signed_out" }
  | { kind: "failed" };

const PROBLEMS: readonly string[] = [
  "not_set",
  "not_found",
  "not_dir",
  "not_writable",
  "unsupported",
];

/**
 * `POST /ui/backup/export`: the Hub writes the whole database as a `.a360` file into the backup
 * location and records it. A Counter sends a manager's PIN `ticket` for `backup.export`. Making
 * the file takes a few seconds on a big database, so the wait is long.
 */
export async function exportDatabase(ticket?: string): Promise<ExportOutcome> {
  const token = getToken();
  if (!token) return { kind: "signed_out" };
  let res: Response;
  try {
    res = await fetch(BACKUP_EXPORT_URL, {
      method: "POST",
      headers: { ...authHeaders(token), "content-type": "application/json" },
      body: JSON.stringify(ticket ? { elevation_ticket: ticket } : {}),
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
      const body = (await res.json()) as BackupExport;
      if (typeof body.file_name === "string" && typeof body.bytes === "number") {
        return { kind: "ok", export: body };
      }
    } catch {
      // fall through
    }
    return { kind: "failed" };
  }
  const error = await errorOf(res);
  noticeLocked(error);
  switch (error?.code) {
    case "VALIDATION_FAILED":
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
