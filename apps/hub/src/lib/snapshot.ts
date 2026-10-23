// Reads the branch snapshot from the local Hub service over loopback (`GET /ui/snapshot`).

import type { HubSnapshot } from "@arena/ui/generated";

/** The Hub service's loopback snapshot endpoint (the Tauri CSP allows only this host). */
export const SNAPSHOT_URL = "http://127.0.0.1:7409/ui/snapshot";
/** The `depends()` key the root layout registers, so Retry can refetch just the snapshot. */
export const SNAPSHOT_DEPENDENCY = "hub:snapshot";
const TIMEOUT_MS = 5000;

export type SnapshotResult = { ok: true; snapshot: HubSnapshot } | { ok: false };

/** Fetches the snapshot; any network failure, timeout or non-200 answer is `{ ok: false }`. */
export async function loadSnapshot(fetcher: typeof fetch): Promise<SnapshotResult> {
  try {
    const res = await fetcher(SNAPSHOT_URL, {
      signal: AbortSignal.timeout(TIMEOUT_MS),
      cache: "no-store",
    });
    if (!res.ok) return { ok: false };
    const snapshot: unknown = await res.json();
    if (!isHubSnapshot(snapshot)) return { ok: false };
    return { ok: true, snapshot };
  } catch {
    return { ok: false };
  }
}

const isObject = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v);
const isString = (v: unknown): v is string => typeof v === "string";

/**
 * Whether `v` has the `HubSnapshot` shape: `branch` is null or `{name, branch_code}` strings,
 * and each device has string `device_id`, `label` and `class` and a string-or-null `zone`.
 * Anything else is treated like an unreachable Hub.
 */
export function isHubSnapshot(v: unknown): v is HubSnapshot {
  if (!isObject(v) || !Array.isArray(v.devices)) return false;
  const branchOk =
    v.branch === null ||
    (isObject(v.branch) && isString(v.branch.name) && isString(v.branch.branch_code));
  return (
    branchOk &&
    v.devices.every(
      (d) =>
        isObject(d) &&
        isString(d.device_id) &&
        isString(d.label) &&
        isString(d.class) &&
        (d.zone === null || isString(d.zone)),
    )
  );
}
