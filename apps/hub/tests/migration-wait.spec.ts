import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type { HubSnapshot } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { ORIGIN } from "./origin";

// A Windows Hub paired with the code of a Hub move (Story 10.16): it shows "Waiting for your Android
// Hub" and nothing else. The Hub's loopback API is stubbed with `page.route`.

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
    migration_wait: "Pixel Arena Gaming",
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
  await expect(page.getByTestId("hub-migration-wait")).toBeVisible();
  return asked;
}

test("a Windows Hub paired for a move shows only Waiting for your Android Hub", async ({
  page,
}) => {
  await open(page);
  await expect(page.locator("h1")).toHaveText(en["hub.migration_wait.title"]);
  await expect(page.getByTestId("wait-branch")).toContainText("Pixel Arena Gaming");
  const screen = page.getByTestId("hub-migration-wait");
  await expect(screen).toContainText(en["hub.migration_wait.steps"]);
  await expect(screen).toContainText(en["hub.migration_wait.serves_nothing"]);
  // No sign-in screen, no rail, no floor: the Hub serves nothing until the move is done.
  await expect(page).toHaveURL(/\/floor$/);
  await expect(page.locator("nav")).toHaveCount(0);
  await expect(page.locator("h1")).toHaveCount(1);
  await expect(page.getByRole("button")).toHaveCount(0);
});

test("the Waiting for your Android Hub screen meets the accessibility floor", async ({ page }) => {
  await open(page);
  const results = await new AxeBuilder({ page }).analyze();
  expect(results.violations).toEqual([]);
});
