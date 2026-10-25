import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type { HubSnapshot } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { ORIGIN } from "./origin";

const HUB = "http://127.0.0.1:7409";
const CORS = {
  "access-control-allow-origin": ORIGIN,
  "access-control-allow-methods": "GET, POST",
  "access-control-allow-headers": "authorization, content-type",
};
const UNPAIRED: HubSnapshot = {
  branch: null,
  groups: [],
  devices: [],
  locale: null,
  ui: { idle_lock_minutes: 5, hc: true },
  connectivity: { state: "online", checked_at: null },
  setup: null,
  lan: null,
  paired: false,
};
const PAIRED: HubSnapshot = {
  branch: { name: "Dev Branch", branch_code: "DEV01" },
  groups: [],
  devices: [],
  locale: null,
  ui: { idle_lock_minutes: 5, hc: false },
  connectivity: { state: "online", checked_at: null },
  lan: null,
  paired: true,
};

type Answer = { status: number; body: unknown; headers?: Record<string, string> };

const fail = (status: number, code: string, extra: Record<string, unknown> = {}): Answer => ({
  status,
  body: { code, message: code.toLowerCase(), ...extra },
});

async function serve(page: Page, answers: Answer[] = []) {
  const state = {
    paired: false,
    answers: [...answers],
    codes: [] as string[],
  };
  await page.route(`${HUB}/ui/**`, (route) => {
    if (route.request().method() === "OPTIONS") {
      return route.fulfill({ status: 204, headers: CORS });
    }
    return route.fulfill({
      status: 409,
      contentType: "application/json",
      headers: CORS,
      body: JSON.stringify({ code: "HUB_NOT_PAIRED", message: "hub not paired" }),
    });
  });
  await page.route(`${HUB}/ui/snapshot`, (route) =>
    route.fulfill({
      status: 200,
      contentType: "application/json",
      headers: CORS,
      body: JSON.stringify(state.paired ? PAIRED : UNPAIRED),
    }),
  );
  await page.route(`${HUB}/ui/pair-hub`, (route) => {
    if (route.request().method() === "OPTIONS") {
      return route.fulfill({ status: 204, headers: CORS });
    }
    state.codes.push((route.request().postDataJSON() as { code: string }).code);
    const answer = state.answers.shift() ?? { status: 200, body: { paired: true } };
    if (answer.status === 200) state.paired = true;
    return route.fulfill({
      status: answer.status,
      contentType: "application/json",
      headers: { ...CORS, ...answer.headers },
      body: JSON.stringify(answer.body),
    });
  });
  return state;
}

const codeField = (page: Page) => page.getByLabel(en["hub.pair.code"], { exact: true });
const pairButton = (page: Page) =>
  page.getByRole("button", { name: en["hub.pair.submit"], exact: true });

async function openPair(page: Page, path = "/pair", answers: Answer[] = []) {
  const state = await serve(page, answers);
  await page.goto(path);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(en["hub.pair.title"]);
  return state;
}

async function submit(page: Page, code: string) {
  await codeField(page).fill(code);
  await pairButton(page).click();
}

test("an unpaired Hub shows only the dark pairing screen", async ({ page }) => {
  await openPair(page);
  await expect(page).toHaveURL(/\/pair$/);
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await expect(page.locator("h1")).toHaveCount(1);
  await expect(page.locator("nav")).toHaveCount(0);
  await expect(page.getByTestId("hub-branch")).toHaveCount(0);
  await expect(codeField(page)).toHaveAttribute("autocomplete", "off");
  await expect(codeField(page)).toBeFocused();
  await expect(pairButton(page)).toBeVisible();
  await expect(page.getByRole("button", { name: en["hub.pair.quit"], exact: true })).toBeVisible();
});

for (const path of ["/", "/floor", "/settings/branch", "/sign-in", "/system/audit"]) {
  test(`${path} redirects to the pairing screen while unpaired`, async ({ page }) => {
    await openPair(page, path);
    await expect(page).toHaveURL(/\/pair$/);
  });
}

test("a good code loads the normal shell", async ({ page }) => {
  const state = await openPair(page);
  await submit(page, "k7q4 9mzd");
  await expect(page).toHaveURL(/\/sign-in/);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(en["hub.sign_in.title"]);
  await expect(page.getByTestId("hub-branch")).toHaveText("Dev Branch");
  expect(state.codes).toEqual(["k7q4 9mzd"]);
});

test("an invalid code clears the field and keeps focus there", async ({ page }) => {
  const state = await openPair(page, "/pair", [fail(401, "PAIR_CODE_INVALID")]);
  await submit(page, "WRONG123");
  await expect(page.getByTestId("pair-error")).toContainText(en["hub.pair.invalid"]);
  await expect(codeField(page)).toHaveValue("");
  await expect(codeField(page)).toBeFocused();
  await expect(codeField(page)).toHaveAttribute("aria-invalid", "true");
  await submit(page, "K7Q49MZD");
  await expect(page).toHaveURL(/\/sign-in/);
  expect(state.codes).toEqual(["WRONG123", "K7Q49MZD"]);
});

