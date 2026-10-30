// The Hub's push stream (`GET /ui/events?token=`, server-sent events). Every open view
// subscribes while someone is signed in; the Hub pushes, the UI never polls.

import type {
  AdminUnlockAckEvent,
  Connectivity,
  EntitlementChangedEvent,
  PcAckEvent,
  SettingsChangedEvent,
  UnlockOk,
  UpdatedByOwnerEvent,
} from "@arena/ui/generated";
import { isUpdatedByOwner } from "./owner-update";
import { pairingChangeOf, type PairingChange } from "./pairing";
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
  /** `groups.changed`: a group or a device's group changed; `byOwner` for an owner command. */
  onGroupsChanged?: (byOwner: boolean) => void;
  /** `updated_by_owner`: an owner's remote change of a rate, plan, rule, coupon or product applied. */
  onUpdatedByOwner?: (event: UpdatedByOwnerEvent) => void;
  /** `catalog.changed`: a product was written; reload the product list. */
  onCatalogChanged?: () => void;
  /** `library.changed`: the branch game library was written; reload the Games screen. */
  onLibraryChanged?: () => void;
  /** `connectivity.changed`: the WAN probe found a link down or back up. */
  onConnectivityChanged?: (event: Connectivity) => void;
  /** `device.admin_unlock_ack`: a kiosk granted a remote unlock. */
  onAdminUnlockAck?: (event: AdminUnlockAckEvent) => void;
  onPcAck?: (event: PcAckEvent) => void;
  onSessionStarted?: () => void;
  /** `order.placed`: products went on a running session's bill; reload the snapshot. */
  onOrderPlaced?: () => void;
  /** `order.line_changed`: a placed line was served or cancelled; reload the snapshot. */
  onOrderLineChanged?: () => void;
  /** `order.voided`: an order was voided and left its session's bill; reload the snapshot. */
  onOrderVoided?: () => void;
  /** `bill.settled`: a bill was settled; reload the snapshot so its UNPAID line clears. */
  onBillSettled?: () => void;
  /** `wallet.changed`: a wallet balance moved; open profiles and bills reload. */
  onWalletChanged?: (playerId: string) => void;
  /** `bill.reverted`: an End or a Settle was undone; reload the snapshot. */
  onBillReverted?: () => void;
  /** `effect.recorded`: a receipt, drawer kick or invoice number was recorded for a bill. */
  onEffectRecorded?: (orderId: string) => void;
  onPairingCodeChanged?: (change: PairingChange) => void;
  /** `entitlement.changed`: the Hub's effective tier moved; the ads gate re-evaluates. */
  onEntitlementChanged?: (event: EntitlementChangedEvent) => void;
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

const TIERS = ["free", "basic", "premium"];

export function isEntitlementChanged(v: unknown): v is EntitlementChangedEvent {
  return isObject(v) && (v.tier === null || (typeof v.tier === "string" && TIERS.includes(v.tier)));
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
    es.addEventListener("session.started", () => handlers.onSessionStarted?.());
    es.addEventListener("order.placed", () => handlers.onOrderPlaced?.());
    es.addEventListener("order.line_changed", () => handlers.onOrderLineChanged?.());
    es.addEventListener("order.voided", () => handlers.onOrderVoided?.());
    es.addEventListener("bill.settled", () => handlers.onBillSettled?.());
    es.addEventListener("bill.reverted", () => handlers.onBillReverted?.());
    es.addEventListener("wallet.changed", (ev) => {
      try {
        const data: unknown = JSON.parse((ev as MessageEvent<string>).data);
        if (isObject(data) && typeof data.player_id === "string") {
          handlers.onWalletChanged?.(data.player_id);
        }
      } catch {
        return;
      }
    });
    es.addEventListener("effect.recorded", (ev) => {
      try {
        const data: unknown = JSON.parse((ev as MessageEvent<string>).data);
        if (isObject(data) && typeof data.order_id === "string") {
          handlers.onEffectRecorded?.(data.order_id);
        }
      } catch {
        return;
      }
    });
    es.addEventListener("groups.changed", (ev) => {
      try {
        const data: unknown = JSON.parse((ev as MessageEvent<string>).data);
        handlers.onGroupsChanged?.(isObject(data) && data.by_owner === true);
      } catch {
        handlers.onGroupsChanged?.(false);
      }
    });
    es.addEventListener("updated_by_owner", (ev) => {
      try {
        const data: unknown = JSON.parse((ev as MessageEvent<string>).data);
        if (isUpdatedByOwner(data)) handlers.onUpdatedByOwner?.(data);
      } catch {
        // A malformed event is ignored: the screens already reloaded from their own events.
      }
    });
    es.addEventListener("catalog.changed", () => handlers.onCatalogChanged?.());
    es.addEventListener("library.changed", () => handlers.onLibraryChanged?.());
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
    es.addEventListener("device.pc_ack", (ev) => {
      try {
        const data = JSON.parse((ev as MessageEvent<string>).data) as Partial<PcAckEvent>;
        if (typeof data?.cmd_id === "string" && typeof data.device_id === "string") {
          handlers.onPcAck?.(data as PcAckEvent);
        }
      } catch {
        return;
      }
    });
    es.addEventListener("pairing.code_changed", (ev) => {
      try {
        const data: unknown = JSON.parse((ev as MessageEvent<string>).data);
        const change = pairingChangeOf(data);
        if (change) handlers.onPairingCodeChanged?.(change);
      } catch {
        return;
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
    es.addEventListener("entitlement.changed", (ev) => {
      try {
        const data: unknown = JSON.parse((ev as MessageEvent<string>).data);
        if (isEntitlementChanged(data)) handlers.onEntitlementChanged?.(data);
      } catch {
        // A malformed event is ignored: the next snapshot carries the tier.
      }
    });
    // `epoch.changed`: this Hub opened a new epoch (a restore); reload what the screens show.
    es.addEventListener("epoch.changed", () => handlers.onResync());
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
