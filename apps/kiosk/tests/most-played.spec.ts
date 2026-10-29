import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import names from "../src-tauri/ipc-names.json" with { type: "json" };
import { mostPlayedOf, rankedGames, type GameCard, type MostPlayedView } from "../src/lib/games";

// Story 6.27: the "Most played here" carousel. The Tauri shell is stubbed in the page; `__calls`
// records what the page asked of it and `__service` holds what the Kiosk service would answer to
// `games` and `most_played` (`mostPlayed: null` is a Hub that cannot be asked).

type Game = { id: string; name: string; running: boolean; category: string; tags: string[] };
type Top = { game_id: string; minutes: number };
type Options = { games?: Game[]; top?: Top[] | null; branch?: string };

const BRANCH = "Arena Koramangala";

async function openGames(page: Page, options: Options = {}) {
  await page.addInitScript(
    ([commands, opts]: [typeof names.commands, Options & { branch: string }]) => {
      const callbacks = new Map<number, (e: unknown) => void>();
      const listeners: Record<string, number[]> = {};
      let next = 1;
      const w = window as unknown as Record<string, unknown>;
      w.__calls = [] as unknown[];
      w.__asked = 0;
      const service = {
        games: opts.games ?? [],
        top: opts.top === undefined ? [] : opts.top,
      };
      w.__service = service;
      w.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
        unregisterListener: (event: string, id: number) => {
          listeners[event] = (listeners[event] ?? []).filter((x) => x !== id);
        },
      };
      w.__TAURI_INTERNALS__ = {
        transformCallback: (cb: (e: unknown) => void) => {
          const id = next++;
          callbacks.set(id, cb);
          return id;
        },
        invoke: async (cmd: string, args: Record<string, unknown>) => {
          switch (cmd) {
            case "plugin:event|listen":
              (listeners[args.event as string] ??= []).push(args.handler as number);
              return args.handler;
            case "plugin:event|unlisten":
              return null;
            case commands.games: {
              const games = service.games as Game[];
              return { ok: true, body: { games, running: games.some((g) => g.running) } };
            }
            case commands.launchers:
              return { ok: true, body: { launchers: [] } };
            case commands.mostPlayed: {
              (w.__asked as number)++;
              if (service.top === null) {
                return { ok: false, error: { code: "HUB_SLOW", message: "unreachable" } };
              }
              return { ok: true, body: { top: service.top, last_played: [] } };
            }
            case commands.gameLaunch: {
              (w.__calls as unknown[]).push(["game_launch", args.id, args.name]);
              return { ok: true, body: { outcome: "started", pid: 4242 } };
            }
            case commands.attract:
              return {
                view: {
                  branch_name: opts.branch,
                  state: "ready",
                  locale: { currency: "INR", exponent: 2, symbol: "₹", tz: "Asia/Kolkata" },
                  rate_minor: null,
                  rule: null,
                  media: [],
                },
                logo: null,
              };
            case commands.staffMessages:
              return [];
            case commands.pcScreen:
            case commands.hubStatus:
            case commands.takeRestoreState:
            case commands.reportAppState:
            case commands.heartbeat:
            case commands.adminExitStatus:
            case commands.theme:
            case commands.volumeGet:
              return null;
            default:
              throw new Error(`unexpected command ${cmd}`);
          }
        },
      };
      w.__shell = {
        emit: (event: string, payload: unknown) => {
          for (const id of listeners[event] ?? []) callbacks.get(id)?.({ event, id, payload });
        },
        listening: (event: string) => (listeners[event] ?? []).length > 0,
      };
    },
    [names.commands, { ...options, branch: options.branch ?? BRANCH }] as [
      typeof names.commands,
      Options & { branch: string },
    ],
  );
  await page.goto("/");
  await page.waitForFunction(
    (event) =>
      (window as unknown as { __shell: { listening: (e: string) => boolean } }).__shell.listening(
        event,
      ),
    names.events.pcScreen,
  );
  await page.evaluate(() =>
    (window as unknown as { __shell: { emit: (e: string, p: unknown) => void } }).__shell.emit(
      "pc-screen",
      {
        locked: false,
        reason: null,
        session: { device_name: "PC 7", elapsed_ms: 0, username: "asha", remaining_ms: 3_600_000 },
      },
    ),
  );
  await expect(page.getByTestId("shell")).toBeVisible();
}

const calls = (page: Page) =>
  page.evaluate(() => (window as unknown as { __calls: unknown[] }).__calls);

const game = (id: string, name: string): Game => ({
  id,
  name,
  running: false,
  category: "",
  tags: [],
});

