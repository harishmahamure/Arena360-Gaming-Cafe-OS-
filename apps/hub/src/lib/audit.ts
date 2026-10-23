// The audit viewer's loopback API (`/ui/audit`, `/ui/audit/item`, `/ui/audit/facets`). Every
// call carries the bearer token and, for a member without `audit.view`, the read-elevation
// ticket a manager's PIN granted (held in the page's memory only, never stored).

import type { AuditDetail, AuditFacets, AuditPage, UiError } from "@arena/ui/generated";
import { authHeaders, errorOf, getToken, noticeLocked, setToken } from "./auth";

export const AUDIT_URL = "http://127.0.0.1:7409/ui/audit";
export const AUDIT_ITEM_URL = "http://127.0.0.1:7409/ui/audit/item";
export const AUDIT_FACETS_URL = "http://127.0.0.1:7409/ui/audit/facets";
/** The header that carries a read-elevation ticket. */
export const ELEVATION_HEADER = "x-arena-elevation";
const TIMEOUT_MS = 5000;

/** The list filters, as the query string takes them (empty means unset). */
export type AuditFilters = {
  /** `<kind>` or `<kind>:<id>`. */
  actor: string;
  module: string;
  /** UTC ms, inclusive. */
  from: number | null;
  /** UTC ms, exclusive. */
  to: number | null;
};

export type AuditOutcome<T> =
  | { kind: "ok"; value: T }
  /** No `audit.view` and no valid read ticket (or it lapsed). */
  | { kind: "denied" }
  | { kind: "signed_out" }
  | { kind: "locked" }
  | { kind: "error"; error: UiError }
  | { kind: "unreachable" };

const isObject = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v);

async function auditGet<T>(
  url: string,
  ticket: string | null,
  valid: (body: unknown) => boolean,
): Promise<AuditOutcome<T>> {
  const token = getToken();
  if (!token) return { kind: "signed_out" };
  const headers: Record<string, string> = { ...(authHeaders(token) as Record<string, string>) };
  if (ticket) headers[ELEVATION_HEADER] = ticket;
  let res: Response;
  try {
    res = await fetch(url, {
      headers,
      signal: AbortSignal.timeout(TIMEOUT_MS),
      cache: "no-store",
    });
  } catch {
    return { kind: "unreachable" };
  }
  if (res.status === 401) {
    setToken(null);
    return { kind: "signed_out" };
  }
  if (res.ok) {
    try {
      const body: unknown = await res.json();
      return valid(body) ? { kind: "ok", value: body as T } : { kind: "unreachable" };
    } catch {
      return { kind: "unreachable" };
    }
  }
  const err = await errorOf(res);
  noticeLocked(err);
  if (err?.code === "SESSION_LOCKED") return { kind: "locked" };
  if (err?.code === "PERMISSION_DENIED") return { kind: "denied" };
  return err ? { kind: "error", error: err } : { kind: "unreachable" };
}

/** The query string of a list request. */
export function auditQuery(filters: AuditFilters, cursor: string | null, limit = 50): string {
  const q = new URLSearchParams();
  if (filters.actor) q.set("actor", filters.actor);
  if (filters.module) q.set("module", filters.module);
  if (filters.from !== null) q.set("from", String(filters.from));
  if (filters.to !== null) q.set("to", String(filters.to));
  if (cursor) q.set("cursor", cursor);
  q.set("limit", String(limit));
  return q.toString();
}

/** `GET /ui/audit`: one page, newest first. */
export function loadAuditPage(
  filters: AuditFilters,
  cursor: string | null,
  ticket: string | null,
): Promise<AuditOutcome<AuditPage>> {
  return auditGet<AuditPage>(
    `${AUDIT_URL}?${auditQuery(filters, cursor)}`, // i18n-ignore (a URL)
    ticket,
    (b) =>
      isObject(b) &&
      Array.isArray(b.items) &&
      (b.next_cursor === null || typeof b.next_cursor === "string"),
  );
}

/** `GET /ui/audit/item`: one change with its before/after images. */
export function loadAuditItem(
  epoch: string,
  seq: number,
  ticket: string | null,
): Promise<AuditOutcome<AuditDetail>> {
  const q = new URLSearchParams({ epoch, seq: String(seq) });
  return auditGet<AuditDetail>(
    `${AUDIT_ITEM_URL}?${q}`, // i18n-ignore (a URL)
    ticket,
    (b) => isObject(b) && isObject(b.item) && isObject(b.after) && Array.isArray(b.changed),
  );
}

/** `GET /ui/audit/facets`: the staff names and modules the filters offer. */
export function loadAuditFacets(ticket: string | null): Promise<AuditOutcome<AuditFacets>> {
  return auditGet<AuditFacets>(
    AUDIT_FACETS_URL,
    ticket,
    (b) => isObject(b) && Array.isArray(b.staff) && Array.isArray(b.modules),
  );
}
