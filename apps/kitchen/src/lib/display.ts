// The kitchen tablet's side of the Hub (Story 7.6): the status and pairing endpoints, the stored
// `kds` token and the `/ws` connection. The page keeps the token in `localStorage` (never in a
// URL) and sends it only in the `hello` frame. There is no board, chime or ticket control here
// yet (Stories 7.7 to 7.10).

import type { Frame, KdsPairResponse, KdsStatus, SettingEntry } from "@arena/ui/generated";

/** Where the Hub serves this app (the build's `paths.base`). */
const BASE = "/kitchen";
const STATUS_URL = `${BASE}/status`; // i18n-ignore (an address)
const PAIR_URL = `${BASE}/pair`; // i18n-ignore (an address)
const SOCKET_PATH = "/ws"; // i18n-ignore (an address)
export const TOKEN_KEY = "arena.kitchen.token";
/** The name a display takes when the field is left empty. */
export const DEFAULT_NAME = "Kitchen";
/** The app version the `hello` reports. */
const APP_VERSION = "0.1.0";
const TIMEOUT_MS = 8000;
const RETRY_MIN_MS = 2000;
const RETRY_MAX_MS = 30_000;

/** The high-contrast key of one display: `display.hc.<display id>`. */
export const hcKey = (displayId: string) => `display.hc.${displayId}`; // i18n-ignore (a setting key)

export function readToken(): string | null {
  try {
    return globalThis.localStorage?.getItem(TOKEN_KEY) ?? null;
  } catch {
    return null;
  }
}

export function storeToken(token: string) {
  try {
    globalThis.localStorage?.setItem(TOKEN_KEY, token);
  } catch {
    // Storage blocked: the display pairs again after a reload.
  }
}

export function clearToken() {
  try {
    globalThis.localStorage?.removeItem(TOKEN_KEY);
  } catch {
    // Nothing stored.
  }
}

/** The display id at the front of a `kds` token (`<id>.<secret>`). */
export function displayIdOf(token: string): string | null {
  const id = token.split(".")[0];
  return /^[0-9A-Z]{26}$/.test(id) ? id : null;
}

export type StatusOutcome = { kind: "ok"; status: KdsStatus } | { kind: "unreachable" };

export async function fetchStatus(): Promise<StatusOutcome> {
  try {
    const res = await fetch(STATUS_URL, {
      cache: "no-store",
      signal: AbortSignal.timeout(TIMEOUT_MS),
    });
    if (!res.ok) return { kind: "unreachable" };
    const body = (await res.json()) as Partial<KdsStatus>;
    if (typeof body.available !== "boolean") return { kind: "unreachable" };
    return {
      kind: "ok",
      status: { available: body.available, branch_name: body.branch_name ?? "" },
    };
  } catch {
    return { kind: "unreachable" };
  }
}

export type PairOutcome =
  | { kind: "paired"; token: string; name: string }
  | { kind: "wrong" }
  | { kind: "locked" }
  | { kind: "limit" }
  | { kind: "limited" }
  | { kind: "unreachable" }
  | { kind: "failed" };

/** Posts the 6-digit code; the Hub answers the `kds` token once. */
export async function pairDisplay(code: string, name: string): Promise<PairOutcome> {
  try {
    const res = await fetch(PAIR_URL, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ code, name: [...name.trim()].slice(0, 40).join("") || DEFAULT_NAME }),
      cache: "no-store",
      signal: AbortSignal.timeout(TIMEOUT_MS),
    });
    const body = (await res.json().catch(() => ({}))) as Partial<KdsPairResponse> & {
      code?: string;
    };
    if (res.ok && typeof body.token === "string") {
      return { kind: "paired", token: body.token, name: body.name ?? DEFAULT_NAME };
    }
    if (body.code === "TIER_LOCKED") return { kind: "locked" };
    if (body.code === "DEVICE_LIMIT") return { kind: "limit" };
    if (body.code === "RATE_LIMITED" || res.status === 429) return { kind: "limited" };
    if (body.code === "PAIR_CODE_INVALID") return { kind: "wrong" };
    return { kind: "failed" };
  } catch {
    return { kind: "unreachable" };
  }
}

export type Link = {
  /** The Hub accepted the token. */
  onReady: () => void;
  /** The token is revoked or unknown: clear it and pair again. */
  onRevoked: () => void;
  /** This display's high-contrast setting, on connect and whenever it changes. */
  onHighContrast: (on: boolean) => void;
  /** The socket is gone (the link reconnects by itself). */
  onLost: () => void;
};

/** `ws://<host>/ws` (or `wss:` on an HTTPS page). */
function socketUrl(): string {
  const scheme = globalThis.location?.protocol === "https:" ? "wss:" : "ws:";
  const host = globalThis.location?.host;
  return [scheme, "//", host, SOCKET_PATH].join("");
}

/**
 * Connects to the Hub as a `kds` client and keeps the link up. Returns a function that closes
 * it. A `TOKEN_REVOKED` or `TOKEN_INVALID` error ends the link for good (`onRevoked`); any
 * other loss reconnects after a growing pause.
 */
export function connect(token: string, link: Link): () => void {
  const displayId = displayIdOf(token);
  let socket: WebSocket | null = null;
  let stopped = false;
  let retryMs = RETRY_MIN_MS;
  let timer: ReturnType<typeof setTimeout> | undefined;

  const send = (frame: Record<string, unknown>) => {
    if (socket?.readyState === WebSocket.OPEN) socket.send(JSON.stringify({ v: 1, ...frame }));
  };

  function takeValues(values: SettingEntry[] | undefined) {
    const mine = values?.find((v) => displayId !== null && v.key === hcKey(displayId));
    if (mine) link.onHighContrast(mine.value === "true");
  }

  function open() {
    if (stopped) return;
    let ready = false;
    const ws = new WebSocket(socketUrl());
    socket = ws;
    ws.onopen = () => {
      send({
        rid: "hello",
        kind: "hello",
        credential: token,
        class: "kds",
        versions: { app: APP_VERSION },
        capabilities: [],
      });
    };
    ws.onmessage = (message) => {
      let frame: Frame;
      try {
        frame = JSON.parse(String(message.data)) as Frame;
      } catch {
        return;
      }
      switch (frame.kind) {
        case "hello.ok":
          ready = true;
          retryMs = RETRY_MIN_MS;
          link.onReady();
          if (displayId) send({ rid: "hc", kind: "settings.get", keys: [hcKey(displayId)] });
          break;
        case "ping":
          send({ rid: frame.rid, kind: "pong" });
          break;
        case "settings.values":
          takeValues(frame.values);
          break;
        case "event":
          if (frame.event.kind === "settings.changed") takeValues(frame.event.values);
          break;
        case "error":
          if (frame.error.code === "TOKEN_REVOKED" || frame.error.code === "TOKEN_INVALID") {
            stopped = true;
            ws.close();
            link.onRevoked();
          }
          break;
      }
    };
    ws.onclose = () => {
      if (socket === ws) socket = null;
      if (stopped) return;
      if (ready) link.onLost();
      timer = setTimeout(open, retryMs);
      retryMs = Math.min(RETRY_MAX_MS, retryMs * 2);
    };
    ws.onerror = () => ws.close();
  }

  open();
  return () => {
    stopped = true;
    clearTimeout(timer);
    socket?.close();
  };
}
