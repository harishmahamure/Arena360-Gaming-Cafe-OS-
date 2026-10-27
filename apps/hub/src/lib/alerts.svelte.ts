// Hub alerts: the open alerts the snapshot carries, the header list's rows, the acknowledge job
// and the sound loop. The sound repeats every 30 s while any open device alert exists; the
// branch-level conditions (licence unverified, PC clock wrong) are rows with no sound.

import { t } from "@arena/ui";
import type { AlertKind, HubSnapshot, SnapshotAlert } from "@arena/ui/generated";
import type { MessageKey } from "@arena/ui/messages";
import { newIdempotencyKey, submitIntent } from "./auth";

/** The bundled sound (a file in `static/sounds`, never fetched from anywhere else). */
export const SOUND_URL = "/sounds/alert.wav";
/** How often the sound repeats while a device alert stays open. */
export const SOUND_REPEAT_MS = 30_000;

export const KIND_LABELS: Record<AlertKind, MessageKey> = {
  fixture: "hub.alerts.kind.fixture",
  time_up: "hub.alerts.kind.time_up",
};

export const KIND_TAGS: Record<AlertKind, "new" | "time-up"> = {
  fixture: "new",
  time_up: "time-up",
};

const CONDITIONS = {
  licence: "hub.licence.unverified",
  clock: "hub.clock.wrong",
} as const satisfies Record<string, MessageKey>;

type Condition = keyof typeof CONDITIONS;

/** One line of the header list. */
export type AlertRow = {
  key: string;
  /** The alert row to acknowledge (`null` for a branch condition, which clears on its own). */
  alertId: string | null;
  /** The device whose sheet the row opens (`null` for a branch-level row). */
  deviceId: string | null;
  label: MessageKey;
  /** When the alert was raised (UTC ms; `null` for a condition). */
  since: number | null;
};

export const alerts: { open: SnapshotAlert[]; conditions: Condition[] } = $state({
  open: [],
  conditions: [],
});

/** What a screen reader has been told since the last announcement (newest last). */
export const announcements: { id: string; text: string }[] = $state([]);

/** Asks the floor to open a device's sheet (the floor opens it when `n` changes). */
export const sheetRequest: { n: number; deviceId: string } = $state({ n: 0, deviceId: "" });

export const requestSheet = (deviceId: string) => {
  sheetRequest.deviceId = deviceId;
  sheetRequest.n++;
};

export const openFor = (deviceId: string) => alerts.open.filter((a) => a.device_id === deviceId);

/** The tag a device's tile shows: its oldest open alert's. */
export const tagOf = (deviceId: string): "new" | "time-up" | undefined => {
  const first = openFor(deviceId)[0];
  return first ? KIND_TAGS[first.kind] : undefined;
};

/** The header list: open alerts oldest first, then the branch conditions. */
export function listRows(): AlertRow[] {
  return [
    ...alerts.open.map((a) => ({
      key: a.id,
      alertId: a.id,
      deviceId: a.device_id ?? null,
      label: KIND_LABELS[a.kind],
      since: a.raised_at,
    })),
    ...alerts.conditions.map((c) => ({
      key: c,
      alertId: null,
      deviceId: null,
      label: CONDITIONS[c],
      since: null,
    })),
  ];
}

const KINDS = Object.keys(KIND_LABELS);

const isAlert = (a: unknown): a is SnapshotAlert =>
  typeof a === "object" &&
  a !== null &&
  typeof (a as SnapshotAlert).id === "string" &&
  KINDS.includes((a as SnapshotAlert).kind) &&
  typeof (a as SnapshotAlert).raised_at === "number";

/** How long ago, in words (`just now`, `5 min ago`, `2 h ago`). */
export function ageOf(since: number, now: number): string {
  const minutes = Math.max(0, Math.floor((now - since) / 60_000));
  if (minutes < 1) return t("hub.alerts.age_now");
  if (minutes < 60) return t("hub.alerts.age_minutes", { count: minutes });
  return t("hub.alerts.age_hours", { count: Math.floor(minutes / 60) });
}

