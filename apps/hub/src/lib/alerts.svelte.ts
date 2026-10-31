// Hub alerts: the open alerts the snapshot carries, the header list's rows, the acknowledge job
// and the sound loop. The sound repeats every 30 s while any open device alert (or an expiring- or
// low-stock alert) exists; the branch-level conditions (licence unverified, PC clock wrong) are rows
// with no sound.

import { t } from "@arena/ui";
import type { AlertKind, HubSnapshot, SnapshotAlert, Tier } from "@arena/ui/generated";
import type { MessageKey } from "@arena/ui/messages";
import { SvelteMap } from "svelte/reactivity";
import { newIdempotencyKey, submitIntent } from "./auth";
import { TIER_LABELS } from "./plan";

/** The bundled sound (a file in `static/sounds`, never fetched from anywhere else). */
export const SOUND_URL = "/sounds/alert.wav";
/** The bundled order-ready chime (Story 7.4): it plays once for each new order-ready alert. */
export const READY_SOUND_URL = "/sounds/order-ready.wav";
/** How often the sound repeats while a device alert stays open. */
export const SOUND_REPEAT_MS = 30_000;

export const KIND_LABELS: Record<AlertKind, MessageKey> = {
  fixture: "hub.alerts.kind.fixture",
  time_up: "hub.alerts.kind.time_up",
  controller_missing: "hub.alerts.kind.controller_missing",
  printer_offline: "hub.alerts.kind.printer_offline",
  stock_expiring: "hub.alerts.kind.stock_expiring",
  stock_low: "hub.alerts.kind.stock_low",
  login_lockout: "hub.alerts.kind.login_lockout",
  check_pc: "hub.alerts.kind.check_pc",
  call_staff: "hub.alerts.kind.call_staff",
  kiosk_order: "hub.alerts.kind.kiosk_order",
  order_ready: "hub.alerts.kind.order_ready",
  backup_disk: "hub.alerts.kind.backup_disk",
  plan_updated: "hub.alerts.kind.plan_updated",
};

/** The tags a device alert puts on its tile (the tile badge's `AlertTag` subset). */
type TileTag =
  "new" | "call" | "time-up" | "controllers" | "locked-out" | "check-pc" | "order" | "ready";

// A branch-level alert (no device) has no tile tag.
export const KIND_TAGS: Record<AlertKind, TileTag | null> = {
  fixture: "new",
  time_up: "time-up",
  controller_missing: "controllers",
  printer_offline: null,
  stock_expiring: null,
  stock_low: null,
  login_lockout: "locked-out",
  check_pc: "check-pc",
  call_staff: "call",
  kiosk_order: "order",
  order_ready: "ready",
  backup_disk: null,
  plan_updated: null,
};

/** Branch-level kinds that also sound, and the screen a pick on them opens. */
const BRANCH_SOUNDS = new Set<AlertKind>(["stock_expiring", "stock_low"]);
const KIND_HREFS: Partial<Record<AlertKind, string>> = {
  stock_expiring: "/inventory/expiring",
  stock_low: "/inventory/low-stock",
  order_ready: "/pos",
};

/** The plan the Hub runs now: the "Plan updated" notice names it (Story 10.6). */
const plan: { tier: Tier | null } = $state({ tier: null });

/** The values a kind's label shows (the low-stock text names the product and what is left). */
// The plan-updated text names the plan now in force, read from the snapshot.
const labelParams = (a: SnapshotAlert): Record<string, string | number> | undefined =>
  a.kind === "plan_updated"
    ? { tier: plan.tier ? t(TIER_LABELS[plan.tier]) : "" }
    : a.kind === "stock_low"
      ? { product: a.product ?? "", qty: a.qty ?? 0 }
      : a.kind === "order_ready" && a.ticket_number != null
        ? { number: a.ticket_number }
        : undefined;

/** The label of an alert: a walk-up order-ready alert names its ticket. */
const labelOf = (a: SnapshotAlert): MessageKey =>
  a.kind === "order_ready" && a.ticket_number != null
    ? "hub.alerts.kind.order_ready_ticket"
    : KIND_LABELS[a.kind];

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
  /** The screen the row opens when it is not about a device. */
  href: string | null;
  label: MessageKey;
  /** The values `label` shows, if it takes any. */
  params?: Record<string, string | number>;
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

/**
 * The kitchen tag of each device with open tickets (Story 7.4): `ready` while any of its tickets
 * is Ready, else `order` while one is New or Preparing. It comes from the ticket stages, so it
 * follows the stage change and clears when the last ticket closes.
 */