// Twelve games on this PC, G1..G12.
const LIBRARY: Game[] = Array.from({ length: 12 }, (_, i) => game(`G${i + 1}`, `Game ${i + 1}`));
const top = (...ids: string[]): Top[] =>
  ids.map((game_id, i) => ({ game_id, minutes: 1000 - i * 10 }));

const serious = async (page: Page) =>
  (await new AxeBuilder({ page }).analyze()).violations.filter(
    (v) => v.impact === "serious" || v.impact === "critical",
  );

test("the carousel sits above the grid with its title, caption and ranked tiles in the Hub's order", async ({
  page,
}) => {
  await openGames(page, { games: LIBRARY, top: top("G7", "G3", "G11") });
  await page.getByTestId("tab-games").click();
  const carousel = page.getByTestId("most-played-carousel");
  await expect(carousel).toBeVisible();
  await expect(
    carousel.getByRole("heading", { name: en["player.games.most_played_title"] }),
  ).toBeVisible();
  await expect(page.getByTestId("most-played-caption")).toHaveText(
    `Top 10 by play hours · last 7 days · ${BRANCH}`,
  );
  const tiles = carousel.getByTestId("game-tile");
  await expect(tiles).toHaveCount(3);
  await expect(carousel.getByTestId("game-tile-name")).toHaveText(["Game 7", "Game 3", "Game 11"]);
  await expect(carousel.getByTestId("game-tile-rank")).toHaveText(["#1", "#2", "#3"]);
  await expect(tiles.first()).toHaveAccessibleName("#1 Game 7");

  const above = await page.evaluate(() => {
    const c = document
      .querySelector('[data-testid="most-played-carousel"]')!
      .getBoundingClientRect();
    const g = document.querySelector('[data-testid="games-grid"]')!.getBoundingClientRect();
    return c.bottom <= g.top;
  });
  expect(above).toBe(true);
  // The grid still lists every game, ranked or not.
  await expect(page.getByTestId("games-grid").getByTestId("game-tile")).toHaveCount(12);
  await expect(page.getByTestId("games-grid").getByTestId("game-tile-rank")).toHaveCount(0);
});

test("a rank is a bg badge with text ink at the tile's top-left", async ({ page }) => {
  await openGames(page, { games: LIBRARY, top: top("G1") });
  await page.getByTestId("tab-games").click();
  const badge = page.getByTestId("most-played-carousel").getByTestId("game-tile-rank");
  await expect(badge).toBeVisible();
  const [bg, text, backgroundColor, color] = await badge.evaluate((el) => {
    const probe = (name: string) => {
      const p = document.createElement("i");
      p.style.color = `var(${name})`;
      document.body.append(p);
      const v = getComputedStyle(p).color;
      p.remove();
      return v;
    };
    const s = getComputedStyle(el);
    return [probe("--bg"), probe("--text"), s.backgroundColor, s.color];
  });
  expect(backgroundColor).toBe(bg);
  expect(color).toBe(text);
  const corner = await badge.evaluate((el) => {
    const b = el.getBoundingClientRect();
    const t = el.closest("button")!.getBoundingClientRect();
    return { dx: b.left - t.left, dy: b.top - t.top };
  });
  expect(corner.dx).toBeLessThan(20);
  expect(corner.dy).toBeLessThan(20);
});

test("a top game that is not installed here is skipped and the next one moves up", async ({
  page,
}) => {
  // G99 is the branch's first but not on this PC.
  await openGames(page, { games: LIBRARY, top: top("G99", "G4", "G98", "G2") });
  await page.getByTestId("tab-games").click();
  const carousel = page.getByTestId("most-played-carousel");
  await expect(carousel.getByTestId("game-tile-name")).toHaveText(["Game 4", "Game 2"]);
  await expect(carousel.getByTestId("game-tile-rank")).toHaveText(["#1", "#2"]);
});

test("no carousel when the window has no plays, none are installed here or the Hub cannot be asked", async ({
  page,
}) => {
  await openGames(page, { games: LIBRARY, top: [] });
  await page.getByTestId("tab-games").click();
  await expect(page.getByTestId("games-grid")).toBeVisible();
  await expect
    .poll(() => page.evaluate(() => (window as unknown as { __asked: number }).__asked))
    .toBeGreaterThan(0);
  await expect(page.getByTestId("most-played-carousel")).toHaveCount(0);

  const elsewhere = await page.context().newPage();
  await openGames(elsewhere, { games: LIBRARY, top: top("X1", "X2") });
  await elsewhere.getByTestId("tab-games").click();
  await expect(elsewhere.getByTestId("games-grid")).toBeVisible();
  await expect(elsewhere.getByTestId("most-played-carousel")).toHaveCount(0);

  const offline = await page.context().newPage();
  await openGames(offline, { games: LIBRARY, top: null });
  await offline.getByTestId("tab-games").click();
  await expect(offline.getByTestId("games-grid")).toBeVisible();
  await expect(offline.getByTestId("most-played-carousel")).toHaveCount(0);
});

