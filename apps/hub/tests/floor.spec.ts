import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type { HubSnapshot } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { CORS, HUB, MANAGER, device, json, snapshot } from "./floor-fixture";

async function open(page: Page, snap: () => HubSnapshot, gate?: () => boolean) {
  let streamed = false;
  await page.addInitScript(() => sessionStorage.setItem("arena.hub.token", "manager-token"));
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot":
        return json(route, snap());
      case "/ui/me":
        return json(route, MANAGER);
      case "/ui/settings":
        return json(route, []);
      case "/ui/events":
        if (!gate || streamed) return route.abort();
        streamed = true;
        while (!gate()) await new Promise((r) => setTimeout(r, 20));
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: "retry: 600000\n\nevent: devices.changed\ndata: {}\n\n",
        });
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/floor");
}

const tile = (page: Page, label: string) =>
  page.locator("[data-component=floor-tile]", { hasText: label });

test("tiles show the rate, last seen, the maintenance note and the zone count", async ({
  page,
}) => {
  await open(page, () =>
    snapshot([
      device(1, {}),
      device(2, { online: false, status: "offline", last_seen_at: Date.UTC(2026, 9, 25, 9, 30) }),
      device(3, { online: false, status: "maintenance", maintenance_note: "fan is dead" }),
      device(4, { group_id: null, zone: null, online: false, status: "offline" }),
    ]),
  );
  await expect(tile(page, "PC 1")).toContainText("₹100.00/hr · tap to start");
  await expect(tile(page, "PC 2").getByTestId("device-last-seen")).toContainText("3:00");
  await expect(tile(page, "PC 3").getByTestId("device-unavailable")).toContainText("fan is dead");
  await expect(tile(page, "PC 4")).toContainText(en["ui.status.unassigned"]);
  const zone = page.getByTestId("floor-zone");
  await expect(zone).toContainText("Main hall");
  await expect(zone).toContainText("3 devices");
  await expect(page.getByRole("region", { name: en["hub.floor.unassigned"] })).toContainText(
    "1 device",
  );
  const headings = await page.getByRole("heading", { level: 2 }).allTextContents();
  expect(headings.at(-1)).toBe(en["hub.floor.unassigned"]);
});

test("a free tile without a rate says tap to start", async ({ page }) => {
  await open(page, () => snapshot([device(1, {})], null));
  await expect(tile(page, "PC 1")).toContainText(en["hub.floor.tap_to_start"]);
});

test("devices.changed updates the tile and keeps focus where it was", async ({ page }) => {
  let online = false;
  let release = false;
  await open(
    page,
    () => snapshot([device(1, { online, status: online ? "online" : "offline" }), device(2, {})]),
    () => {
      if (release) online = true;
      return release;
    },
  );
  await expect(tile(page, "PC 1").getByTestId("device-presence")).toHaveText(
    en["hub.floor.offline"],
  );
  await tile(page, "PC 2").getByRole("group").focus();
  release = true;
  await expect(tile(page, "PC 1").getByTestId("device-presence")).toHaveText(
    en["hub.floor.online"],
  );
  await expect(tile(page, "PC 2").getByRole("group")).toBeFocused();
});

test("arrows move focus in grid order and Enter activates the tile", async ({ page }) => {
  await open(page, () => snapshot([device(1, {}), device(2, {}), device(3, {})]));
  const faces = page.locator("[data-tile-face]");
  await faces.nth(0).focus();
  await page.keyboard.press("ArrowRight");
  await expect(faces.nth(1)).toBeFocused();
  await page.keyboard.press("ArrowRight");
  await expect(faces.nth(2)).toBeFocused();
  await page.keyboard.press("ArrowLeft");
  await expect(faces.nth(1)).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(tile(page, "PC 2")).toHaveAttribute("aria-current", "true");
  await expect(faces.nth(1)).toHaveAttribute("tabindex", "0");
  await expect(faces.nth(0)).toHaveAttribute("tabindex", "-1");
});

test("tile accessible names include device, state and layout holds at 800px and 1920px", async ({
  page,
}) => {
  await page.setViewportSize({ width: 800, height: 800 });
  await open(page, () => snapshot(Array.from({ length: 12 }, (_, i) => device(i + 1, {}))));
  await expect(page.getByRole("group", { name: "PC 1, Online" })).toBeVisible();
  const cols = (selector: string) =>
    page
      .locator(selector)
      .evaluate((el) => getComputedStyle(el).gridTemplateColumns.split(" ").length);
  const group = page.getByTestId("floor-zone").locator("ul");
  await expect(group).toBeVisible();
  const narrow = await tile(page, "PC 1").first().boundingBox();
  expect(narrow?.height ?? 0).toBeGreaterThanOrEqual(128);
  expect(await cols("[data-testid=floor-zone] ul")).toBe(2);
  await page.setViewportSize({ width: 1920, height: 1000 });
  expect(await cols("[data-testid=floor-zone] ul")).toBe(6);
});

test("axe: the floor has no serious violations", async ({ page }) => {
  await open(page, () =>
    snapshot([
      device(1, {}),
      device(2, { online: false, status: "offline" }),
      device(3, { group_id: null, zone: null }),
    ]),
  );
  await expect(tile(page, "PC 1")).toBeVisible();
  const results = await new AxeBuilder({ page }).analyze();
  expect(
    results.violations.filter((v) => v.impact === "serious" || v.impact === "critical"),
  ).toEqual([]);
});
