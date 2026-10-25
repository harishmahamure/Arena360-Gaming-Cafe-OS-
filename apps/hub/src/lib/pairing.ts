import type { KioskPairingCode } from "@arena/ui/generated";
import { authHeaders, getToken, setToken } from "./auth";

export const PAIRING_CODE_URL = "http://127.0.0.1:7409/ui/pairing-code";
export const PAIRING_DISCARD_URL = "http://127.0.0.1:7409/ui/pairing-code/discard";
const TIMEOUT_MS = 5000;

export type PairingCodeOutcome =
  | { kind: "ok"; code: KioskPairingCode }
  | { kind: "signed_out" }
  | { kind: "denied" }
  | { kind: "failed" };

export function isPairingCode(v: unknown): v is KioskPairingCode {
  if (typeof v !== "object" || v === null) return false;
  const c = v as Record<string, unknown>;
  return typeof c.code === "string" && /^\d{6}$/.test(c.code) && typeof c.expires_at === "number";
}

export async function requestPairingCode(): Promise<PairingCodeOutcome> {
  const token = getToken();
  if (!token) return { kind: "signed_out" };
  try {
    const res = await fetch(PAIRING_CODE_URL, {
      method: "POST",
      headers: authHeaders(token),
      signal: AbortSignal.timeout(TIMEOUT_MS),
      cache: "no-store",
    });
    if (res.status === 401) {
      setToken(null);
      return { kind: "signed_out" };
    }
    if (res.status === 403) return { kind: "denied" };
    if (!res.ok) return { kind: "failed" };
    const body: unknown = await res.json();
    return isPairingCode(body) ? { kind: "ok", code: body } : { kind: "failed" };
  } catch {
    return { kind: "failed" };
  }
}

export function discardPairingCode() {
  const token = getToken();
  if (!token) return;
  void fetch(PAIRING_DISCARD_URL, {
    method: "POST",
    headers: authHeaders(token),
    keepalive: true,
    cache: "no-store",
  }).catch(() => {});
}

export type PairingChange = KioskPairingCode | "replaced";

export function pairingChangeOf(v: unknown): PairingChange | null {
  if (isPairingCode(v)) return v;
  if (typeof v === "object" && v !== null && (v as { replaced?: unknown }).replaced === true) {
    return "replaced";
  }
  return null;
}

const listeners = new Set<(change: PairingChange) => void>();

export function notePairingCode(change: PairingChange) {
  for (const cb of listeners) cb(change);
}

export function onPairingCode(cb: (change: PairingChange) => void): () => void {
  listeners.add(cb);
  return () => listeners.delete(cb);
}

export function groupCode(code: string): string {
  return [code.slice(0, 3), code.slice(3)].join(" ");
}

export function countdown(msLeft: number): string {
  const secs = Math.max(0, Math.ceil(msLeft / 1000));
  return [String(Math.floor(secs / 60)), String(secs % 60).padStart(2, "0")].join(":");
}
