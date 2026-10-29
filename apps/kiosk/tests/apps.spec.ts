import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import names from "../src-tauri/ipc-names.json" with { type: "json" };
import { appsOf } from "../src/lib/games";

// Story 6.28: the Apps tab. The Tauri shell is stubbed in the page; `__calls` records what the
// page asked of it and `__service` holds what the Kiosk service would answer to `apps` and
// `app_launch` (opening an app flips its `running` flag, the way the service's list would). Games
// and launchers are stubbed too, to prove the Apps tab shows neither the launcher cards nor the
// carousel.

type App = { id: string; name: string; running: boolean; category?: string; tags?: string[] };
type Options = { apps?: App[] | null; outcome?: string };

async function openApps(page: Page, options: Options = {}) {
  await page.addInitScript(
    ([commands, opts]: [typeof names.commands, Options]) => {
      const callbacks = new Map<number, (e: unknown) => void>();
      const listeners: Record<string, number[]> = {};
      let next = 1;
      const w = window as unknown as Record<string, unknown>;
      w.__calls = [] as unknown[];
      const service = {
        apps: opts.apps === undefined ? [] : opts.apps,
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
            case commands.apps: {
              const apps = service.apps as App[] | null;
              return apps === null ? null : { ok: true, body: { apps } };
            }
            case commands.appLaunch: {
              (w.__calls as unknown[]).push(["app_launch", args.id, args.name]);
              const apps = service.apps as App[] | null;
              if (service.outcome === "started" && apps) {
                for (const a of apps) if (a.id === args.id) a.running = true;
              }
              return { ok: true, body: { outcome: service.outcome, pid: 4242 } };
            }
            case commands.games:
              return {
                ok: true,
                body: { games: [{ id: "G1", name: "Apex", running: false }], running: false },
              };
            case commands.launchers:
              return {
                ok: true,
                body: { launchers: [{ id: "steam", name: "Steam", running: false }] },
              };
            case commands.mostPlayed:
              return { ok: true, body: { top: [{ game_id: "G1", minutes: 90 }], last_played: [] } };
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

const LIBRARY: App[] = [
  { id: "A1", name: "Chrome", running: false, category: "Browser", tags: ["web"] },
  { id: "A2", name: "Discord", running: false, category: "Social", tags: ["chat", "web"] },
  { id: "A3", name: "Spotify", running: false, category: "Music", tags: [] },
];

const serious = async (page: Page) =>
  (await new AxeBuilder({ page }).analyze()).violations.filter(
    (v) => v.impact === "serious" || v.impact === "critical",
  );

test("the Apps tab shows the filter rail, search and a tile for each app, with no launcher cards or carousel", async ({
  page,
}) => {
  await openApps(page, { apps: LIBRARY });
  await page.getByTestId("tab-apps").click();
  await expect(page.getByTestId("apps")).toBeVisible();
  await expect(page.getByTestId("game-tile-name")).toHaveText(["Chrome", "Discord", "Spotify"]);
  await expect(page.getByTestId("filter-rail")).toBeVisible();
  await expect(page.getByTestId("filter-item")).toHaveText([
    en["player.games.filter_all"],
    "Browser",
    "chat",
    "Music",
    "Social",
    "web",
  ]);
  await expect(page.getByTestId("games-search")).toHaveAttribute(
    "aria-label",
    en["player.apps.search_label"],
  );
  await expect(
    page.getByRole("heading", { name: en["player.apps.title"], level: 2 }),
  ).toBeVisible();
  await expect(page.getByTestId("launchers")).toHaveCount(0);
  await expect(page.getByTestId("most-played-carousel")).toHaveCount(0);
  expect(await serious(page)).toEqual([]);
});

test("the Games tab still shows the games, not the apps", async ({ page }) => {
  await openApps(page, { apps: LIBRARY });
  await page.getByTestId("tab-games").click();
  await expect(page.getByTestId("game-tile-name").first()).toHaveText("Apex");
  await expect(page.getByTestId("launchers")).toBeVisible();
  await expect(page.getByTestId("apps")).toHaveCount(0);
});

test("filter and search work as on Games", async ({ page }) => {
  await openApps(page, { apps: LIBRARY });
  await page.getByTestId("tab-apps").click();
  await page.getByTestId("filter-item").filter({ hasText: "web" }).click();
  await expect(page.getByTestId("game-tile-name")).toHaveText(["Chrome", "Discord"]);
  await page.getByTestId("games-search").fill("DISC");
  await expect(page.getByTestId("game-tile-name")).toHaveText(["Discord"]);
  await page.getByTestId("games-search").fill("zzz");
  await expect(page.getByTestId("games-no-match")).toContainText(en["player.apps.no_match"]);
  await page.getByTestId("games-clear").click();
  await expect(page.getByTestId("game-tile")).toHaveCount(3);
});

test("Enter or a click on an app tile opens that app, and the list is asked for again", async ({
  page,
}) => {
  await openApps(page, { apps: LIBRARY });
  await page.getByTestId("tab-apps").click();
  const tiles = page.getByTestId("game-tile");
  await tiles.nth(1).focus();
  await page.keyboard.press("Enter");
  await expect.poll(() => calls(page)).toEqual([["app_launch", "A2", "Discord"]]);
  await tiles.nth(2).click();
  await expect
    .poll(() => calls(page))
    .toEqual([
      ["app_launch", "A2", "Discord"],
      ["app_launch", "A3", "Spotify"],
    ]);
  // The service now says both are open: the tiles show it. An open app is not a game running.
  await expect(tiles.nth(1)).toHaveAttribute("data-running", "true");
  await expect(tiles.nth(2)).toHaveAttribute("data-running", "true");
  await expect(tiles.nth(0)).toHaveAttribute("data-running", "false");
  await expect(tiles.nth(0)).toHaveAttribute("data-game-running", "false");
  expect(await serious(page)).toEqual([]);
});

test("a second activation of an open app asks the service again and nothing else changes", async ({
  page,
}) => {
  await openApps(page, {
    apps: [{ id: "A1", name: "Chrome", running: true }],
    outcome: "already_running",
  });
  await page.getByTestId("tab-apps").click();
  await page.getByTestId("game-tile").click();
  await expect.poll(() => calls(page)).toEqual([["app_launch", "A1", "Chrome"]]);
  await expect(page.getByTestId("game-tile")).toHaveCount(1);
});

test("no apps on this PC says so", async ({ page }) => {
  await openApps(page, { apps: [] });
  await page.getByTestId("tab-apps").click();
  await expect(page.getByTestId("games-empty")).toHaveText(en["player.shell.empty"]);
  await expect(page.getByTestId("filter-rail")).toHaveCount(0);
  await expect(page.getByTestId("game-tile")).toHaveCount(0);
});

test("until the service has answered the Apps tab shows neither tiles nor the empty line", async ({
  page,
}) => {
  await openApps(page, { apps: null });
  await page.getByTestId("tab-apps").click();
  await expect(page.getByTestId("apps")).toBeAttached();
  await expect(page.getByTestId("games-empty")).toHaveCount(0);
  await expect(page.getByTestId("game-tile")).toHaveCount(0);
});

test("the answers are read defensively", () => {
  expect(appsOf({ apps: [{ id: "a", name: "A", running: false, extra: 1 }] })).toEqual([
    { id: "a", name: "A", running: false, category: "", tags: [], launcher: "", ageRating: "" },
  ]);
  for (const bad of [
    null,
    "x",
    {},
    { apps: "no" },
    { games: [], running: false },
    { apps: [{ id: "a", name: "A" }] },
  ]) {
    expect(appsOf(bad)).toBeNull();
  }
});
