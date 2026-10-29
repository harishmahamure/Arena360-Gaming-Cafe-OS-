import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import names from "../src-tauri/ipc-names.json" with { type: "json" };
import {
  daysSince,
  featuredOf,
  featuredRow,
  gamesOf,
  mediaOf,
  mostPlayedOf,
  type GameCard,
  type MostPlayedView,
} from "../src/lib/games";

// Story 6.29: Home. The Tauri shell is stubbed in the page; `__calls` records what the page asked
// of it and `__service` holds what the Kiosk service and the Hub would answer to `games` and
// `most_played`. A launch marks the game running, as the service's next `games` answer would.

type Game = {
  id: string;
  name: string;
  running: boolean;
  category: string;
  tags: string[];
  launcher: string;
  age_rating: string;
};
type Options = {
  games?: Game[];
  top?: { game_id: string; minutes: number }[] | null;
  last_played?: { game_id: string; at: number }[];
  media?: { game_id: string; video: string | null; cover: string | null }[];
  branch?: string;
};

const BRANCH = "Arena Koramangala";
const DAY = 86_400_000;
const sha = (letter: string) => letter.repeat(64);
const bytesOf = (base64: string) => Uint8Array.from(atob(base64), (c) => c.charCodeAt(0));
const asBody = (bytes: Uint8Array) =>
  (globalThis as unknown as { Buffer: { from(b: Uint8Array): never } }).Buffer.from(bytes);
const PNG = bytesOf(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==",
);

/** A short VP8 clip recorded in the browser, so no binary lives in the repo. */
async function recordWebm(page: Page): Promise<Uint8Array> {
  const base64 = await page.evaluate(async () => {
    const canvas = document.createElement("canvas");
    canvas.width = 64;
    canvas.height = 36;
    const ctx = canvas.getContext("2d")!;
    const recorder = new MediaRecorder(canvas.captureStream(15), {
      mimeType: "video/webm;codecs=vp8",
    });
    const chunks: Blob[] = [];
    recorder.ondataavailable = (e) => chunks.push(e.data);
    const stopped = new Promise((resolve) => (recorder.onstop = resolve));
    recorder.start();
    let n = 0;
    const timer = setInterval(() => {
      ctx.fillStyle = `hsl(${n++ * 30}, 80%, 50%)`;
      ctx.fillRect(0, 0, 64, 36);
    }, 66);
    await new Promise((resolve) => setTimeout(resolve, 900));
    clearInterval(timer);
    recorder.stop();
    await stopped;
    const bytes = new Uint8Array(await new Blob(chunks).arrayBuffer());
    let binary = "";
    for (const b of bytes) binary += String.fromCharCode(b);
    return btoa(binary);
  });
  return bytesOf(base64);
}

/** Answers `/media/<letter × 64>`: `v` is a video, `c` a cover; everything else is missing. */
async function serveMedia(page: Page) {
  await page.goto("/");
  const webm = await recordWebm(page);
  await page.route(/\/media\/([a-f])\1{63}$/, (route: Route) => {
    const letter = route.request().url().slice(-1);
    if (letter === "a")
      return route.fulfill({ status: 200, contentType: "video/webm", body: asBody(webm) });
    if (letter === "b")
      return route.fulfill({ status: 200, contentType: "image/png", body: asBody(PNG) });
    return route.fulfill({ status: 404, body: "" });
  });
}