test("an expired code reads the same as an invalid one", async ({ page }) => {
  await openPair(page, "/pair", [fail(401, "PAIR_CODE_EXPIRED")]);
  await submit(page, "K7Q49MZD");
  await expect(page.getByTestId("pair-error")).toContainText(en["hub.pair.invalid"]);
  await expect(codeField(page)).toHaveValue("");
});

test("a Windows Hub on Free points to the Android Hub", async ({ page }) => {
  await openPair(page, "/pair", [fail(403, "FEATURE_NOT_ENTITLED", { field: "platform" })]);
  await submit(page, "K7Q49MZD");
  await expect(page.getByTestId("pair-not-entitled")).toContainText(en["hub.pair.not_entitled"]);
  await expect(
    page.getByRole("link", { name: en["hub.pair.android_download"], exact: true }),
  ).toHaveAttribute("href", "https://arena360.cloud/download/android");
  await expect(page).toHaveURL(/\/pair$/);
});

test("no internet keeps the typed code, and a retry succeeds", async ({ page }) => {
  const state = await openPair(page, "/pair", [fail(503, "CLOUD_UNREACHABLE")]);
  await submit(page, "K7Q4-9MZD");
  await expect(page.getByTestId("pair-error")).toContainText(en["hub.pair.unreachable"]);
  await expect(codeField(page)).toHaveValue("K7Q4-9MZD");
  await expect(codeField(page)).not.toHaveAttribute("aria-invalid", "true");
  await pairButton(page).click();
  await expect(page).toHaveURL(/\/sign-in/);
  expect(state.codes).toEqual(["K7Q4-9MZD", "K7Q4-9MZD"]);
});

test("too many tries counts down in m:ss", async ({ page }) => {
  await page.clock.install({ time: new Date("2026-10-25T10:00:00Z") });
  const state = await openPair(page, "/pair", [
    { ...fail(429, "RATE_LIMITED", { retry_after_s: 75 }), headers: { "retry-after": "75" } },
  ]);
  await page.clock.pauseAt(new Date("2026-10-25T10:00:05Z"));
  await submit(page, "K7Q49MZD");
  const timer = page.getByTestId("pair-rate-limited");
  await expect(timer).toContainText(en["hub.pair.rate_limited"].replace("{time}", "1:15"));
  await expect(pairButton(page)).toHaveAttribute("aria-disabled", "true");
  await pairButton(page).click({ force: true });
  expect(state.codes).toHaveLength(1);
  await page.clock.runFor(16_000);
  await expect(timer).toContainText(en["hub.pair.rate_limited"].replace("{time}", "0:59"));
  await page.clock.runFor(60_000);
  await expect(timer).toHaveCount(0);
  await expect(pairButton(page)).not.toHaveAttribute("aria-disabled", "true");
});

test("an already paired Hub loads the shell", async ({ page }) => {
  const state = await openPair(page, "/pair", [fail(409, "ALREADY_SET")]);
  await page.route(`${HUB}/ui/snapshot`, (route) =>
    route.fulfill({
      status: 200,
      contentType: "application/json",
      headers: CORS,
      body: JSON.stringify(PAIRED),
    }),
  );
  await submit(page, "K7Q49MZD");
  await expect(page).toHaveURL(/\/sign-in/);
  expect(state.codes).toHaveLength(1);
});

test("an empty code is not sent", async ({ page }) => {
  const state = await openPair(page);
  await pairButton(page).click();
  await expect(page.getByTestId("pair-error")).toContainText(en["hub.pair.required"]);
  await expect(codeField(page)).toBeFocused();
  expect(state.codes).toEqual([]);
});

test("quit setup in a browser says the window can be closed", async ({ page }) => {
  await openPair(page);
  await page.getByRole("button", { name: en["hub.pair.quit"], exact: true }).click();
  await expect(page.getByTestId("pair-closed")).toHaveText(en["hub.pair.closed"]);
});

test("the pairing screen passes axe, with an error showing", async ({ page }) => {
  await openPair(page, "/pair", [fail(401, "PAIR_CODE_INVALID")]);
  await expect(page.getByRole("main")).toHaveCount(1);
  for (const name of [en["hub.pair.submit"], en["hub.pair.quit"]]) {
    const box = await page.getByRole("button", { name, exact: true }).boundingBox();
    expect(box!.height).toBeGreaterThanOrEqual(44);
  }
  expect((await codeField(page).boundingBox())!.height).toBeGreaterThanOrEqual(44);
  await submit(page, "WRONG123");
  await expect(page.getByTestId("pair-error")).toBeVisible();
  const results = await new AxeBuilder({ page }).analyze();
  const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  expect(bad.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual(
    [],
  );
});
