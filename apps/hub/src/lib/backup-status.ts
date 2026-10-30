// The cloud backup status (`GET /ui/backup/status`, Story 9.16): what the header's backup line and
// System -> Backup say. The Hub answers from its own logs, so it reads the same whether or not the
// cloud can be reached now.

import type { BackupError, BackupStatus } from "@arena/ui/generated";
import type { MessageKey } from "@arena/ui/messages";
import { authHeaders, getToken, setToken } from "./auth";

export const BACKUP_STATUS_URL = "http://127.0.0.1:7409/ui/backup/status";
const TIMEOUT_MS = 5000;

export type BackupStatusOutcome =
  { kind: "ok"; status: BackupStatus } | { kind: "signed_out" } | { kind: "unreachable" };

/** The status from a response body, or `null` when it is not one. */
export function parseBackupStatus(body: unknown): BackupStatus | null {
  if (typeof body !== "object" || body === null) return null;
  const b = body as Record<string, unknown>;
  const numbers = ["as_of", "upload_interval_s", "pending_rows", "pending_batches"] as const;
  if (typeof b.cloud !== "boolean" || !numbers.every((k) => typeof b[k] === "number")) return null;
  return body as BackupStatus;
}

/** Reads the backup status. */
export async function loadBackupStatus(): Promise<BackupStatusOutcome> {
  const token = getToken();
  if (!token) return { kind: "signed_out" };
  try {
    const res = await fetch(BACKUP_STATUS_URL, {
      headers: authHeaders(token),
      signal: AbortSignal.timeout(TIMEOUT_MS),
      cache: "no-store",
    });
    if (res.status === 401) {
      setToken(null);
      return { kind: "signed_out" };
    }
    const status = res.ok ? parseBackupStatus(await res.json().catch(() => null)) : null;
    return status ? { kind: "ok", status } : { kind: "unreachable" };
  } catch {
    return { kind: "unreachable" };
  }
}

/**
 * The last upload is older than twice the interval: "Backup behind". A Hub that has not uploaded
 * yet is "first backup pending", not behind; a Free Hub is never behind.
 */
export function isBehind(status: BackupStatus, now: number): boolean {
  if (!status.cloud || status.upload_interval_s <= 0 || status.last_upload_at === null) {
    return false;
  }
  return now - status.last_upload_at > 2 * status.upload_interval_s * 1000;
}

/** How long ago `at` was, as the message and its count. */
export function agoOf(at: number | null, now: number): { key: MessageKey; n?: number } {
  if (at === null) return { key: "hub.backup.ago.never" };
  const minutes = Math.max(0, Math.floor((now - at) / 60_000));
  if (minutes < 1) return { key: "hub.backup.ago.now" };
  if (minutes < 60) return { key: "hub.backup.ago.minutes", n: minutes };
  const hours = Math.floor(minutes / 60);
  if (hours < 48) return { key: "hub.backup.ago.hours", n: hours };
  return { key: "hub.backup.ago.days", n: Math.floor(hours / 24) };
}

/** The plain-words message for the cause the Hub recorded. */
export function errorMessage(error: BackupError): MessageKey {
  switch (error.code) {
    case "CLOUD_UNREACHABLE":
      return "hub.backup.cloud.error.unreachable";
    case "RATE_LIMITED":
      return "hub.backup.cloud.error.rate_limited";
    case "TOKEN_INVALID":
    case "TOKEN_REVOKED":
      return "hub.backup.cloud.error.unpaired";
    case "FENCED":
      return "hub.backup.cloud.error.fenced";
    default:
      return "hub.backup.cloud.error.other";
  }
}

/** The name of the step the error belongs to. */
export function errorSource(error: BackupError): MessageKey {
  switch (error.source) {
    case "keys":
      return "hub.backup.cloud.source.keys";
    case "first_snapshot":
      return "hub.backup.cloud.source.first_snapshot";
    default:
      return "hub.backup.cloud.source.upload";
  }
}
