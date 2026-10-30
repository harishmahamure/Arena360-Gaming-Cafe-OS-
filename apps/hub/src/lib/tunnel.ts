// The Hub's Cloudflare Tunnel line (`GET /ui/tunnel`, Story 8.39): the connector's state and the
// branch's tunnel address. It never carries the credential. Loopback only, like the snapshot.

import type { TunnelLine, TunnelState } from "@arena/ui/generated";
import type { MessageKey } from "@arena/ui/messages";

export const TUNNEL_URL = "http://127.0.0.1:7409/ui/tunnel";
const TIMEOUT_MS = 5000;

const STATES: readonly TunnelState[] = [
  "off",
  "starting",
  "on",
  "reconnecting",
  "unsupported_platform",
];

/** The text of each state; `unsupported_platform` reads "Needs the Windows Hub". */
export const TUNNEL_LABELS: Record<TunnelState, MessageKey> = {
  off: "hub.tunnel.state.off",
  starting: "hub.tunnel.state.starting",
  on: "hub.tunnel.state.on",
  reconnecting: "hub.tunnel.state.reconnecting",
  unsupported_platform: "hub.tunnel.state.unsupported_platform",
};

/** Whether the line is a problem the staff should notice, not just a state. */
export const needsAttention = (state: TunnelState) =>
  state === "reconnecting" || state === "unsupported_platform";

/** A valid line from the response body, or `null`. */
export function parseTunnelLine(body: unknown): TunnelLine | null {
  if (typeof body !== "object" || body === null) return null;
  const { state, hostname } = body as { state?: unknown; hostname?: unknown };
  if (!STATES.includes(state as TunnelState)) return null;
  if (hostname !== undefined && typeof hostname !== "string") return null;
  return { state: state as TunnelState, hostname };
}

/** Reads the line; `null` when the Hub cannot be reached or answers nonsense. */
export async function loadTunnel(): Promise<TunnelLine | null> {
  try {
    const res = await fetch(TUNNEL_URL, {
      signal: AbortSignal.timeout(TIMEOUT_MS),
      cache: "no-store",
    });
    return res.ok ? parseTunnelLine(await res.json()) : null;
  } catch {
    return null;
  }
}
