import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type { SnapshotSession } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { CORS, HUB, MANAGER, device, json, snapshot } from "./floor-fixture";

const MIN = 60_000;

const session = (n: number, extra: Partial<SnapshotSession>): SnapshotSession => ({
  id: `S${n}`,
  device_id: device(n, {}).device_id,
  state: "active",
  started_at: Date.now() - 20 * MIN,
  paused_total_ms: 0,
  rev: 1,
  guest_no: n,
  ...extra,
});

async function open(page: Page, sessions: SnapshotSession[]) {
  await page.addInitScript(() => sessionStorage.setItem("arena.hub.token", "manager-token"));
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot": {
        const snap = snapshot(
          sessions.map((s) => device(Number(s.id.slice(1)), { status: "in_session" })),
        );
        snap.sessions = sessions;
        return json(route, snap);
      }
      case "/ui/me":
        return json(route, MANAGER);
      case "/ui/settings":
        return json(route, []);
      case "/ui/events":
        return route.abort();
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/floor");
}

const tile = (page: Page, label: string) =>
  page.locator("[data-component=floor-tile]", { hasText: label });

test("a timed session counts down and warns from 10 minutes", async ({ page }) => {
  await open(page, [
    session(1, { ends_at: Date.now() + 30 * MIN }),
    session(2, { ends_at: Date.now() + 9 * MIN + 30_000 }),
    session(3, { ends_at: Date.now() + 5 * MIN, state: "paused", paused_at: Date.now() - MIN }),
    session(4, {}),
  ]);
  await expect(tile(page, "PC 1")).toContainText("0:29:");
  await expect(tile(page, "PC 1").getByTestId("tile-time-warning")).toHaveCount(0);
  await expect(tile(page, "PC 2")).toContainText("0:09:");
  await expect(tile(page, "PC 2").getByTestId("tile-time-warning")).toBeVisible();
  await expect(tile(page, "PC 2").locator("[data-tile-face]")).toHaveAttribute(
    "aria-label",
    new RegExp(en["ui.floor_tile.time_warning"]),
  );
  await expect(tile(page, "PC 3").getByTestId("tile-time-warning")).toHaveCount(0);
  await expect(tile(page, "PC 4").getByTestId("tile-time-warning")).toHaveCount(0);
  const results = await new AxeBuilder({ page }).analyze();
  expect(
    results.violations.filter((v) => v.impact === "serious" || v.impact === "critical"),
  ).toEqual([]);
});

test("a session in grace shows its countdown and a held one shows time up", async ({ page }) => {
  await open(page, [
    session(1, { state: "grace", ends_at: Date.now() - MIN, grace_ends_at: Date.now() + 75_000 }),
    session(2, { state: "expired", ends_at: Date.now() - 5 * MIN }),
  ]);
  await expect(tile(page, "PC 1")).toContainText(/Grace 1:1\d/);
  await expect(tile(page, "PC 1").getByTestId("tile-time-warning")).toBeVisible();
  await expect(tile(page, "PC 2")).toContainText(en["hub.floor.time_up"]);
  await expect(tile(page, "PC 2").getByTestId("tile-time-warning")).toBeVisible();
  const results = await new AxeBuilder({ page }).analyze();
  expect(
    results.violations.filter((v) => v.impact === "serious" || v.impact === "critical"),
  ).toEqual([]);
});
