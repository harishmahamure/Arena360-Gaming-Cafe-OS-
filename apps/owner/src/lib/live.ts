import type {
  LiveFloor,
  LiveGrant,
  LiveTile,
  LiveTileState,
  OwnerBranch,
} from "@arena/ui/generated";
import { t } from "@arena/ui";
import type { MessageKey } from "@arena/ui/messages";
/** The floor-tile states (the shared `Status` of the tile component). */
type Status = "free" | "in-session" | "paused" | "reserved" | "maintenance" | "offline";
import { probe } from "./api";

/** The token is replaced on the open socket this long after it was issued (it lives 5 minutes). */
export const REFRESH_MS = 4 * 60_000;
/** The floor is called live while the newest frame is no older than this. */
export const LIVE_AGE_MS = 3_000;
/** The tunnel flag that entitles a branch to live view. */
export const TUNNEL_FLAG = "tunnel";

/** What the owner is offered for a branch: the view, the tier lock or the Windows Hub note. */
export type Entry = "ready" | "tier" | "platform";

export function entryOf(b: OwnerBranch): Entry {
  if (b.tier === "free" || !b.flags.includes(TUNNEL_FLAG)) return "tier";
  return b.hub.platform && b.hub.platform !== "windows" ? "platform" : "ready";
}

export type GrantAnswer =
  | { state: "ok"; grant: LiveGrant }
  | { state: "tier" }
  | { state: "platform" }
  | { state: "offline" }
  | { state: "signed-out" };

/** Asks the cloud for a live token and the Hub's address through the tunnel. */
export async function fetchGrant(branchId: string): Promise<GrantAnswer> {
  const res = await probe(`/live/${encodeURIComponent(branchId)}`);
  if (res === "signed-out") return { state: "signed-out" };
  if (!res) return { state: "offline" };
  const code = (res.data as { code?: string } | null)?.code;
  if (res.status === 200 && res.data) return { state: "ok", grant: res.data as LiveGrant };
  if (code === "FEATURE_NOT_ENTITLED") return { state: "tier" };
  if (code === "PLATFORM_UNSUPPORTED") return { state: "platform" };
  return { state: "offline" };
}

/** What the socket tells the screen. */
export type LiveEvents = {
  /** The Hub admitted the viewer. */
  open: () => void;
  floor: (floor: LiveFloor) => void;
  /** The connection ended (or never opened): the Hub is not reachable through the tunnel. */
  closed: () => void;
};

const frame = (kind: string, rid: string, rest: Record<string, unknown> = {}) =>
  JSON.stringify({ v: 1, kind, rid, ...rest });

export type LiveConnection = { close: () => void };

/**
 * Opens the Hub's WebSocket with `grant`, reads the pushed floor and renews the token every
 * [`REFRESH_MS`] over the same socket (`ask` fetches the next grant), so the view never drops.
 */
export function connectLive(
  grant: LiveGrant,
  ask: () => Promise<GrantAnswer>,
  events: LiveEvents,
  open: (url: string) => WebSocket = (url) => new WebSocket(url),
): LiveConnection {
  let done = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const ws = open(grant.url);
  const end = () => {
    if (done) return;
    done = true;
    clearTimeout(timer);
    events.closed();
  };
  const renew = async () => {
    if (done) return;
    const next = await ask();
    if (done) return;
    if (next.state === "ok" && ws.readyState === WebSocket.OPEN)
      ws.send(frame("live.token", `t-${Date.now()}`, { token: next.grant.token })); // i18n-ignore
    // The old token still runs for a minute; try again in half of that.
    timer = setTimeout(renew, next.state === "ok" ? REFRESH_MS : 30_000);
  };
  ws.onopen = () =>
    ws.send(
      frame("hello", "h", {
        credential: grant.token,
        class: "live_view",
        versions: { app: "0.1.0" },
        capabilities: [],
      }),
    );
  ws.onmessage = (m) => {
    let f: { kind?: string; rid?: string; event?: { kind?: string } & Partial<LiveFloor> };
    try {
      f = JSON.parse(String(m.data));
    } catch {
      return;
    }
    if (f.kind === "hello.ok") {
      timer = setTimeout(renew, REFRESH_MS);
      events.open();
    } else if (f.kind === "ping") ws.send(frame("pong", f.rid ?? ""));
    else if (f.kind === "event" && f.event?.kind === "live.floor") {
      const { kind: _kind, ...floor } = f.event;
      events.floor(floor as LiveFloor);
    }
  };
  ws.onclose = end;
  ws.onerror = end;
  return {
    close: () => {
      done = true;
      clearTimeout(timer);
      ws.onclose = ws.onerror = ws.onmessage = null;
      ws.close();
    },
  };
}

