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

function snapshot(extra: Record<string, unknown> = {}): HubSnapshot {
  return {
    branch: null,
    devices: [],
    locale: null,
    ui: { idle_lock_minutes: 5, hc: false },
    connectivity: { state: "online", checked_at: 1_800_000_000_000 },
    paired: true,
    migration_wait: "Pixel Arena Gaming",
    ...extra,
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

async function open(
  page: Page,
  extra: Record<string, unknown> = {},
  answers: Record<string, { status: number; body: unknown }> = {},
) {
  const asked: string[] = [];
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    const url = new URL(req.url());
    asked.push(url.pathname);
    switch (url.pathname) {
      case "/ui/snapshot":
        return json(route, 200, snapshot(extra));
      case "/ui/me":
        return json(route, 401, { code: "AUTH_FAILED", message: "sign in" });
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: `retry: 300\n\n`,
        });
      default: {
        const answer = answers[url.pathname];
        if (answer) return json(route, answer.status, answer.body);
        return route.fulfill({ status: 404, headers: CORS });
      }
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
  // The only controls are the forms that take the tablet's address and code (Story 10.17) or a
  // snapshot file (Story 10.18).
  await expect(page.getByRole("button")).toHaveCount(2);
  for (const button of await page.getByRole("button").all()) await expect(button).toBeDisabled();
});

test("the Waiting for your Android Hub screen meets the accessibility floor", async ({ page }) => {
  await open(page);
  const results = await new AxeBuilder({ page }).analyze();
  expect(results.violations).toEqual([]);
});

const RECEIVED = {
  handover: { phase: "received", target_hub_name: "Pixel Arena Gaming", last_seq: 42 },
};

test("a received snapshot waits for the owner to release the branch key before it can be imported", async ({
  page,
}) => {
  const asked = await open(page, RECEIVED);
  await expect(page.getByTestId("handover-received")).toBeVisible();
  await expect(page.getByTestId("handover-key")).toHaveText(en["hub.migration_wait.key_waiting"]);
  const button = page.getByRole("button", { name: en["hub.migration_wait.import"] });
  await expect(button).toBeDisabled();
  await expect(page.getByText(en["hub.migration_wait.import_needs"])).toBeVisible();
  await button.click({ force: true });
  expect(asked).not.toContain("/ui/migration/import");
});

test("with the key released the import is one tap and the Hub says it is restarting", async ({
  page,
}) => {
  const asked = await open(
    page,
    { ...RECEIVED, restore_key_released: true },
    { "/ui/migration/import": { status: 200, body: { restarting: true } } },
  );
  await expect(page.getByTestId("handover-key")).toHaveText(en["hub.migration_wait.key_released"]);
  await page.getByRole("button", { name: en["hub.migration_wait.import"] }).click();
  await expect(page.getByTestId("handover-importing")).toHaveText(
    en["hub.migration_wait.importing"],
  );
  expect(asked).toContain("/ui/migration/import");
});

test("an import the Hub refuses shows its words and can be tried again", async ({ page }) => {
  await open(
    page,
    { ...RECEIVED, restore_key_released: true },
    {
      "/ui/migration/import": {
        status: 409,
        body: { code: "FENCED", message: "The branch moved on since the snapshot was made." },
      },
    },
  );
  const button = page.getByRole("button", { name: en["hub.migration_wait.import"] });
  await button.click();
  await expect(page.getByTestId("handover-problem")).toContainText("The branch moved on");
  await expect(button).toBeEnabled();
});

test("a snapshot file on this PC is sent to the Hub and a refusal shows its reason", async ({
  page,
}) => {
  const asked = await open(
    page,
    {},
    {
      "/ui/migration/file": {
        status: 422,
        body: { code: "RESTORE_CORRUPT", message: "That file was not made for a move." },
      },
    },
  );
  await page.getByTestId("handover-file-input").fill("D:\backup\move.a360");
  await page.getByRole("button", { name: en["hub.migration_wait.file_use"] }).click();
  await expect(page.getByTestId("handover-problem")).toContainText("not made for a move");
  expect(asked).toContain("/ui/migration/file");
});

test("the received screen meets the accessibility floor", async ({ page }) => {
  await open(page, { ...RECEIVED, restore_key_released: true });
  const results = await new AxeBuilder({ page }).analyze();
  expect(results.violations).toEqual([]);
});
