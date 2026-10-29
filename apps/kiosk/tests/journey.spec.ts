import { expect, test, type Page } from "@playwright/test";
import names from "../src-tauri/ipc-names.json" with { type: "json" };
import { ATTRACT, PREPAID, answer, emit, openKiosk, shown } from "./support";

// Story 6.38: Journey 3 on the kiosk UI, timed. attract → login → plan picker → Home → launch →
// warning → Extend → grace → clean-up → attract, on the shell window and the HUD window the way
// the real shell runs them side by side (both are told the same `pc-screen`). Each leg is the
// time the page takes between what the player does or the service says and what the player
// sees; the Hub's own legs (login, approval, Extend, an order on the tile) are timed over the
// LAN in `arena-api` (`the_hub_side_of_journey_3_meets_its_timings`). `scripts/journey-report.mjs`
// adds the two and checks the story's budgets; run on the reference PC this is the slow-hardware
// half. The tests are run with the stubbed shell, so no leg waits on a Hub.

const env = (globalThis as { process?: { env: Record<string, string | undefined> } }).process!.env;

/** The longest any one UI leg may take: the story's budget unless `JOURNEY_UI_LEG_MS` says so. */
const LEG_MS = Number(env.JOURNEY_UI_LEG_MS ?? 2_000);

const OPTIONS = {
  quotes: [
    { minutes: 30, price_minor: 3_000 },
    { minutes: 60, price_minor: 6_000 },
  ],
  wallet_minor: 25_050,
  packs: [],
  credit_headroom_minor: null,
  closing_label: "11:00 pm",
};

/** Runs `act`, then waits for `seen`, and returns the milliseconds from the start to the sight. */
async function timed(act: () => Promise<unknown>, seen: () => Promise<unknown>): Promise<number> {
  const t0 = performance.now();
  await act();
  await seen();
  return Math.round(performance.now() - t0);
}

const calls = (page: Page) =>
  page.evaluate(() => (window as unknown as { __calls: string[] }).__calls);

test("Journey 3 on the kiosk UI: every leg is timed and the PC ends back on attract", async ({
  page,
  context,
}) => {
  const hud = await context.newPage();
  await openKiosk(page);
  await openKiosk(hud, { path: "/hud" });
  await answer(hud, names.commands.playerExtendOptions, { ok: true, body: OPTIONS });
  await answer(hud, names.commands.playerExtend, { ok: true, body: { outcome: "ok" } });
  for (const p of [page, hud]) await emit(p, names.events.attract, ATTRACT);
  await expect(page.getByTestId("attract")).toBeVisible();

  const legs: Record<string, number> = {};

  // Attract → login → plan picker.
  await page.keyboard.press("x");
  await expect(page.getByTestId("login-username")).toBeVisible();
  await page.getByTestId("login-username").fill("asha");
  await page.getByTestId("login-password").fill("right-password");
  legs.login_ms = await timed(
    () => page.getByTestId("login-password").press("Enter"),
    () => expect(page.getByTestId("picker-option").first()).toBeVisible(),
  );

  // Start: the Hub approves and the shell opens on Home.
  await page.getByTestId("picker-option").first().click();
  await page.getByRole("button", { name: "Start" }).first().click();
  expect(await calls(page)).toContain(names.commands.playerStart);
  const running = shown({ ...PREPAID, remaining_ms: 40 * 60_000 });
  legs.shell_ms = await timed(
    async () => {
      await emit(page, names.events.pcScreen, running);
      await emit(hud, names.events.pcScreen, running);
    },
    () => expect(page.getByTestId("shell")).toBeVisible(),
  );
  await expect(page.getByTestId("tab-home")).toHaveAttribute("aria-current", "page");

  // Launch a game from the Games tab.
  await page.getByTestId("tab-games").click();
  await expect(page.getByTestId("games-grid")).toBeVisible();
  legs.launch_ms = await timed(
    () => page.getByTestId("games-grid").locator("button").first().click(),
    () => expect.poll(() => calls(page)).toContain(names.commands.gameLaunch),
  );

  // The 10-minute warning toast, then Extend from the expanded HUD.
  const warned = shown({ ...PREPAID, remaining_ms: 10 * 60_000, warning_min: 10 });
  legs.warning_ms = await timed(
    () => emit(hud, names.events.pcScreen, warned),
    () => expect(hud.getByTestId("toast")).toBeVisible(),
  );
  await emit(hud, names.events.hudExpand);
  await expect(hud.getByTestId("extend-options")).toBeVisible();
  await hud.getByTestId("extend-step").first().locator("input").check();
  legs.extend_ms = await timed(
    () => hud.getByTestId("extend-confirm").click(),
    () => expect(hud.getByTestId("toast-text")).toHaveText("More time added. GG."),
  );
  expect(await calls(hud)).toContain(names.commands.playerExtend);

  // Time runs out: the grace card, then the clean-up and the lock back to attract.
  const grace = shown({ ...PREPAID, remaining_ms: 0, grace_in_ms: 0, grace_ms: 120_000 });
  legs.grace_ms = await timed(
    () => emit(hud, names.events.pcScreen, grace),
    () => expect(hud.getByTestId("grace")).toBeVisible(),
  );
  legs.attract_ms = await timed(
    async () => {
      await emit(page, names.events.pcScreen, shown(null));
      await emit(hud, names.events.pcScreen, shown(null));
    },
    () => expect(page.getByTestId("attract")).toBeVisible(),
  );
  await expect(page.getByTestId("shell")).toHaveCount(0);
  await expect(hud.getByTestId("grace")).toHaveCount(0);

  const result = { source: "ui", ...legs, returned_to_attract: true };
  console.log(`journey ui timings: ${JSON.stringify(result)}`);
  // The kiosk package carries no Node typings, so the file modules are loaded untyped.
  const [fs, path] = await Promise.all([
    import(/* @vite-ignore */ "node:" + "fs"),
    import(/* @vite-ignore */ "node:" + "path"),
  ]);
  const out = path.resolve(env.ARENA_JOURNEY_UI_OUT ?? "test-results/journey-ui.json");
  fs.mkdirSync(path.dirname(out), { recursive: true });
  fs.writeFileSync(out, JSON.stringify(result, null, 2));
  for (const [leg, ms] of Object.entries(legs)) {
    expect(ms, `${leg} took ${ms} ms`).toBeLessThanOrEqual(LEG_MS);
  }
});