test("a carousel holds at most ten tiles", async ({ page }) => {
  await openGames(page, {
    games: LIBRARY,
    top: top("G1", "G2", "G3", "G4", "G5", "G6", "G7", "G8", "G9", "G10", "G11", "G12"),
  });
  await page.getByTestId("tab-games").click();
  const tiles = page.getByTestId("most-played-carousel").getByTestId("game-tile");
  await expect(tiles).toHaveCount(10);
  await expect(page.getByTestId("most-played-carousel").getByTestId("game-tile-rank")).toHaveText(
    Array.from({ length: 10 }, (_, i) => `#${i + 1}`),
  );
});

test("activating a ranked tile launches the game through game_launch", async ({ page }) => {
  await openGames(page, { games: LIBRARY, top: top("G5", "G6") });
  await page.getByTestId("tab-games").click();
  await page.getByTestId("most-played-carousel").getByTestId("game-tile").nth(1).click();
  await expect.poll(() => calls(page)).toEqual([["game_launch", "G6", "Game 6"]]);
});

test("Left and Right move focus tile to tile and scroll it into view", async ({ page }) => {
  await page.setViewportSize({ width: 1366, height: 768 });
  await openGames(page, {
    games: LIBRARY,
    top: top("G1", "G2", "G3", "G4", "G5", "G6", "G7", "G8", "G9", "G10"),
  });
  await page.getByTestId("tab-games").click();
  const tiles = page.getByTestId("most-played-carousel").getByTestId("game-tile");
  await expect(tiles).toHaveCount(10);
  const row = page.getByTestId("most-played-row");
  expect(await row.evaluate((el) => el.scrollWidth > el.clientWidth)).toBe(true);
  expect(await row.evaluate((el) => el.scrollLeft)).toBe(0);

  await tiles.first().focus();
  for (let i = 1; i < 10; i++) {
    await page.keyboard.press("ArrowRight");
    await expect(tiles.nth(i)).toBeFocused();
  }
  // The last tile is inside the row's visible part, so the row scrolled.
  expect(await row.evaluate((el) => el.scrollLeft)).toBeGreaterThan(0);
  const inView = await tiles.last().evaluate((el) => {
    const r = el.getBoundingClientRect();
    const c = el.closest("ul")!.getBoundingClientRect();
    return r.left >= c.left - 1 && r.right <= c.right + 1;
  });
  expect(inView).toBe(true);
  // At the end the key does nothing more; Left walks back.
  await page.keyboard.press("ArrowRight");
  await expect(tiles.last()).toBeFocused();
  await page.keyboard.press("ArrowLeft");
  await expect(tiles.nth(8)).toBeFocused();
});

test("the carousel passes the accessibility check", async ({ page }) => {
  await openGames(page, { games: LIBRARY, top: top("G7", "G3") });
  await page.getByTestId("tab-games").click();
  await expect(page.getByTestId("most-played-carousel")).toBeVisible();
  expect(await serious(page)).toEqual([]);
});

test("the answer is read strictly and the ranks number the tiles shown", () => {
  expect(mostPlayedOf({ top: [], last_played: [] })).toEqual({ top: [], last_played: [] });
  expect(
    mostPlayedOf({
      top: [{ game_id: "A", minutes: 5 }],
      last_played: [{ game_id: "A", at: 9 }, { nope: 1 }],
    }),
  ).toEqual({ top: [{ game_id: "A", minutes: 5 }], last_played: [{ game_id: "A", at: 9 }] });
  for (const bad of [null, "x", {}, { top: "A" }, { top: [{ game_id: "A" }] }]) {
    expect(mostPlayedOf(bad)).toBeNull();
  }
  const card = (id: string): GameCard => ({
    id,
    name: id,
    running: false,
    category: "",
    tags: [],
  });
  const view: MostPlayedView = {
    top: [
      { game_id: "X", minutes: 90 },
      { game_id: "B", minutes: 50 },
      { game_id: "Z", minutes: 0 },
      { game_id: "A", minutes: 10 },
    ],
    last_played: [],
  };
  expect(
    rankedGames(view, [card("A"), card("B"), card("Z")]).map((r) => [r.rank, r.game.id]),
  ).toEqual([
    [1, "B"],
    [2, "A"],
  ]);
  expect(rankedGames(null, [card("A")])).toEqual([]);
  expect(rankedGames(view, null)).toEqual([]);
  expect(rankedGames({ top: [], last_played: [] }, [card("A")])).toEqual([]);
});
