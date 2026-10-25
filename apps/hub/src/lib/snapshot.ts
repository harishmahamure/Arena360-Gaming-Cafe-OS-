// Reads the branch snapshot from the local Hub service over loopback (`GET /ui/snapshot`).

import type {
  Connectivity,
  HubSnapshot,
  SnapshotLan,
  SnapshotLocale,
  SnapshotUi,
} from "@arena/ui/generated";

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
    // A Hub from before branch locales sends no `locale`: treat it as not set up. One from
    // before the idle lock sends no `ui`: the defaults apply.
    return {
      ok: true,
      snapshot: {
        ...snapshot,
        // A Hub from before device presence sends no `online`: offline.
        devices: snapshot.devices.map((d) => ({ ...d, online: d.online === true })),
        groups: Array.isArray(snapshot.groups) ? snapshot.groups : [],
        locale: snapshot.locale ?? null,
        // A missing or malformed `ui` block falls back to the defaults.
        ui: normalizeUi(snapshot.ui),
        // A Hub from before the connectivity probe sends none: online (no chip).
        connectivity: normalizeConnectivity(snapshot.connectivity),
        lan: normalizeLan(snapshot.lan),
      },
    };
  } catch {
    return { ok: false };
  }
}

const isObject = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v);
const isString = (v: unknown): v is string => typeof v === "string";
const isInt = (v: unknown, min: number, max: number): v is number =>
  typeof v === "number" && Number.isInteger(v) && v >= min && v <= max;

/** Whether `v` has the `SnapshotLocale` shape (the branch locale block). */
export function isSnapshotLocale(v: unknown): v is SnapshotLocale {
  if (!isObject(v)) return false;
  const p = v.pending;
  const entry = (e: unknown, ok: (value: unknown) => boolean) =>
    e === undefined || (isObject(e) && isString(e.effective_from) && ok(e.value));
  const pendingOk =
    p === null ||
    p === undefined ||
    (isObject(p) &&
      entry(p.tz, isString) &&
      entry(p.day_start_min, (x) => isInt(x, 0, 1439)) &&
      entry(p.fy_start_month, (x) => isInt(x, 1, 12)));
  return (
    isString(v.currency) &&
    isInt(v.exponent, 0, 6) &&
    isString(v.symbol) &&
    (v.symbol_position === "before" || v.symbol_position === "after") &&
    (v.number_format === "indian" || v.number_format === "international") &&
    isString(v.tz) &&
    isInt(v.day_start_min, 0, 1439) &&
    isInt(v.fy_start_month, 1, 12) &&
    pendingOk
  );
}

/**
 * Whether `v` has the `SnapshotUi` shape (`idle_lock_minutes` 1–30; `hc` may be missing, from a
 * Hub before per-device high contrast).
 */
export function isSnapshotUi(v: unknown): v is SnapshotUi {
  return (
    isObject(v) &&
    isInt(v.idle_lock_minutes, 1, 30) &&
    (v.hc === undefined || typeof v.hc === "boolean")
  );
}

/** The UI settings in `v`, or the defaults (5 minutes, not high contrast) when malformed. */
export function normalizeUi(v: unknown): SnapshotUi {
  return isSnapshotUi(v)
    ? { idle_lock_minutes: v.idle_lock_minutes, hc: v.hc === true }
    : { idle_lock_minutes: 5, hc: false };
}

export function normalizeLan(v: unknown): SnapshotLan | null {
  if (!isObject(v) || !Array.isArray(v.addresses)) return null;
  if (!isInt(v.lan_port, 1, 65535) || !isInt(v.web_port, 1, 65535) || !isString(v.fingerprint)) {
    return null;
  }
  return {
    addresses: v.addresses.filter(isString),
    lan_port: v.lan_port,
    web_port: v.web_port,
    fingerprint: v.fingerprint,
  };
}

export function shortFingerprint(fp: string): string {
  return (fp.replace(/[^0-9a-fA-F]/g, "").match(/.{1,4}/g) ?? []).slice(0, 4).join(" ");
}

const CONNECTIVITY_STATES: readonly string[] = ["online", "internet_offline", "cloud_unreachable"];

/** Whether `v` has the `Connectivity` shape (the WAN probe's last answer). */
export function isConnectivity(v: unknown): v is Connectivity {
  return (
    isObject(v) &&
    typeof v.state === "string" &&
    CONNECTIVITY_STATES.includes(v.state) &&
    (v.checked_at === null || typeof v.checked_at === "number")
  );
}

/** The connectivity in `v`, or `online` (no chip) when missing or malformed. */
export function normalizeConnectivity(v: unknown): Connectivity {
  return isConnectivity(v)
    ? { state: v.state, checked_at: v.checked_at }
    : { state: "online", checked_at: null };
}

/**
 * Whether `v` has the `HubSnapshot` shape: `branch` is null or `{name, branch_code}` strings,
 * each device has string `device_id`, `label` and `class` and a string-or-null `zone`, and
 * `locale` is null, absent or a `SnapshotLocale` (`ui` is not checked: a bad one means the
 * defaults).
 * Anything else is treated like an unreachable Hub.
 */
export function isHubSnapshot(v: unknown): v is HubSnapshot {
  if (!isObject(v) || !Array.isArray(v.devices)) return false;
  const branchOk =
    v.branch === null ||
    (isObject(v.branch) && isString(v.branch.name) && isString(v.branch.branch_code));
  const localeOk = v.locale === null || v.locale === undefined || isSnapshotLocale(v.locale);
  return (
    branchOk &&
    localeOk &&
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
