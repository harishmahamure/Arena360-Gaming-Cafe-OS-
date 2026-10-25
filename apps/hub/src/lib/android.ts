import type { DeviceSpec, SpecFailure } from "@arena/ui/generated";

export const MIN_WEBVIEW_MAJOR = 111;

export type Device =
  { kind: "ok" } | { kind: "spec"; failures: SpecFailure[] } | { kind: "webview"; major: number };

type Invoke = (command: string) => Promise<unknown>;

export function webviewMajor(ua: string): number | null {
  const m = /Chrome\/(\d+)/.exec(ua);
  return m ? Number(m[1]) : null;
}

export function gbOf(bytes: number): number {
  return Math.round(bytes / 1e8) / 10;
}

let started = false;

async function startHub(invoke: Invoke | null) {
  const hub = (globalThis as { ArenaHub?: { startHub(): void } }).ArenaHub;
  if (started || !hub) return;
  started = true;
  hub.startHub();
  await invoke?.("hub_ready").catch(() => null);
}

function tauriInvoke(): Invoke | null {
  const g = (window as { __TAURI__?: { core?: { invoke?: Invoke } } }).__TAURI__;
  return g?.core?.invoke ?? null;
}

export async function loadDevice(
  ua: string = navigator.userAgent,
  invoke: Invoke | null = tauriInvoke(),
): Promise<Device> {
  if (!/Android/.test(ua)) return { kind: "ok" };
  const spec = invoke
    ? ((await invoke("device_spec").catch(() => null)) as DeviceSpec | null)
    : null;
  if (Array.isArray(spec?.failures) && spec.failures.length > 0) {
    return { kind: "spec", failures: spec.failures };
  }
  await startHub(invoke);
  const major = webviewMajor(ua);
  return major !== null && major < MIN_WEBVIEW_MAJOR ? { kind: "webview", major } : { kind: "ok" };
}
