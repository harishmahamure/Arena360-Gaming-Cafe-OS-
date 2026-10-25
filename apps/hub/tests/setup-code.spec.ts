import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type { HubSnapshot } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { ORIGIN } from "./origin";

const HUB = "http://127.0.0.1:7409";
const SNAPSHOT: HubSnapshot = {
  branch: { name: "Dev Branch", branch_code: "DEV01" },
  groups: [],
  zones: [],
  devices: [],
  locale: null,
  ui: { idle_lock_minutes: 5, hc: false },
  connectivity: { state: "online", checked_at: null },
  lan: null,
  paired: true,
};
const CORS = {
  "access-control-allow-origin": ORIGIN,
  "access-control-allow-methods": "GET, POST",
  "access-control-allow-headers": "authorization, content-type",
};
const OWNER = {
  staff_id: "01JOWNER00000000000000000",
  display_name: "Asha Rao",
  role_name: "Owner",
  permissions: ["settings.edit"],
};

type Hub = {
  posts: { login: string; code: string; password: string; pin: string }[];
  answer: { status: number; body: unknown } | null;
  signedIn: boolean;
};

function json(route: Route, status: number, body: unknown) {
  return route.fulfill({
    status,
    contentType: "application/json",
    headers: CORS,
    body: JSON.stringify(body),
  });
}

async function stubHub(page: Page): Promise<Hub> {
  const hub: Hub = { posts: [], answer: null, signedIn: false };
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot":
        return json(route, 200, SNAPSHOT);
      case "/ui/auth/setup-code": {
        hub.posts.push(req.postDataJSON());
        if (hub.answer) return json(route, hub.answer.status, hub.answer.body);
        hub.signedIn = true;
        return json(route, 200, { token: "token-1", staff: OWNER });
      }
      case "/ui/me":
        return hub.signedIn
          ? json(route, 200, OWNER)
          : json(route, 401, { code: "AUTH_FAILED", message: "sign-in failed" });
      case "/ui/settings":
        return json(route, 200, []);
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: "retry: 60000\n\n",
        });
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  return hub;
}

async function openSetup(page: Page) {
  await page.goto("/sign-in");
  await page.getByRole("button", { name: en["hub.sign_in.setup_link"] }).click();
}

async function enterCode(page: Page, code: string) {
  await page.getByLabel(en["hub.setup.login"]).fill("+919876543210");
  await page.getByLabel(en["hub.setup.code"]).fill(code);
  await page.getByRole("button", { name: en["hub.setup.continue"] }).click();
}

async function choose(page: Page, password: string, pin: string, again: string) {
  await page.getByLabel(en["hub.setup.password"], { exact: true }).fill(password);
  await page.getByLabel(en["hub.setup.pin"], { exact: true }).fill(pin);
  await page.getByLabel(en["hub.setup.pin_again"]).fill(again);
  await page.getByRole("button", { name: en["hub.setup.submit"] }).click();
}

test("the setup screen says what to do and has no accessibility violations", async ({ page }) => {
  await stubHub(page);
  await openSetup(page);
  await expect(page.getByText(en["hub.setup.intro"])).toBeVisible();
  await expect(page.getByLabel(en["hub.setup.login"])).toBeVisible();
  await expect(page.getByLabel(en["hub.setup.code"])).toBeVisible();
  const results = await new AxeBuilder({ page }).analyze();
  expect(results.violations).toEqual([]);
});

test("a code, then a password and a PIN twice, signs the person in", async ({ page }) => {
  const hub = await stubHub(page);
  await openSetup(page);
  await enterCode(page, "abcd 2345");
  await choose(page, "a-long-new-password", "4821", "4821");
  await expect(page).toHaveURL(/\/floor$/);
  expect(hub.posts).toEqual([
    {
      login: "+919876543210",
      code: "abcd 2345",
      password: "a-long-new-password",
      pin: "4821",
    },
  ]);
});

test("a short code, a weak password and mismatched PINs are caught before anything is sent", async ({
  page,
}) => {
  const hub = await stubHub(page);
  await openSetup(page);
  await enterCode(page, "ABC");
  await expect(page.getByText(en["hub.setup.code_shape"]).first()).toBeVisible();
  await enterCode(page, "ABCD2345");
  await choose(page, "short", "4821", "4821");
  await expect(page.getByText(en["hub.setup.password_short"]).first()).toBeVisible();
  await choose(page, "a-long-new-password", "12", "12");
  await expect(page.getByText(en["hub.setup.pin_shape"]).first()).toBeVisible();
  await choose(page, "a-long-new-password", "4821", "4822");
  await expect(page.getByText(en["hub.setup.pin_mismatch"]).first()).toBeVisible();
  expect(hub.posts).toEqual([]);
});

test("a refused code shows one plain line and returns to the code step", async ({ page }) => {
  const hub = await stubHub(page);
  hub.answer = {
    status: 401,
    body: { code: "SETUP_CODE_INVALID", message: "that setup code can't be used" },
  };
  await openSetup(page);
  await enterCode(page, "ABCD2345");
  await choose(page, "a-long-new-password", "4821", "4821");
  await expect(page.getByText(en["hub.setup.invalid"]).first()).toBeVisible();
  await expect(page.getByLabel(en["hub.setup.code"])).toHaveValue("");
  await expect(page).toHaveURL(/\/sign-in(\?|$)/);
});

test("five failures pause entry with a countdown", async ({ page }) => {
  await page.clock.install({ time: new Date("2026-10-23T10:00:00") });
  const hub = await stubHub(page);
  hub.answer = {
    status: 429,
    body: { code: "AUTH_LOCKED", message: "locked", retry_after_s: 60 },
  };
  await openSetup(page);
  await page.clock.pauseAt(new Date("2026-10-23T10:00:10"));
  await enterCode(page, "ABCD2345");
  await choose(page, "a-long-new-password", "4821", "4821");
  const locked = page.getByTestId("setup-locked");
  await expect(locked).toContainText(en["hub.sign_in.locked"].replace("{seconds}", "60"));
  await expect(page.getByRole("button", { name: en["hub.setup.continue"] })).toHaveAttribute(
    "aria-disabled",
    "true",
  );
  await page.clock.runFor(61_000);
  await expect(locked).toHaveCount(0);
});

test("back returns to the sign-in form", async ({ page }) => {
  await stubHub(page);
  await openSetup(page);
  await page.getByRole("button", { name: en["hub.setup.back"] }).click();
  await expect(page.locator("main h1")).toHaveText(en["hub.sign_in.title"]);
});