/** The Hub's clock now, from the newest frame (immune to the owner's device clock). */
export const hubNow = (floor: LiveFloor, receivedAt: number, now: number) =>
  floor.generated_at + Math.max(0, now - receivedAt);

const two = (n: number) => String(n).padStart(2, "0");
const clock = (s: number) =>
  [Math.floor(s / 3600), two(Math.floor(s / 60) % 60), two(s % 60)].join(":");

export type TileView = {
  state: Status;
  label: MessageKey;
  /** The running time or countdown, or the grace/time-up text. */
  time?: string;
  warning: boolean;
  /** "Player 01J…" or "Guest 3". */
  who?: string;
};

const IDLE: Record<LiveTileState, [Status, MessageKey]> = {
  online: ["free", "ui.status.free"],
  offline: ["offline", "ui.status.offline"],
  active: ["in-session", "ui.status.in_session"],
  paused: ["paused", "ui.status.paused"],
  grace: ["in-session", "ui.status.in_session"],
  expired: ["in-session", "ui.status.in_session"],
  reserved: ["reserved", "ui.status.reserved"],
  maintenance: ["maintenance", "ui.status.maintenance"],
};
const WARN_MS = 10 * 60_000;

/** What one tile shows at Hub time `at`. */
export function tileView(tile: LiveTile, at: number): TileView {
  const [state, label] = IDLE[tile.state];
  const s = tile.session;
  if (!s) return { state, label, warning: false };
  const who = s.player_id
    ? t("owner.live.player", { id: s.player_id })
    : s.guest_no != null
      ? t("hub.session.guest", { n: s.guest_no })
      : undefined;
  let time: string;
  let warning = false;
  if (tile.state === "grace" && s.grace_ends_at != null) {
    const left = Math.max(0, Math.ceil((s.grace_ends_at - at) / 1000));
    time = t("hub.floor.grace", { time: `${Math.floor(left / 60)}:${two(left % 60)}` }); // i18n-ignore
    warning = true;
  } else if (tile.state === "expired") {
    time = t("hub.floor.time_up");
    warning = true;
  } else if (tile.state === "active" && s.ends_at != null) {
    time = clock(Math.max(0, Math.ceil((s.ends_at - at) / 1000)));
    warning = s.ends_at - at <= WARN_MS;
  } else {
    const end = s.paused_at ?? at;
    time = clock(Math.max(0, Math.floor((end - s.started_at - s.paused_total_ms) / 1000)));
  }
  return { state, label, time, warning, who };
}

/** The tiles of each zone in zone order, then the Unassigned ones. */
export function byZone(floor: LiveFloor): { id: string; name: string | null; tiles: LiveTile[] }[] {
  const known = new Set(floor.zones.map((z) => z.id));
  const groups = floor.zones.map((z) => ({
    id: z.id,
    name: z.name as string | null,
    tiles: floor.tiles.filter((x) => x.zone === z.id),
  }));
  const loose = floor.tiles.filter((x) => !x.zone || !known.has(x.zone));
  return [...groups, ...(loose.length ? [{ id: "", name: null, tiles: loose }] : [])].filter(
    (g) => g.tiles.length,
  );
}
