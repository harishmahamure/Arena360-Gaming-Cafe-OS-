export type Access = { notifications: boolean; battery_exempt: boolean; asked: boolean };

export type Bridge = {
  state(): string;
  startHub(): void;
  stopHub(): void;
  requestAccess(): void;
  skipAccess(): void;
  openBatterySettings(): void;
  openNotificationSettings(): void;
  /** "granted", or "ask" while the Bluetooth permission is still to be requested (Android 12+). */
  bluetoothState?(): string;
  /** Asks for the Bluetooth permission; the page gets `arena:bluetooth` when the user answers. */
  requestBluetooth?(): void;
  openAppSettings?(): void;
};

export const service: { access: Access | null } = $state({ access: null });

export const bridge = (): Bridge | null =>
  typeof window === "undefined" ? null : ((window as { ArenaHub?: Bridge }).ArenaHub ?? null);

export function refreshAccess() {
  try {
    const raw = bridge()?.state();
    service.access = raw ? (JSON.parse(raw) as Access) : null;
  } catch {
    service.access = null;
  }
}

export const alertCount = (a: Access | null) =>
  a ? Number(!a.battery_exempt) + Number(!a.notifications) : 0;
