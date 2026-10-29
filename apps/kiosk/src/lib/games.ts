// The Games tab's rules (Story 6.23): what the service's `games` and `game_launch` answers hold.
// Pure, so the page, the tab and their tests share them.

/** How often the page asks the service for the games and whether one is running. */
export const GAMES_POLL_MS = 3000;

/** One tile of the Games tab. */
export type GameCard = {
  id: string;
  name: string;
  /** The game this tile starts is running now. */
  running: boolean;
};

/** The service's `games` answer: the tiles, and whether any game is running. */
export type GamesView = { games: GameCard[]; running: boolean };

const isCard = (v: unknown): v is GameCard => {
  if (typeof v !== "object" || v === null) return false;
  const c = v as Record<string, unknown>;
  return typeof c.id === "string" && typeof c.name === "string" && typeof c.running === "boolean";
};

/** The view in a `games` answer body, or `null` when it is not one. */
export function gamesOf(body: unknown): GamesView | null {
  if (typeof body !== "object" || body === null) return null;
  const b = body as Record<string, unknown>;
  if (!Array.isArray(b.games) || typeof b.running !== "boolean") return null;
  if (!b.games.every(isCard)) return null;
  return {
    games: b.games.map(({ id, name, running }) => ({ id, name, running })),
    running: b.running,
  };
}

/** What a tile's activation did: it started, it was running already, or it did not start. */
export type LaunchResult = "started" | "already_running" | "failed";

/** The result in a `game_launch` answer body; anything unexpected is a failure. */
export function launchOf(body: unknown): LaunchResult {
  const outcome =
    typeof body === "object" && body !== null ? (body as Record<string, unknown>).outcome : null;
  return outcome === "started" || outcome === "already_running" ? outcome : "failed";
}
