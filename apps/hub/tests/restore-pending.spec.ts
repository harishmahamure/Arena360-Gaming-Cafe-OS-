import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type { HubSnapshot } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { ORIGIN } from "./origin";

// A replacement Hub (Story 9.9): paired with a branch that already has history, it shows
// "Restore this branch" and nothing else, with the one option off until the owner releases the
// branch key. The Hub's loopback API is stubbed with `page.route`.

const HUB = "http://127.0.0.1:7409";
const CORS = {
  "access-control-allow-origin": ORIGIN,
  "access-control-allow-methods": "GET, POST",
  "access-control-allow-headers": "authorization, content-type",
};

function snapshot(): HubSnapshot {
  return {
    branch: null,
    devices: [],
    locale: null,
    ui: { idle_lock_minutes: 5, hc: false },
    connectivity: { state: "online", checked_at: 1_800_000_000_000 },
    paired: true,
    restore_pending: "Pixel Arena Gaming",
  } as unknown as HubSnapshot;
}

function json(route: Route, status: number, body: unknown) {
  return route.fulfill({
    status,
    contentType: "application/json",
    headers: CORS,
    body: JSON.stringify(body),
  });
}

async function open(page: Page) {
  const asked: string[] = [];
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    const url = new URL(req.url());
    asked.push(url.pathname);
    switch (url.pathname) {
      case "/ui/snapshot":
        return json(route, 200, snapshot());
      case "/ui/me":
        return json(route, 401, { code: "AUTH_FAILED", message: "sign in" });
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: `retry: 300\n\n`,
        });
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/floor");
  await expect(page.getByTestId("hub-restore-pending")).toBeVisible();
  return asked;
}

test("a replacement Hub shows only Restore this branch with the option off and the reason shown", async ({
  page,
}) => {
  await open(page);
  await expect(page.locator("h1")).toHaveText(en["hub.restore_pending.title"]);
  await expect(page.getByTestId("restore-branch")).toContainText("Pixel Arena Gaming");
  const screen = page.getByTestId("hub-restore-pending");
  await expect(screen).toContainText(en["hub.restore_pending.replaces"]);
  await expect(screen).toContainText(en["hub.restore_pending.serves_nothing"]);
  const option = screen.getByRole("button", { name: en["hub.restore_pending.option"] });
  await expect(option).toBeDisabled();
  await expect(screen).toContainText(en["hub.restore_pending.waiting"]);

  // No sign-in screen, no rail, no floor: the Hub serves nothing until it is restored.
  await expect(page).toHaveURL(/\/floor$/);
  await expect(page.locator("nav")).toHaveCount(0);
  await expect(page.locator("h1")).toHaveCount(1);
});

test("the Restore this branch screen meets the accessibility floor", async ({ page }) => {
  await open(page);
  const results = await new AxeBuilder({ page }).analyze();
  expect(results.violations).toEqual([]);
});
