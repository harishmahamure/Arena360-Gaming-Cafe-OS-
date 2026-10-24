// The Hub's push stream (`GET /ui/events?token=`, server-sent events). Every open view
// subscribes while someone is signed in; the Hub pushes, the UI never polls.

import type {
  AdminUnlockAckEvent,
  Connectivity,
  SettingsChangedEvent,
  UnlockOk,
} from "@arena/ui/generated";
import { isConnectivity, isSnapshotLocale, normalizeUi } from "./snapshot";

export const EVENTS_URL = "http://127.0.0.1:7409/ui/events";
/** Waits before reopening a stream the Hub closed or refused, in seconds (then 15 s each). */
const REOPEN_DELAYS = [1, 2, 4, 8, 15];

export type EventHandlers = {
  /** `settings.changed`: the locale and UI settings now in effect and the keys written. */
  onSettingsChanged: (event: SettingsChangedEvent) => void;
  /** `session.locked`: this session was locked (in another window sharing it). */
  onSessionLocked?: () => void;
  /** `session.unlocked`: this session was unlocked, possibly by another member's PIN. */
  onSessionUnlocked?: (event: UnlockOk) => void;
  /** `devices.changed`: a device came online or went offline; reload the snapshot. */
  onDevicesChanged?: () => void;
  /** `connectivity.changed`: the WAN probe found a link down or back up. */
  onConnectivityChanged?: (event: Connectivity) => void;
  /** `device.admin_unlock_ack`: a kiosk granted a remote unlock. */
  onAdminUnlockAck?: (event: AdminUnlockAckEvent) => void;
  /** `resync`, or the stream reopened after a drop: events may be missed; reload the snapshot. */
  onResync: () => void;
  /** The Hub closed or refused the stream (signed out, revoked, or the Hub restarted). */
  onClosed: () => void;
};

const isObject = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v);

export function isSettingsChanged(v: unknown): v is SettingsChangedEvent {
  return (
    isObject(v) &&
    (v.locale === null || isSnapshotLocale(v.locale)) &&
    Array.isArray(v.keys) &&
    v.keys.every((k) => typeof k === "string")
  );
}

/** The event's UI settings; a missing or malformed block means the defaults. */
export function uiOf(event: SettingsChangedEvent): SettingsChangedEvent["ui"] {
  return normalizeUi(event.ui);
}

/**
 * Opens the stream for `token` and keeps it open: the browser retries a dropped connection by
 * itself; a refused one (401) is reopened with a back-off after `onClosed` has run. Returns the
 * function that closes it for good.
 */
export function connectEvents(token: string, handlers: EventHandlers): () => void {
  if (typeof EventSource === "undefined") return () => {};
  let source: EventSource | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let failures = 0;
  let stopped = false;
  let opened = false;

  const open = () => {
    if (stopped) return;
    const es = new EventSource(`${EVENTS_URL}?${new URLSearchParams({ token })}`); // i18n-ignore (a URL)
    source = es;
    es.addEventListener("open", () => {
      failures = 0;
      // Anything pushed while the stream was down is lost: catch up from the snapshot.
      if (opened) handlers.onResync();
      opened = true;
    });
    es.addEventListener("settings.changed", (ev) => {
      try {
        const data: unknown = JSON.parse((ev as MessageEvent<string>).data);
        if (isSettingsChanged(data)) handlers.onSettingsChanged(data);
        else handlers.onResync();
      } catch {
        handlers.onResync();
      }
    });
    es.addEventListener("resync", () => handlers.onResync());
    es.addEventListener("devices.changed", () => handlers.onDevicesChanged?.());
    es.addEventListener("device.admin_unlock_ack", (ev) => {
      try {
        const data = JSON.parse((ev as MessageEvent<string>).data) as Partial<AdminUnlockAckEvent>;
        if (typeof data?.exit_id === "string" && typeof data.device_id === "string") {
          handlers.onAdminUnlockAck?.(data as AdminUnlockAckEvent);
        }
      } catch {
        // A malformed ack is ignored: the floor times out and offers a retry.
      }
    });
    es.addEventListener("connectivity.changed", (ev) => {
      try {
        const data: unknown = JSON.parse((ev as MessageEvent<string>).data);
        if (isConnectivity(data)) handlers.onConnectivityChanged?.(data);
      } catch {
        // A malformed event is ignored: the next snapshot carries the state.
      }
    });
    es.addEventListener("session.locked", () => handlers.onSessionLocked?.());
    es.addEventListener("session.unlocked", (ev) => {
      try {
        const data = JSON.parse((ev as MessageEvent<string>).data) as Partial<UnlockOk>;
        if (typeof data?.switched === "boolean" && data.staff) {
          handlers.onSessionUnlocked?.(data as UnlockOk);
          return;
        }
      } catch {
        // fall through
      }
      handlers.onResync();
    });
    es.addEventListener("error", () => {
      if (es.readyState !== EventSource.CLOSED || stopped) return;
      handlers.onClosed();
      const seconds = REOPEN_DELAYS[Math.min(failures, REOPEN_DELAYS.length - 1)];
      failures++;
      timer = setTimeout(open, seconds * 1000);
    });
  };
  open();
  return () => {
    stopped = true;
    clearTimeout(timer);
    source?.close();
  };
}
