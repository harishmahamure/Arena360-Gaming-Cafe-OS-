import type { GameMediaRef, LastPlayed, MostPlayed, MostPlayedGame } from "@arena/ui/generated";

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
  /** The library launcher (`steam`, `epic`, `riot`, `battlenet` or `none`); empty when unknown. */
  launcher: string;
  /** The library age rating (empty when none): the Home hero's "Game details" (Story 6.29). */
  ageRating: string;
};

/** The service's `games` answer: the tiles, and whether any game is running. */
export type GamesView = { games: GameCard[]; running: boolean };

const isCard = (v: unknown): v is GameCard => {
  if (typeof v !== "object" || v === null) return false;
  const c = v as Record<string, unknown>;
  return typeof c.id === "string" && typeof c.name === "string" && typeof c.running === "boolean";
};

const cardOf = (c: GameCard): GameCard => {
  const { id, name, running, category, tags, launcher, age_rating } = c as Record<string, unknown> &
    GameCard & { age_rating: unknown };
  return {
    id,
    name,
    running,
    category: typeof category === "string" ? category : "",
    tags: Array.isArray(tags) ? tags.filter((x): x is string => typeof x === "string") : [],
    launcher: typeof launcher === "string" ? launcher : "",
    ageRating: typeof age_rating === "string" ? age_rating : "",
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

const isMedia = (v: unknown): v is GameMediaRef => {
  if (typeof v !== "object" || v === null) return false;
  const g = v as Record<string, unknown>;
  const hash = (x: unknown) => x === null || x === undefined || typeof x === "string";
  return typeof g.game_id === "string" && hash(g.video) && hash(g.cover);
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
  const media = Array.isArray(b.media) ? b.media.filter(isMedia) : [];
  return { top: b.top.slice(0, MOST_PLAYED_LIMIT), last_played: last, media };
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

// Home (Story 6.29): the featured game, its hero media and the featured row.

/** The games the featured row shows at most, the featured one first; CSS cuts it per width. */
export const FEATURED_ROW_LIMIT = 8;

const DAY_MS = 24 * 3_600_000;

/** What Home features: a game, and whether it is the branch's most played of the week. */
export type Featured = {
  game: GameCard;
  /** The game leads the branch's last 7 days (otherwise it is the first library game with media). */
  mostPlayed: boolean;
};

/** The Hub's stored media of `game` (nothing when it has none or the Hub could not be asked). */
export function mediaOf(
  view: MostPlayedView | null,
  game: GameCard,
): { video: string | null; cover: string | null } {
  const found = view?.media?.find((m) => m.game_id === game.id);
  const url = (sha: string | null | undefined) => (sha ? `/media/${sha}` : null);
  return { video: url(found?.video), cover: url(found?.cover) };
}

/**
 * The game Home features: the branch's most-played game of the last 7 days that this PC has,
 * else the first library game that has media, else the first game. `null` for an empty library.
 */
export function featuredOf(view: MostPlayedView | null, games: GameCard[]): Featured | null {
  const [lead] = rankedGames(view, games);
  if (lead !== undefined) return { game: lead.game, mostPlayed: true };
  const withMedia = games.find((g) => {
    const m = mediaOf(view, g);
    return m.video !== null || m.cover !== null;
  });
  const game = withMedia ?? games[0];
  return game === undefined ? null : { game, mostPlayed: false };
}

/**
 * The featured row: the featured game, then the next most-played games this PC has, at most
 * `FEATURED_ROW_LIMIT` in all. The "All games" tile that ends the row is the view's.
 */
export function featuredRow(view: MostPlayedView | null, games: GameCard[]): GameCard[] {
  const featured = featuredOf(view, games);
  if (featured === null) return [];
  const rest = rankedGames(view, games)
    .map((r) => r.game)
    .filter((g) => g.id !== featured.game.id);
  return [featured.game, ...rest].slice(0, FEATURED_ROW_LIMIT);
}

/** When the live player last played `game` (Unix ms), or `null` when never (or unknown). */
export function lastPlayedAt(view: MostPlayedView | null, game: GameCard): number | null {
  return view?.last_played.find((l) => l.game_id === game.id)?.at ?? null;
}

/** Whole days between `at` and `now` (a time in the future counts as today). */
export const daysSince = (at: number, now: number): number =>
  Math.max(0, Math.floor((now - at) / DAY_MS));
