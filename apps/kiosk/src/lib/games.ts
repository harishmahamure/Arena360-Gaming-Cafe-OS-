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

/** The view in a `games` answer body, or `null` when it is not one. */
export function gamesOf(body: unknown): GamesView | null {
  if (typeof body !== "object" || body === null) return null;
  const b = body as Record<string, unknown>;
  if (!Array.isArray(b.games) || typeof b.running !== "boolean") return null;
  if (!b.games.every(isCard)) return null;
  return {
    games: b.games.map((c) => {
      const { id, name, running, category, tags } = c as Record<string, unknown> & GameCard;
      return {
        id,
        name,
        running,
        category: typeof category === "string" ? category : "",
        tags: Array.isArray(tags) ? tags.filter((x): x is string => typeof x === "string") : [],
      };
    }),
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