async function openHome(page: Page, options: Options = {}) {
  await page.addInitScript(
    ([commands, opts]: [typeof names.commands, Options & { branch: string }]) => {
      const callbacks = new Map<number, (e: unknown) => void>();
      const listeners: Record<string, number[]> = {};
      let next = 1;
      const w = window as unknown as Record<string, unknown>;
      w.__calls = [] as unknown[];
      const service = {
        games: opts.games ?? [],
        top: opts.top === undefined ? [] : opts.top,
        last_played: opts.last_played ?? [],
        media: opts.media ?? [],
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
            case commands.apps:
              return { ok: true, body: { apps: [] } };
            case commands.mostPlayed: {
              if (service.top === null) {
                return { ok: false, error: { code: "HUB_SLOW", message: "unreachable" } };
              }
              return {
                ok: true,
                body: {
                  top: service.top,
                  last_played: service.last_played,
                  media: service.media,
                },
              };
            }
            case commands.gameLaunch: {
              (w.__calls as unknown[]).push(["game_launch", args.id]);
              const game = (service.games as Game[]).find((g) => g.id === args.id);
              if (game) game.running = true;
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

const game = (id: string, name: string, extra: Partial<Game> = {}): Game => ({
  id,
  name,
  running: false,
  category: "",
  tags: [],
  launcher: "none",
  age_rating: "",
  ...extra,
});

const LIBRARY: Game[] = [
  game("G1", "Apex Legends", { category: "Shooter", launcher: "steam", age_rating: "16+" }),
  game("G2", "Dota 2", { category: "MOBA", launcher: "steam", age_rating: "12+" }),
  game("G3", "Valorant", { category: "Shooter", launcher: "riot", age_rating: "16+" }),
  game("G4", "Fortnite", { category: "Battle royale", launcher: "epic" }),
  game("G5", "Minecraft"),
  game("G6", "Rocket League", { launcher: "epic" }),
  game("G7", "Overwatch", { launcher: "battlenet" }),
  game("G8", "Chess"),
];
const top = (...ids: string[]) => ids.map((game_id, i) => ({ game_id, minutes: 1000 - i * 10 }));

const serious = async (page: Page) =>
  (await new AxeBuilder({ page }).analyze()).violations.filter(
    (v) => v.impact === "serious" || v.impact === "critical",
  );

test("the hero shows the branch's most played game with eyebrow, title, meta and both buttons", async ({
  page,
}) => {
  await openHome(page, {
    games: LIBRARY,
    top: top("G3", "G1"),
    last_played: [{ game_id: "G3", at: Date.now() - 2 * DAY - 3_600_000 }],
  });
  await expect(page.getByTestId("hero-eyebrow")).toHaveText(`Most played at ${BRANCH} this week`, {
    ignoreCase: true,
  });
  await expect(page.getByTestId("hero-title")).toHaveText("Valorant");
  await expect(page.getByTestId("hero-meta")).toHaveText(
    "Shooter · Riot Games · You last played 2 days ago",
  );
  await expect(page.getByTestId("hero-play")).toHaveText("Play now");
  await expect(page.getByTestId("hero-details")).toHaveText("Game details");

  // The copy stays in the scrim's zone (left 45% × bottom 50%) and is at most 864px wide.
  const [vw, vh] = [1920, 1080];
  const copy = (await page.getByTestId("hero-copy").boundingBox())!;
  expect(copy.x).toBeGreaterThanOrEqual(0);
  expect(copy.x + copy.width).toBeLessThanOrEqual(vw * 0.45 + 1);
  expect(copy.y).toBeGreaterThanOrEqual(vh * 0.5 - 1);
  expect(copy.width).toBeLessThanOrEqual(864);
  const type = await page.getByTestId("hero-title").evaluate((el) => {
    const s = getComputedStyle(el);
    return [s.fontSize, s.fontWeight];
  });
  expect(type).toEqual(["144px", "200"]);
  expect(await serious(page)).toEqual([]);
});

test("the hero is at most 720px wide at 1366", async ({ page }) => {
  await page.setViewportSize({ width: 1366, height: 768 });
  await openHome(page, { games: LIBRARY, top: top("G3") });
  const copy = (await page.getByTestId("hero-copy").boundingBox())!;
  expect(copy.width).toBeLessThanOrEqual(720);
  expect(copy.x + copy.width).toBeLessThanOrEqual(1366 * 0.45 + 1);
});

test("a game this player never played has no last-played part", async ({ page }) => {
  await openHome(page, { games: LIBRARY, top: top("G1"), last_played: [] });
  await expect(page.getByTestId("hero-meta")).toHaveText("Shooter · Steam");
});

test("a launcher-less game with no genre has no meta line at all", async ({ page }) => {
  await openHome(page, { games: [game("A", "Solo")], top: top("A") });
  await expect(page.getByTestId("hero-title")).toHaveText("Solo");
  await expect(page.getByTestId("hero-meta")).toHaveCount(0);
});

test("with no plays the hero falls back to the first library game with media", async ({ page }) => {
  await openHome(page, {
    games: LIBRARY,
    top: [],
    media: [{ game_id: "G4", video: null, cover: sha("b") }],
  });
  await expect(page.getByTestId("hero-title")).toHaveText("Fortnite");
  await expect(page.getByTestId("hero-eyebrow")).toHaveText(`Featured at ${BRANCH}`, {
    ignoreCase: true,
  });
});

test("with no plays and no media the hero is the first game on solid bg", async ({ page }) => {
  await openHome(page, { games: LIBRARY, top: null });
  await expect(page.getByTestId("hero-title")).toHaveText("Apex Legends");
  await expect(page.getByTestId("hero-media")).toHaveAttribute("data-stage", "solid");
});

test("Play now launches the game and the hero video stops entirely", async ({ page }) => {
  await serveMedia(page);
  await openHome(page, {
    games: LIBRARY,
    top: top("G3"),
    media: [{ game_id: "G3", video: sha("a"), cover: sha("b") }],
  });
  const media = page.getByTestId("hero-media");
  await expect(media).toHaveAttribute("data-playing", "true");
  await expect(media.locator("video")).toHaveCount(1);
  await page.getByTestId("hero-play").click();
  expect(await calls(page)).toEqual([["game_launch", "G3"]]);
  await expect(media).toHaveAttribute("data-stage", "cover");
  await expect(media.locator("video")).toHaveCount(0);
  await expect(page.getByTestId("video-control")).toHaveCount(0);
});

test("Game details opens a card with genre, age rating and launcher; Esc closes it and refocuses", async ({
  page,
}) => {
  await openHome(page, { games: LIBRARY, top: top("G3") });
  const button = page.getByTestId("hero-details");
  await expect(page.getByTestId("game-details")).toHaveCount(0);
  await button.click();
  const panel = page.getByTestId("game-details");
  await expect(panel).toBeVisible();
  await expect(button).toHaveAttribute("aria-expanded", "true");
  await expect(panel.getByTestId("details-genre")).toHaveText("Shooter");
  await expect(panel.getByTestId("details-age")).toHaveText("16+");
  await expect(panel.getByTestId("details-launcher")).toHaveText("Riot Games");
  await expect(panel.getByTestId("details-close")).toBeFocused();
  expect(await serious(page)).toEqual([]);
  await page.keyboard.press("Escape");
  await expect(panel).toHaveCount(0);
  await expect(button).toBeFocused();
  await expect(button).toHaveAttribute("aria-expanded", "false");
});

test("Game details says 'Not listed' for what the library does not hold", async ({ page }) => {
  await openHome(page, { games: [game("A", "Solo")], top: top("A") });
  await page.getByTestId("hero-details").click();
  const panel = page.getByTestId("game-details");
  await expect(panel.getByTestId("details-genre")).toHaveText(en["player.home.details_unknown"]);
  await expect(panel.getByTestId("details-age")).toHaveText(en["player.home.details_unknown"]);
  await expect(panel.getByTestId("details-launcher")).toHaveText(en["player.home.launcher.none"]);
});

test("the featured row leads with the 190×260 featured tile, then the next most played, then All games", async ({
  page,
}) => {
  await openHome(page, {
    games: LIBRARY,
    top: top("G3", "G1", "G2", "G9", "G5", "G4", "G6", "G7"),
    media: [{ game_id: "G3", video: null, cover: sha("b") }],
  });
  const row = page.getByTestId("featured-row");
  await expect(row).toBeVisible();
  const lead = row.getByTestId("game-tile-featured");
  await expect(lead).toHaveCount(1);
  await expect(lead).toHaveAccessibleName("Valorant, featured");
  const box = (await lead.boundingBox())!;
  expect([Math.round(box.width), Math.round(box.height)]).toEqual([190, 260]);
  // The outline is the row's mark: 3px magenta, 3px out from the tile, not a focus ring.
  const outline = await row.locator("li.lead").evaluate((el) => {
    const s = getComputedStyle(el, "::after");
    const probe = document.createElement("i");
    probe.style.color = "var(--accent)";
    document.body.append(probe);
    const accent = getComputedStyle(probe).color;
    probe.remove();
    return [s.borderTopWidth, s.borderTopStyle, s.borderTopColor === accent, s.top];
  });
  expect(outline).toEqual(["3px", "solid", true, "-6px"]);
  // 1920 wide: the featured tile and five more, then All games.
  const tiles = row.locator("button.game-tile:visible");
  await expect(tiles).toHaveCount(6);
  expect(await tiles.evaluateAll((els) => els.map((e) => e.getAttribute("aria-label")))).toEqual([
    "Valorant, featured",
    "Apex Legends",
    "Dota 2",
    "Minecraft",
    "Fortnite",
    "Rocket League",
  ]);
  const all = row.getByTestId("all-games-tile");
  await expect(all).toHaveText(en["player.home.all_games"]);
  const last = (await all.boundingBox())!;
  const prev = (await tiles.last().boundingBox())!;
  expect(last.x).toBeGreaterThan(prev.x);
  // The other tiles are 140×196.
  const small = (await tiles.nth(1).boundingBox())!;
  expect([Math.round(small.width), Math.round(small.height)]).toEqual([140, 196]);
});

test("the featured row is 4 tiles + All games at 1366 and 8 at 2560", async ({ page }) => {
  const library = Array.from({ length: 12 }, (_, i) => game(`G${i + 1}`, `Game ${i + 1}`));
  const ids = library.map((g) => g.id);
  await page.setViewportSize({ width: 1366, height: 768 });
  await openHome(page, { games: library, top: top(...ids) });
  const tiles = page.getByTestId("featured-row").locator("button.game-tile:visible");
  await expect(tiles).toHaveCount(4);
  await expect(page.getByTestId("all-games-tile")).toBeVisible();
  await page.setViewportSize({ width: 2560, height: 1440 });
  await expect(tiles).toHaveCount(8);
  await expect(page.getByTestId("all-games-tile")).toBeVisible();
});

test("All games opens the Games tab with no filter", async ({ page }) => {
  await openHome(page, { games: LIBRARY, top: top("G3") });
  await page.getByTestId("all-games-tile").click();
  await expect(page.getByTestId("tab-games")).toHaveAttribute("aria-current", "page");
  await expect(page.getByTestId("games-grid").getByTestId("game-tile")).toHaveCount(8);
  await expect(page.getByTestId("filter-item").first()).toHaveAttribute("aria-pressed", "true");
});

test("a featured-row tile launches its game", async ({ page }) => {
  await openHome(page, { games: LIBRARY, top: top("G3", "G1") });
  await page.getByTestId("featured-row").getByRole("button", { name: "Apex Legends" }).click();
  expect(await calls(page)).toEqual([["game_launch", "G1"]]);
});

test("Left and Right move focus along the featured row", async ({ page }) => {
  await openHome(page, { games: LIBRARY, top: top("G3", "G1", "G2") });
  const row = page.getByTestId("featured-row");
  await row.getByTestId("game-tile-featured").focus();
  await page.keyboard.press("ArrowRight");
  await expect(row.getByRole("button", { name: "Apex Legends" })).toBeFocused();
  await page.keyboard.press("ArrowLeft");
  await expect(row.getByTestId("game-tile-featured")).toBeFocused();
});

test("pausing the hero holds its frame for the rest of the session's Home view", async ({
  page,
}) => {
  await serveMedia(page);
  await openHome(page, {
    games: LIBRARY,
    top: top("G3"),
    media: [{ game_id: "G3", video: sha("a"), cover: sha("b") }],
  });
  const media = page.getByTestId("hero-media");
  await expect(media).toHaveAttribute("data-playing", "true");
  const control = page.getByTestId("video-control");
  // Bottom-right of the hero.
  const box = (await control.boundingBox())!;
  expect(box.x + box.width).toBeGreaterThan(1920 / 2);
  expect(box.y).toBeGreaterThan(1080 / 2);
  await control.click();
  await expect(control).toHaveAccessibleName(en["ui.game_media.play"]);
  await expect(media.locator("video")).toHaveJSProperty("paused", true);
  // Leaving Home and coming back keeps the pause.
  await page.getByTestId("tab-games").click();
  await page.getByTestId("tab-home").click();
  await expect(page.getByTestId("hero-media").locator("video")).toHaveCount(1);
  await expect(page.getByTestId("video-control")).toHaveAccessibleName(en["ui.game_media.play"]);
  await expect(page.getByTestId("hero-media").locator("video")).toHaveJSProperty("paused", true);
});

test("a library with no games shows the empty line on solid bg and no Play now", async ({
  page,
}) => {
  await openHome(page, { games: [], top: [] });
  await expect(page.getByTestId("home-empty")).toHaveText(en["player.shell.empty"]);
  await expect(page.getByTestId("hero-media")).toHaveAttribute("data-stage", "solid");
  await expect(page.getByTestId("hero-play")).toHaveCount(0);
  await expect(page.getByTestId("hero-details")).toHaveCount(0);
  await expect(page.getByTestId("featured-row")).toHaveCount(0);
});

test("the top bar and device name stay above and clickable over the hero", async ({ page }) => {
  await openHome(page, { games: LIBRARY, top: top("G3") });
  await expect(page.getByTestId("session-device")).toBeVisible();
  const hit = await page.evaluate(() => {
    const el = document.querySelector('[data-testid="tab-games"]')!;
    const r = el.getBoundingClientRect();
    return document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2) === el;
  });
  expect(hit).toBe(true);
});

test("pure rules: the featured game, the row, media, days", () => {
  const card = (id: string): GameCard => ({
    id,
    name: id,
    running: false,
    category: "",
    tags: [],
    launcher: "",
    ageRating: "",
  });
  const cards = ["A", "B", "C", "D"].map(card);
  const view: MostPlayedView = {
    top: [
      { game_id: "X", minutes: 90 },
      { game_id: "C", minutes: 50 },
      { game_id: "A", minutes: 10 },
    ],
    last_played: [{ game_id: "C", at: 5 }],
    media: [{ game_id: "D", video: "d".repeat(64), cover: null }],
  };
  // The most played game this PC has (X is not here).
  expect(featuredOf(view, cards)).toEqual({ game: cards[2], mostPlayed: true });
  expect(featuredRow(view, cards).map((g) => g.id)).toEqual(["C", "A"]);
  // No plays: the first game with media, else the first game.
  const quiet = { ...view, top: [] };
  expect(featuredOf(quiet, cards)).toEqual({ game: cards[3], mostPlayed: false });
  expect(featuredRow(quiet, cards).map((g) => g.id)).toEqual(["D"]);
  expect(featuredOf({ ...quiet, media: [] }, cards)?.game.id).toBe("A");
  expect(featuredOf(null, cards)?.game.id).toBe("A");
  expect(featuredOf(null, [])).toBeNull();
  expect(featuredRow(null, [])).toEqual([]);
  // The row never passes eight games.
  const many = Array.from({ length: 12 }, (_, i) => card(`G${i}`));
  const wide: MostPlayedView = {
    top: many.map((g, i) => ({ game_id: g.id, minutes: 100 - i })),
    last_played: [],
    media: [],
  };
  expect(featuredRow(wide, many)).toHaveLength(8);
  expect(mediaOf(view, cards[3])).toEqual({ video: `/media/${"d".repeat(64)}`, cover: null });
  expect(mediaOf(view, cards[0])).toEqual({ video: null, cover: null });
  expect(mediaOf(null, cards[0])).toEqual({ video: null, cover: null });
  expect(daysSince(1000, 1000 + 2 * DAY + 5)).toBe(2);
  expect(daysSince(1000, 1000 + DAY - 1)).toBe(0);
  expect(daysSince(5000, 1000)).toBe(0);
  // The wire shape: media arrives as hashes, a Hub without the field means none.
  expect(
    mostPlayedOf({
      top: [],
      last_played: [],
      media: [{ game_id: "A", video: null, cover: "x" }, { nope: 1 }],
    })?.media,
  ).toEqual([{ game_id: "A", video: null, cover: "x" }]);
  expect(mostPlayedOf({ top: [], last_played: [] })?.media).toEqual([]);
  expect(
    gamesOf({
      games: [{ id: "A", name: "A", running: false, launcher: "steam", age_rating: "16+" }],
      running: false,
    })?.games[0],
  ).toMatchObject({ launcher: "steam", ageRating: "16+", category: "", tags: [] });
});
