import type { HubSnapshot, SessionStaff } from "@arena/ui/generated";

export const KEY_OPENING_HOURS = "branch.opening_hours";
export const KEY_OPENING_HOURS_SET = "branch.opening_hours_set";
export const KEY_FULFILMENT = "branch.fulfilment";
export const KEY_KIOSK_VOLUME = "kiosk.volume_default";
export const KEY_DISPLAY_MODES = "kiosk.display_modes";
export const KEY_BACKUP_DIR = "system.backup_dir";
export const KEY_ADS_CONSENT = "branch.ads_consent";
export const KEY_ADS_RATING = "branch.ads_rating";
export const AD_POLICY_URL = "https://arena360.cloud/legal/ad-policy";
export const DAYS_PER_WEEK = 7;
export const ADS_POLICY_VERSION = 1;

export type Day = { closed: true } | { open_min: number; close_min: number };

const isMinute = (v: unknown): v is number =>
  typeof v === "number" && Number.isInteger(v) && v >= 0 && v <= 1439;

export function parseWeek(value: string): Day[] | null {
  let parsed: unknown;
  try {
    parsed = JSON.parse(value);
  } catch {
    return null;
  }
  if (!Array.isArray(parsed) || parsed.length !== DAYS_PER_WEEK) return null;
  const days: Day[] = [];
  for (const d of parsed) {
    if (typeof d !== "object" || d === null) return null;
    const o = d as Record<string, unknown>;
    if (o.closed === true) days.push({ closed: true });
    else if (isMinute(o.open_min) && isMinute(o.close_min))
      days.push({ open_min: o.open_min, close_min: o.close_min });
    else return null;
  }
  return days;
}

export const weekJson = (days: Day[]): string => JSON.stringify(days);

export function toClock(minutes: number): string {
  const h = String(Math.floor(minutes / 60)).padStart(2, "0");
  const m = String(minutes % 60).padStart(2, "0");
  return h + ":" + m;
}

export function parseClock(text: string): number | null {
  const m = /^\s*(\d{1,2}):(\d{2})\s*$/.exec(text);
  if (!m) return null;
  const h = Number(m[1]);
  const min = Number(m[2]);
  if (h > 23 || min > 59) return null;
  return h * 60 + min;
}

export function dayOfField(field: string | undefined): number | null {
  const m = /^branch\.opening_hours\[(\d)\]$/.exec(field ?? "");
  return m ? Number(m[1]) : null;
}

export function sessionStaff(data: unknown): SessionStaff | null {
  const session = (data as { session?: { state: string; staff?: SessionStaff } }).session;
  return session?.state === "signed_in" ? (session.staff ?? null) : null;
}

export function snapshotOf(data: unknown): HubSnapshot | null {
  const hub = (data as { hub?: { ok: boolean; snapshot?: HubSnapshot } }).hub;
  return hub?.ok ? (hub.snapshot ?? null) : null;
}

export function adsNeeded(snapshot: HubSnapshot | null): boolean {
  return snapshot?.setup?.ad_consent !== "not_needed";
}

/** The folder chosen on Android is a document tree URI; this is its readable part, e.g. `1A2B-3C4D:Backups`. */
export function folderLabel(location: string): string {
  const tree = /\/tree\/([^/?#]+)/.exec(location)?.[1];
  if (!tree) return location;
  try {
    return decodeURIComponent(tree);
  } catch {
    return tree;
  }
}
