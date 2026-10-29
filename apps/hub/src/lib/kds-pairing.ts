// Kitchen display pairing codes (Story 7.6): `POST /ui/kds-pairing-code` hands a Manager on a
// Basic-and-above branch a 6-digit single-use code and the address the tablet opens.

import type { KdsPairingCode } from "@arena/ui/generated";
import { authHeaders, getToken, setToken } from "./auth";
import { isPairingCode } from "./pairing";

export const KDS_PAIRING_CODE_URL = "http://127.0.0.1:7409/ui/kds-pairing-code";
export const KDS_PAIRING_DISCARD_URL = "http://127.0.0.1:7409/ui/kds-pairing-code/discard";
const TIMEOUT_MS = 5000;

export type KdsCodeOutcome =
  | { kind: "ok"; code: KdsPairingCode }
  | { kind: "signed_out" }
  | { kind: "denied" }
  | { kind: "locked" }
  | { kind: "failed" };

export function isKdsPairingCode(v: unknown): v is KdsPairingCode {
  if (!isPairingCode(v)) return false;
  const address = (v as { address?: unknown }).address;
  return address === undefined || typeof address === "string";
}

/** Asks the Hub for a kitchen display code. */
export async function requestKdsPairingCode(): Promise<KdsCodeOutcome> {
  const token = getToken();
  if (!token) return { kind: "signed_out" };
  try {
    const res = await fetch(KDS_PAIRING_CODE_URL, {
      method: "POST",
      headers: authHeaders(token),
      signal: AbortSignal.timeout(TIMEOUT_MS),
      cache: "no-store",
    });
    if (res.status === 401) {
      setToken(null);
      return { kind: "signed_out" };
    }
    if (res.status === 403) {
      const body: unknown = await res.json().catch(() => null);
      return (body as { code?: unknown } | null)?.code === "TIER_LOCKED"
        ? { kind: "locked" }
        : { kind: "denied" };
    }
    if (!res.ok) return { kind: "failed" };
    const body: unknown = await res.json();
    return isKdsPairingCode(body) ? { kind: "ok", code: body } : { kind: "failed" };
  } catch {
    return { kind: "failed" };
  }
}

/** The dialog closed: the Hub drops its code. */
export function discardKdsPairingCode() {
  const token = getToken();
  if (!token) return;
  void fetch(KDS_PAIRING_DISCARD_URL, {
    method: "POST",
    headers: authHeaders(token),
    keepalive: true,
    cache: "no-store",
  }).catch(() => {});
}
