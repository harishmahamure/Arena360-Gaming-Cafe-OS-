import type { LastPlayed, MostPlayed, MostPlayedGame } from "@arena/ui/generated";

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
  /** The library category (empty when none); a filter-rail item (Story 6.24). */
  category: string;
  /** The library tags; filter-rail items (Story 6.24). */
  tags: string[];
};

/** The service's `games` answer: the tiles, and whether any game is running. */
export type GamesView = { games: GameCard[]; running: boolean };

const isCard = (v: unknown): v is GameCard => {
  if (typeof v !== "object" || v === null) return false;
  const c = v as Record<string, unknown>;
  return typeof c.id === "string" && typeof c.name === "string" && typeof c.running === "boolean";
};

const cardOf = (c: GameCard): GameCard => {
  const { id, name, running, category, tags } = c as Record<string, unknown> & GameCard;
  return {
    id,
    name,
    running,
    category: typeof category === "string" ? category : "",
    tags: Array.isArray(tags) ? tags.filter((x): x is string => typeof x === "string") : [],
  };
};

/** The view in a `games` answer body, or `null` when it is not one. */
export function gamesOf(body: unknown): GamesView | null {
  if (typeof body !== "object" || body === null) return null;
  const b = body as Record<string, unknown>;
  if (!Array.isArray(b.games) || typeof b.running !== "boolean") return null;
  if (!b.games.every(isCard)) return null;
  return { games: b.games.map(cardOf), running: b.running };
}

/**
 * The tiles in an `apps` answer body (Story 6.28), or `null` when it is not one. An app tile has
 * the shape of a game tile; `running` there means the app is open, not that a game is running.
 */
export function appsOf(body: unknown): GameCard[] | null {
  if (typeof body !== "object" || body === null) return null;
  const apps = (body as Record<string, unknown>).apps;
  if (!Array.isArray(apps) || !apps.every(isCard)) return null;
  return apps.map(cardOf);
}

/** What a tile's activation did: it started, it was running already, or it did not start. */
export type LaunchResult = "started" | "already_running" | "failed";

/** The result in a `game_launch` answer body; anything unexpected is a failure. */
export function launchOf(body: unknown): LaunchResult {
  const outcome =
    typeof body === "object" && body !== null ? (body as Record<string, unknown>).outcome : null;
  return outcome === "started" || outcome === "already_running" ? outcome : "failed";
}

// The filter rail and the search field (Story 6.24). Everything is matched case-insensitively and
// a label is one rail item whether the library calls it a category or a tag.

const fold = (s: string) => s.trim().toLowerCase();

/** The rail's items after "All": every category and tag in the library, once each, sorted. */
export function railLabels(games: GameCard[]): string[] {
  const seen = new Map<string, string>();
  for (const g of games) {
    for (const label of [g.category, ...g.tags]) {
      const key = fold(label);
      if (key !== "" && !seen.has(key)) seen.set(key, label.trim());
    }
  }
  return [...seen.values()].sort((a, b) => fold(a).localeCompare(fold(b)));
}

/** Whether the game is in the category or carries the tag named `label`. */
export function inLabel(game: GameCard, label: string): boolean {
  const key = fold(label);
  return fold(game.category) === key || game.tags.some((t) => fold(t) === key);
}

/** The games the active rail item (`null`: All) and the search text leave, in their order. */
export function visibleGames(games: GameCard[], label: string | null, query: string): GameCard[] {
  const needle = fold(query);
  return games.filter(
    (g) =>
      (label === null || inLabel(g, label)) && (needle === "" || fold(g.name).includes(needle)),
  );
}

// The launcher cards (Story 6.26): the row "Play on your own account" above the games.

/** The launchers the Games tab can open, in the order the cards appear. */
export const LAUNCHER_IDS = ["steam", "epic", "riot"] as const;
export type LauncherId = (typeof LAUNCHER_IDS)[number];

/** One launcher card: an installed launcher, and whether it is open now. */
export type LauncherCardData = { id: LauncherId; name: string; running: boolean };

const isLauncher = (v: unknown): v is LauncherCardData => {
  if (typeof v !== "object" || v === null) return false;
  const c = v as Record<string, unknown>;
  return (
    typeof c.id === "string" &&
    (LAUNCHER_IDS as readonly string[]).includes(c.id) &&
    typeof c.name === "string" &&
    typeof c.running === "boolean"
  );
};

/** The cards in a `launchers` answer body, in Steam, Epic, Riot order; `null` when it is not one. */
export function launchersOf(body: unknown): LauncherCardData[] | null {
  if (typeof body !== "object" || body === null) return null;
  const list = (body as Record<string, unknown>).launchers;
  if (!Array.isArray(list) || !list.every(isLauncher)) return null;
  return [...list].sort((a, b) => LAUNCHER_IDS.indexOf(a.id) - LAUNCHER_IDS.indexOf(b.id));
}

// "Most played here" (Story 6.27): the branch's top games of the last 7 days, from the Hub.

/** How often the open Games tab asks again; the Hub itself recomputes at most every 15 minutes. */
export const MOST_PLAYED_POLL_MS = 5 * 60_000;

/** The carousel never shows more than this many tiles. */
export const MOST_PLAYED_LIMIT = 10;

/** The Hub's `games.most_played_result`: its top games, and when this player last played each. */
export type MostPlayedView = MostPlayed;

const isTop = (v: unknown): v is MostPlayedGame => {
  if (typeof v !== "object" || v === null) return false;
  const g = v as Record<string, unknown>;
  return typeof g.game_id === "string" && typeof g.minutes === "number";
};

const isLast = (v: unknown): v is LastPlayed => {
  if (typeof v !== "object" || v === null) return false;
  const g = v as Record<string, unknown>;
  return typeof g.game_id === "string" && typeof g.at === "number";
};

/** The view in a `most_played` answer body, or `null` when it is not one. */
export function mostPlayedOf(body: unknown): MostPlayedView | null {
  if (typeof body !== "object" || body === null) return null;
  const b = body as Record<string, unknown>;
  if (!Array.isArray(b.top) || !b.top.every(isTop)) return null;
  const last = Array.isArray(b.last_played) ? b.last_played.filter(isLast) : [];
  return { top: b.top.slice(0, MOST_PLAYED_LIMIT), last_played: last };
}

/** A carousel tile: a game of this PC's library and its place among the tiles shown. */
export type RankedGame = { rank: number; game: GameCard };

/**
 * The tiles the carousel shows: the Hub's top games that this PC has, in the Hub's order. A game
 * not installed here is skipped and the next one moves up, so the ranks ("#1"...) number the tiles
 * shown, not the Hub's list. Empty (no carousel) when the window has no minutes or the Hub could
 * not be asked (`null`).
 */
export function rankedGames(view: MostPlayedView | null, games: GameCard[] | null): RankedGame[] {
  if (view === null || games === null) return [];
  const here = new Map(games.map((g) => [g.id, g]));
  const ranked: RankedGame[] = [];
  for (const { game_id, minutes } of view.top) {
    const game = here.get(game_id);
    if (game === undefined || minutes <= 0) continue;
    ranked.push({ rank: ranked.length + 1, game });
  }
  return ranked;
}
