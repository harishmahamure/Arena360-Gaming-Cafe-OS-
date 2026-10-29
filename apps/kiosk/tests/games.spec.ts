import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import names from "../src-tauri/ipc-names.json" with { type: "json" };
import { gamesOf, launchOf } from "../src/lib/games";

// Story 6.23: the Games tab. The Tauri shell is stubbed in the page; `__calls` records what the
// page asked of it and `__service` holds what the Kiosk service would answer to `games` and
// `game_launch` (starting a game flips its `running` flag, the way the service's list would).

type Game = { id: string; name: string; running: boolean };
type Options = { games?: Game[] | null; outcome?: string };

async function openGames(page: Page, options: Options = {}) {
  await page.addInitScript(
    ([commands, opts]: [typeof names.commands, Options]) => {
      const callbacks = new Map<number, (e: unknown) => void>();
      const listeners: Record<string, number[]> = {};
      let next = 1;
      const w = window as unknown as Record<string, unknown>;
      w.__calls = [] as unknown[];
      w.__listed = 0;
      const service = {
        games: opts.games === undefined ? [] : opts.games,
        outcome: opts.outcome ?? "started",
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
              (w.__listed as number)++;
              const games = service.games as Game[] | null;
              if (games === null) return null;
              return { ok: true, body: { games, running: games.some((g) => g.running) } };
            }
            case commands.gameLaunch: {
              (w.__calls as unknown[]).push(["game_launch", args.id, args.name]);
              const games = service.games as Game[] | null;
              if (service.outcome === "started" && games) {
                for (const g of games) if (g.id === args.id) g.running = true;
              }
              return { ok: true, body: { outcome: service.outcome, pid: 4242 } };
            }
            case commands.staffMessages:
              return [];
            case commands.pcScreen:
            case commands.attract:
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
    [names.commands, options] as [typeof names.commands, Options],
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

const LIBRARY: Game[] = [
  { id: "G1", name: "Apex Legends", running: false },
  { id: "G2", name: "Counter-Strike 2", running: false },
  { id: "G3", name: "Valorant", running: false },
];

const serious = async (page: Page) =>
  (await new AxeBuilder({ page }).analyze()).violations.filter(
    (v) => v.impact === "serious" || v.impact === "critical",
  );

test("the Games tab shows a game-tile for every game the service lists, in its order", async ({
  page,
}) => {
  await openGames(page, { games: LIBRARY });
  await expect(page.getByTestId("games")).toHaveCount(0);
  await page.getByTestId("tab-games").click();
  const tiles = page.getByTestId("game-tile");
  await expect(tiles).toHaveCount(3);
  await expect(page.getByTestId("game-tile-name")).toHaveText([
    "Apex Legends",
    "Counter-Strike 2",
    "Valorant",
  ]);
  await expect(page.getByTestId("games-empty")).toHaveCount(0);
  expect(await serious(page)).toEqual([]);
});

test("a tile is 140 × 196 with a radius of lg and, with no cover, the name centred on surface", async ({
  page,
}) => {
  await openGames(page, { games: LIBRARY });
  await page.getByTestId("tab-games").click();
  const tile = page.getByTestId("game-tile").first();
  const box = (await tile.boundingBox())!;
  expect(Math.round(box.width)).toBe(140);
  expect(Math.round(box.height)).toBe(196);
  const style = await tile.evaluate((el) => {
    const s = getComputedStyle(el);
    return {
      radius: s.borderTopLeftRadius,
      surface: getComputedStyle(document.documentElement).getPropertyValue("--surface").trim(),
    };
  });
  expect(style.radius).toBe("20px");
  const name = (await tile.getByTestId("game-tile-name").boundingBox())!;
  const centre = (b: { x: number; width: number }) => b.x + b.width / 2;
  expect(Math.abs(centre(name) - centre(box))).toBeLessThan(2);
  const media = await tile
    .getByTestId("game-tile-media")
    .evaluate((el) => getComputedStyle(el).backgroundColor);
  const probe = await page.evaluate((c) => {
    const el = document.createElement("i");
    el.style.color = c;
    document.body.append(el);
    const v = getComputedStyle(el).color;
    el.remove();
    return v;
  }, style.surface);
  expect(media).toBe(probe);
});

test("Enter or a click on a tile launches that game, and the list is asked for again", async ({
  page,
}) => {
  await openGames(page, { games: LIBRARY });
  await page.getByTestId("tab-games").click();
  const tiles = page.getByTestId("game-tile");
  await tiles.nth(1).focus();
  await page.keyboard.press("Enter");
  await expect.poll(() => calls(page)).toEqual([["game_launch", "G2", "Counter-Strike 2"]]);
  await tiles.nth(2).click();
  await expect
    .poll(() => calls(page))
    .toEqual([
      ["game_launch", "G2", "Counter-Strike 2"],
      ["game_launch", "G3", "Valorant"],
    ]);
  // The service now says both run: the tiles show it.
  await expect(tiles.nth(1)).toHaveAttribute("data-running", "true");
  await expect(tiles.nth(2)).toHaveAttribute("data-running", "true");
  await expect(tiles.nth(0)).toHaveAttribute("data-running", "false");
  await expect(tiles.nth(1)).toHaveAccessibleName(
    en["player.games.tile_running"].replace("{name}", "Counter-Strike 2"),
  );
  await expect(page.getByTestId("game-tile-running")).toHaveCount(2);
  expect(await serious(page)).toEqual([]);
});

test("the running signal reaches every game-media and drops when the last game exits", async ({
  page,
}) => {
  await openGames(page, { games: LIBRARY });
  await page.getByTestId("tab-games").click();
  const tiles = page.getByTestId("game-tile");
  for (let i = 0; i < 3; i++)
    await expect(tiles.nth(i)).toHaveAttribute("data-game-running", "false");
  await tiles.first().click();
  for (let i = 0; i < 3; i++)
    await expect(tiles.nth(i)).toHaveAttribute("data-game-running", "true");
  // The game exits; the next poll of the service clears the signal.
  await page.evaluate(() => {
    const service = (window as unknown as { __service: { games: Game[] } }).__service;
    for (const g of service.games) g.running = false;
  });
  await expect(tiles.first()).toHaveAttribute("data-game-running", "false", { timeout: 8000 });
});

test("a second activation of a running game asks the service again and nothing else changes", async ({
  page,
}) => {
  await openGames(page, { games: LIBRARY, outcome: "already_running" });
  await page.getByTestId("tab-games").click();
  const tile = page.getByTestId("game-tile").first();
  await tile.click();
  await tile.click();
  await expect.poll(async () => (await calls(page)).length).toBe(2);
  await expect(page.getByTestId("game-tile")).toHaveCount(3);
});

test("a library with no games here says so", async ({ page }) => {
  await openGames(page, { games: [] });
  await page.getByTestId("tab-games").click();
  await expect(page.getByTestId("games-empty")).toHaveText(en["player.shell.empty"]);
  await expect(page.getByTestId("game-tile")).toHaveCount(0);
});

test("until the service has answered the tab shows neither tiles nor the empty line", async ({
  page,
}) => {
  await openGames(page, { games: null });
  await page.getByTestId("tab-games").click();
  await expect(page.getByTestId("games")).toBeAttached();
  await expect(page.getByTestId("games-empty")).toHaveCount(0);
  await expect(page.getByTestId("game-tile")).toHaveCount(0);
});

test("tiles are keyboard reachable in order and keep a focus ring", async ({ page }) => {
  await openGames(page, { games: LIBRARY });
  await page.getByTestId("tab-games").click();
  const tiles = page.getByTestId("game-tile");
  await tiles.first().focus();
  await page.keyboard.press("Tab");
  await expect(tiles.nth(1)).toBeFocused();
  const outline = await tiles.nth(1).evaluate((el) => getComputedStyle(el).outlineStyle);
  expect(outline).toBe("solid");
});

test("the answers are read defensively", () => {
  expect(
    gamesOf({ games: [{ id: "a", name: "A", running: false, extra: 1 }], running: true }),
  ).toEqual({ games: [{ id: "a", name: "A", running: false }], running: true });
  for (const bad of [
    null,
    "x",
    {},
    { games: [], running: "yes" },
    { games: "no", running: false },
    { games: [{ id: "a", name: "A" }], running: false },
  ]) {
    expect(gamesOf(bad)).toBeNull();
  }
  expect(launchOf({ outcome: "started" })).toBe("started");
  expect(launchOf({ outcome: "already_running", pid: 3 })).toBe("already_running");
  for (const bad of [null, {}, { outcome: "failed" }, { outcome: "later" }, "started"]) {
    expect(launchOf(bad)).toBe("failed");
  }
});