let audio: HTMLAudioElement | undefined;
let timer: ReturnType<typeof setInterval> | undefined;
let heard = new Set<string>();
let told = new Set<string>();
let reload: (() => void) | null = null;

/** Registers what reloads the snapshot after an acknowledgement (the layout's). */
export function onAlertsChanged(fn: (() => void) | null) {
  reload = fn;
}

function play() {
  if (typeof Audio === "undefined") return;
  audio ??= new Audio(SOUND_URL);
  audio.currentTime = 0;
  void Promise.resolve(audio.play()).catch(() => {});
}

function settleSound() {
  const ids = new Set(alerts.open.filter((a) => a.device_id).map((a) => a.id));
  const fresh = [...ids].some((id) => !heard.has(id));
  heard = ids;
  if (ids.size === 0) {
    clearInterval(timer);
    timer = undefined;
    return;
  }
  if (!fresh && timer) return;
  clearInterval(timer);
  play();
  timer = setInterval(play, SOUND_REPEAT_MS);
}

function announce(snapshot: HubSnapshot) {
  const names = new Map(snapshot.devices.map((d) => [d.device_id, d.label]));
  const now = new Set<string>();
  const say = (id: string, where: string, what: string) => {
    const old = announcements.findIndex((x) => x.id === id);
    if (old >= 0) announcements.splice(old, 1);
    announcements.push({ id, text: t("hub.alerts.announce", { where, what }) });
  };
  for (const a of alerts.open) {
    now.add(a.id);
    if (told.has(a.id)) continue;
    say(
      a.id,
      a.device_id ? (names.get(a.device_id) ?? "") : t("hub.alerts.branch"),
      t(KIND_LABELS[a.kind]),
    );
  }
  for (const c of alerts.conditions) {
    now.add(c);
    if (told.has(c)) continue;
    say(c, t("hub.alerts.branch"), t(CONDITIONS[c]));
  }
  told = now;
  if (announcements.length > 5) announcements.splice(0, announcements.length - 5);
}

/**
 * Takes the open alerts and branch conditions of a loaded snapshot (`null`: nothing known), then
 * starts, keeps or stops the sound and announces what is new.
 */
export function syncAlerts(snapshot: HubSnapshot | null) {
  const usable = snapshot && snapshot.paired !== false ? snapshot : null;
  alerts.open = Array.isArray(usable?.alerts) ? usable.alerts.filter(isAlert) : [];
  alerts.conditions = [
    ...(usable?.licence_unverified ? (["licence"] as const) : []),
    ...(usable?.clock_wrong ? (["clock"] as const) : []),
  ];
  settleSound();
  if (usable) announce(usable);
}

/** Stops the sound loop (the shell is going away). */
export function stopAlerts() {
  clearInterval(timer);
  timer = undefined;
  heard = new Set();
}

const acking = new Set<string>();

/**
 * Acknowledges these alerts. One that is already acknowledged is ignored; the badge and sound
 * clear at once and the snapshot is reloaded.
 */
export async function ackAlerts(ids: string[]) {
  const mine = ids.filter((id) => !acking.has(id));
  if (mine.length === 0) return;
  for (const id of mine) acking.add(id);
  try {
    const done = new Set<string>();
    for (const id of mine) {
      const out = await submitIntent({ kind: "alert.ack", alert_id: id }, newIdempotencyKey());
      const gone = out.kind === "error" && ["STALE", "NOT_FOUND"].includes(out.error.code);
      if (out.kind === "applied" || gone) done.add(id);
    }
    if (done.size > 0) {
      alerts.open = alerts.open.filter((a) => !done.has(a.id));
      settleSound();
      reload?.();
    }
  } finally {
    for (const id of mine) acking.delete(id);
  }
}

/** Acknowledges every open alert of a device (its sheet was opened). */
export const ackDevice = (deviceId: string) => ackAlerts(openFor(deviceId).map((a) => a.id));
