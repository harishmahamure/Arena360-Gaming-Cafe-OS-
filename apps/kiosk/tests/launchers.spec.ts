import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import names from "../src-tauri/ipc-names.json" with { type: "json" };
import { launchersOf } from "../src/lib/games";

// Story 6.26: the launcher cards. The Tauri shell is stubbed in the page; `__calls` records what
// the page asked of it and `__service` holds what the Kiosk service would answer to `launchers`
// and `launcher_open` (opening a launcher flips its `running` flag, the way the service's list
// would).

type Launcher = { id: string; name: string; running: boolean };
type Options = { launchers?: Launcher[]; outcome?: string };

async function openGames(page: Page, options: Options = {}) {
  await page.addInitScript(
    ([commands, opts]: [typeof names.commands, Options]) => {
      const callbacks = new Map<number, (e: unknown) => void>();
      const listeners: Record<string, number[]> = {};
      let next = 1;
      const w = window as unknown as Record<string, unknown>;
      w.__calls = [] as unknown[];
      const service = {
        launchers: opts.launchers ?? [],
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
            case commands.games:
              return { ok: true, body: { games: [], running: false } };
            case commands.launchers:
              return { ok: true, body: { launchers: service.launchers } };
            case commands.launcherOpen: {
              (w.__calls as unknown[]).push(["launcher_open", args.id, args.name]);
              if (service.outcome === "started") {
                for (const l of service.launchers as Launcher[]) {
                  if (l.id === args.id) l.running = true;
                }
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

const ALL: Launcher[] = [
  { id: "steam", name: "Steam", running: false },
  { id: "epic", name: "Epic Games", running: false },
  { id: "riot", name: "Riot Client", running: false },
];

const serious = async (page: Page) =>
  (await new AxeBuilder({ page }).analyze()).violations.filter(
    (v) => v.impact === "serious" || v.impact === "critical",
  );

test("a launcher-card row shows each installed launcher with its name and the own-account line", async ({
  page,
}) => {
  await openGames(page, { launchers: ALL });
  await page.getByTestId("tab-games").click();
  await expect(page.getByTestId("launchers")).toBeVisible();
  await expect(
    page.getByRole("heading", { name: en["player.games.launchers_title"] }),
  ).toBeVisible();
  await expect(page.getByTestId("launcher-card")).toHaveCount(3);
  await expect(page.getByTestId("launcher-card-name")).toHaveText([
    "Steam",
    "Epic Games",
    "Riot Client",
  ]);
  await expect(page.getByTestId("launcher-card-caption")).toHaveText(
    Array(3).fill(en["player.games.launcher_caption"]),
  );
  await expect(page.getByTestId("launcher-card-mark")).toHaveText(["S", "E", "R"]);
  expect(await serious(page)).toEqual([]);
});

test("a launcher that is not installed has no card and no installed launcher means no row", async ({
  page,
}) => {
  await openGames(page, { launchers: [ALL[1]] });
  await page.getByTestId("tab-games").click();
  await expect(page.getByTestId("launcher-card")).toHaveCount(1);
  await expect(page.getByTestId("launcher-card")).toHaveAttribute("data-launcher", "epic");

  const none = await page.context().newPage();
  await openGames(none, { launchers: [] });
  await none.getByTestId("tab-games").click();
  await expect(none.getByTestId("games")).toBeVisible();
  await expect(none.getByTestId("launchers")).toHaveCount(0);
  await expect(none.getByTestId("launcher-card")).toHaveCount(0);
});

test("a card is a surface card with a radius of lg and a visible focus ring", async ({ page }) => {
  await openGames(page, { launchers: ALL });
  await page.getByTestId("tab-games").click();
  const card = page.getByTestId("launcher-card").first();
  await expect(card).toHaveCSS("border-top-left-radius", "20px");
  const surface = await page.evaluate(() =>
    getComputedStyle(document.documentElement).getPropertyValue("--surface").trim(),
  );
  const background = await card.evaluate((el) => getComputedStyle(el).backgroundColor);
  expect(surface).not.toBe("");
  expect(background).not.toBe("rgba(0, 0, 0, 0)");
  await card.focus();
  await expect(card).toHaveCSS("outline-style", "solid");
});

test("activating a card asks the service to open that launcher by id, and only by id", async ({
  page,
}) => {
  await openGames(page, { launchers: ALL });
  await page.getByTestId("tab-games").click();
  await page.getByTestId("launcher-card").nth(1).click();
  await expect.poll(() => calls(page)).toEqual([["launcher_open", "epic", "Epic Games"]]);
  // The card now says the launcher is open.
  const epic = page.locator('[data-testid="launcher-card"][data-launcher="epic"]');
  await expect(epic).toHaveAttribute("data-running", "true");
  await expect(epic.getByTestId("launcher-card-open")).toHaveText(en["player.games.launcher_open"]);
  await expect(epic).toHaveAccessibleName(
    `Epic Games ${en["player.games.launcher_caption"]} ${en["player.games.launcher_open"]}`,
  );
  await expect(
    page.locator('[data-testid="launcher-card"][data-launcher="steam"]'),
  ).toHaveAttribute("data-running", "false");
});

test("Enter on a focused card opens it, and an open launcher asked again is brought forward", async ({
  page,
}) => {
  await openGames(page, { launchers: ALL, outcome: "already_running" });
  await page.getByTestId("tab-games").click();
  const steam = page.locator('[data-testid="launcher-card"][data-launcher="steam"]');
  await steam.focus();
  await page.keyboard.press("Enter");
  await expect.poll(() => calls(page)).toEqual([["launcher_open", "steam", "Steam"]]);
  // The service said it was open already: the shell brings the window forward; no second copy is
  // the service's rule, and the card keeps whatever the service lists.
  await steam.focus();
  await page.keyboard.press("Space");
  await expect.poll(async () => (await calls(page)).length).toBe(2);
});

test("a launcher that is open at the next poll shows as open", async ({ page }) => {
  await openGames(page, { launchers: ALL });
  await page.getByTestId("tab-games").click();
  await expect(page.getByTestId("launcher-card-open")).toHaveCount(0);
  await page.evaluate(() => {
    const s = (window as unknown as { __service: { launchers: Launcher[] } }).__service;
    s.launchers[0].running = true;
  });
  await expect(page.getByTestId("launcher-card-open")).toHaveCount(1, { timeout: 8000 });
});

test("the launcher answer is read strictly and listed Steam, Epic, Riot", () => {
  expect(launchersOf({ launchers: [] })).toEqual([]);
  expect(
    launchersOf({
      launchers: [
        { id: "riot", name: "Riot Client", running: false },
        { id: "steam", name: "Steam", running: true },
      ],
    })?.map((l) => l.id),
  ).toEqual(["steam", "riot"]);
  for (const bad of [
    null,
    "x",
    {},
    { launchers: "steam" },
    { launchers: [{ id: "discord", name: "Discord", running: false }] },
    { launchers: [{ id: "steam", name: "Steam" }] },
    { launchers: [{ id: "steam", name: 5, running: false }] },
  ]) {
    expect(launchersOf(bad)).toBeNull();
  }
});