const ticketTags = new SvelteMap<string, "ready" | "order">();

/**
 * The tag a device's tile shows: READY while a ticket is Ready (it outranks every alert tag), else its
 * oldest open alert's, else ORDER while a ticket is in the kitchen.
 */
export const tagOf = (deviceId: string): TileTag | undefined => {
  const first = openFor(deviceId)[0];
  const alert = first ? (KIND_TAGS[first.kind] ?? undefined) : undefined;
  const kitchen = ticketTags.get(deviceId);
  if (kitchen === "ready") return "ready";
  return alert ?? kitchen;
};

const takeTicketTags = (snapshot: HubSnapshot | null) => {
  ticketTags.clear();
  for (const bill of snapshot?.bills ?? []) {
    if (!bill.device_id) continue;
    for (const tk of bill.tickets ?? []) {
      if (tk.stage === "ready") ticketTags.set(bill.device_id, "ready");
      else if (!ticketTags.has(bill.device_id) && (tk.stage === "new" || tk.stage === "preparing"))
        ticketTags.set(bill.device_id, "order");
    }
  }
};

/** The header list: open alerts oldest first, then the branch conditions. */
export function listRows(): AlertRow[] {
  return [
    ...alerts.open.map((a) => ({
      key: a.id,
      alertId: a.id,
      deviceId: a.device_id ?? null,
      href: KIND_HREFS[a.kind] ?? null,
      label: labelOf(a),
      params: labelParams(a),
      since: a.raised_at,
    })),
    ...alerts.conditions.map((c) => ({
      key: c,
      alertId: null,
      deviceId: null,
      href: null,
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
let readyAudio: HTMLAudioElement | undefined;
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

/** The order-ready chime: once per new alert, never repeated. */
function playReady() {
  if (typeof Audio === "undefined") return;
  readyAudio ??= new Audio(READY_SOUND_URL);
  readyAudio.currentTime = 0;
  void Promise.resolve(readyAudio.play()).catch(() => {});
}

function settleSound() {
  const sounding = alerts.open.filter(
    (a) => a.device_id || BRANCH_SOUNDS.has(a.kind) || a.kind === "order_ready",
  );
  const fresh = sounding.filter((a) => !heard.has(a.id));
  heard = new Set(sounding.map((a) => a.id));
  if (fresh.some((a) => a.kind === "order_ready")) playReady();
  const ids = sounding.filter((a) => a.kind !== "order_ready");
  if (ids.length === 0) {
    clearInterval(timer);
    timer = undefined;
    return;
  }
  if (!fresh.some((a) => a.kind !== "order_ready") && timer) return;
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
      a.device_id
        ? (names.get(a.device_id) ?? "")
        : a.ticket_number != null
          ? t("hub.alerts.ticket_where", { number: a.ticket_number })
          : t("hub.alerts.branch"),
      a.kind === "order_ready" ? t(KIND_LABELS.order_ready) : t(labelOf(a), labelParams(a)),
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
  plan.tier = usable?.tier ?? null;
  takeTicketTags(usable);
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
      try {
        const out = await submitIntent({ kind: "alert.ack", alert_id: id }, newIdempotencyKey());
        const gone = out.kind === "error" && ["STALE", "NOT_FOUND"].includes(out.error.code);
        if (out.kind === "applied" || gone) done.add(id);
      } catch {
        continue;
      }
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

/**
 * Kinds that stay open when a sheet is opened: staff clear them with a sheet action (a CHECK PC
 * alert goes with "Retry cleanup" or "Checked", Story 6.11).
 */
const STAYS_OPEN = new Set<AlertKind>(["check_pc"]);

/** The open alerts of a device that opening its sheet acknowledges. */
export const ackable = (deviceId: string) =>
  openFor(deviceId).filter((a) => !STAYS_OPEN.has(a.kind));

/** The device's open CHECK PC alert, if any (its sheet then offers the two cleanup actions). */
export const checkPcOf = (deviceId: string) => openFor(deviceId).find((a) => a.kind === "check_pc");

/** Acknowledges every open alert of a device that its sheet opening clears. */
export const ackDevice = (deviceId: string) => ackAlerts(ackable(deviceId).map((a) => a.id));

/** Whether the open alert `alertId` stays open when its row is picked (see `STAYS_OPEN`). */
export const staysOpen = (alertId: string) => {
  const a = alerts.open.find((x) => x.id === alertId);
  return a !== undefined && STAYS_OPEN.has(a.kind);
};
