// The kitchen tablet's side of the Hub (Story 7.6): the status and pairing endpoints, the stored
// `kds` token and the `/ws` connection. The page keeps the token in `localStorage` (never in a
// URL) and sends it only in the `hello` frame. Once connected it asks the Hub for the board
// (`kitchen.board`) and takes every whole board the Hub pushes after a ticket changes (Story 7.7).
// A tap moves a ticket on with `kitchen.advance` and Undo takes it back with
// `kitchen.advance.reverted` (Story 7.8). There is no chime or queueing yet (Stories 7.9, 7.10).

import type {
  Frame,
  KdsPairResponse,
  KdsStatus,
  KitchenBoard,
  SettingEntry,
} from "@arena/ui/generated";
import { ulid } from "./ulid";

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

/** How a ticket move ended: the Hub applied it, or refused it with an error code (`OFFLINE` when no link). */
export type Move = { ok: true } | { ok: false; code: string };

/** What a connected display can ask of the Hub. */
export type Connection = {
  close: () => void;
  /** Moves a ticket to `to` (the next stage) as the ticket stood at `baseRev`. `key` is minted at the tap. */
  advance: (ticketId: string, to: string, baseRev: number, key: string) => Promise<Move>;
  /** Takes the latest advance of a ticket back; `baseRev` is the rev that advance left. */
  revert: (ticketId: string, baseRev: number) => Promise<Move>;
  /** Asks the Hub for the whole board again, to snap to its state. */
  refresh: () => void;
};

export type Link = {
  /** The Hub accepted the token. */
  onReady: () => void;
  /** The token is revoked or unknown: clear it and pair again. */
  onRevoked: () => void;
  /** This display's high-contrast setting, on connect and whenever it changes. */
  onHighContrast: (on: boolean) => void;
  /** The whole board: the reply to the request, then every push after a ticket changed. */
  onBoard: (board: KitchenBoard) => void;
  /** The socket is gone (the link reconnects by itself). */
  onLost: () => void;
};

/** The board fields of a `kitchen.board_result` frame or a `kitchen.board` event. */
function boardOf(body: KitchenBoard): KitchenBoard {
  return { branch_name: body.branch_name, now_ms: body.now_ms, tickets: body.tickets };
}

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
export function connect(token: string, link: Link): Connection {
  const displayId = displayIdOf(token);
  let socket: WebSocket | null = null;
  let stopped = false;
  let retryMs = RETRY_MIN_MS;
  let timer: ReturnType<typeof setTimeout> | undefined;

  const send = (frame: Record<string, unknown>): boolean => {
    if (socket?.readyState !== WebSocket.OPEN) return false;
    socket.send(JSON.stringify({ v: 1, ...frame }));
    return true;
  };

  // Replies to the intents this link submitted, by request id.
  const waiting = new Map<string, (move: Move) => void>();
  const settleAll = (move: Move) => {
    for (const resolve of waiting.values()) resolve(move);
    waiting.clear();
  };

  function submit(kind: string, payload: Record<string, unknown>, baseRev: number, key: string) {
    return new Promise<Move>((resolve) => {
      const rid = ulid();
      waiting.set(rid, resolve);
      const sent = send({
        rid,
        kind: "intent.submit",
        intent: {
          intent_id: ulid(),
          kind,
          idempotency_key: key,
          issued_at: Date.now(),
          base_rev: baseRev,
          // The Hub overwrites the actor with this session's.
          actor: { kind: "device", id: displayId ?? "" },
          payload: { kind, ...payload },
        },
      });
      if (!sent) {
        waiting.delete(rid);
        resolve({ ok: false, code: "OFFLINE" });
      }
    });
  }

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
          send({ rid: "board", kind: "kitchen.board" });
          break;
        case "intent.result": {
          const resolve = waiting.get(frame.rid);
          waiting.delete(frame.rid);
          resolve?.({ ok: true });
          break;
        }
        case "kitchen.board_result":
          link.onBoard(boardOf(frame));
          break;
        case "ping":
          send({ rid: frame.rid, kind: "pong" });
          break;
        case "settings.values":
          takeValues(frame.values);
          break;
        case "event":
          if (frame.event.kind === "settings.changed") takeValues(frame.event.values);
          else if (frame.event.kind === "kitchen.board") link.onBoard(boardOf(frame.event));
          break;
        case "error":
          if (waiting.has(frame.rid)) {
            const resolve = waiting.get(frame.rid);
            waiting.delete(frame.rid);
            resolve?.({ ok: false, code: frame.error.code });
            break;
          }
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
      settleAll({ ok: false, code: "OFFLINE" });
      if (stopped) return;
      if (ready) link.onLost();
      timer = setTimeout(open, retryMs);
      retryMs = Math.min(RETRY_MAX_MS, retryMs * 2);
    };
    ws.onerror = () => ws.close();
  }

  open();
  return {
    close: () => {
      stopped = true;
      clearTimeout(timer);
      socket?.close();
    },
    advance: (ticketId, to, baseRev, key) =>
      submit("kitchen.advance", { ticket_id: ticketId, to }, baseRev, key),
    revert: (ticketId, baseRev) =>
      submit("kitchen.advance.reverted", { ticket_id: ticketId }, baseRev, ulid()),
    refresh: () => void send({ rid: ulid(), kind: "kitchen.board" }),
  };
}
