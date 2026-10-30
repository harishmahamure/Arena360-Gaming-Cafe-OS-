// The yearly GST archives the Hub has uploaded (`GET /ui/backup/gst-archive`, Story 9.14): one
// line per financial year for System -> Backup. A Free branch uploads none, so its list is empty.

import type { GstArchiveEntry, GstArchiveList } from "@arena/ui/generated";
import { authHeaders, getToken, setToken } from "./auth";

export const GST_ARCHIVE_URL = "http://127.0.0.1:7409/ui/backup/gst-archive";
const TIMEOUT_MS = 5000;

export type GstArchiveOutcome =
  | { kind: "ok"; archives: GstArchiveEntry[] }
  | { kind: "denied" }
  | { kind: "signed_out" }
  | { kind: "unreachable" };

/** A valid entry from the response body, or `null`. */
function entryOf(v: unknown): GstArchiveEntry | null {
  if (typeof v !== "object" || v === null) return null;
  const e = v as Record<string, unknown>;
  const numbers = ["invoices", "credit_notes", "orphaned", "bytes", "built_at"] as const;
  if (typeof e.fy !== "string" || typeof e.label !== "string") return null;
  if (!numbers.every((k) => typeof e[k] === "number")) return null;
  return e as unknown as GstArchiveEntry;
}

/** The archives from the response body, or `null` when it is not a list. */
export function parseGstArchives(body: unknown): GstArchiveEntry[] | null {
  const list = (body as Partial<GstArchiveList> | null)?.archives;
  if (!Array.isArray(list)) return null;
  const entries = list.map(entryOf);
  return entries.every((e) => e !== null) ? (entries as GstArchiveEntry[]) : null;
}

/** Reads the archived years, newest first. */
export async function loadGstArchives(): Promise<GstArchiveOutcome> {
  const token = getToken();
  if (!token) return { kind: "signed_out" };
  try {
    const res = await fetch(GST_ARCHIVE_URL, {
      headers: authHeaders(token),
      signal: AbortSignal.timeout(TIMEOUT_MS),
      cache: "no-store",
    });
    if (res.status === 401) {
      setToken(null);
      return { kind: "signed_out" };
    }
    if (res.status === 403) return { kind: "denied" };
    const archives = res.ok ? parseGstArchives(await res.json().catch(() => null)) : null;
    return archives ? { kind: "ok", archives } : { kind: "unreachable" };
  } catch {
    return { kind: "unreachable" };
  }
}
