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
  /** Opens the system folder picker; the page gets `arena:backup-folder` with `detail.uri` (null if cancelled). */
  pickBackupFolder?(): void;
  /** After the Hub accepted the folder as the backup location, drops the grants for other folders. */
  keepBackupFolder?(tree: string): void;
  /** The Play update state: "none", "downloading" or "ready" (Story 4.16). */
  updateState?(): string;
  /** Asks Play for an update now; the page gets `arena:update` when the state changes. */
  checkUpdate?(): void;
  /** Installs a downloaded update and restarts the Hub. Only called with no open session. */
  installUpdate?(): void;
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
