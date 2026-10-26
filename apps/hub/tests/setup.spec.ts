import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type { HubSnapshot, SessionStaff, SetupStatus, SnapshotDevice } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { ORIGIN } from "./origin";

const HUB = "http://127.0.0.1:7409";
const CORS = {
  "access-control-allow-origin": ORIGIN,
  "access-control-allow-methods": "GET, POST",
  "access-control-allow-headers": "authorization, content-type",
};
const COUNTER: SessionStaff = {
  staff_id: "01JCOUNTER0000000000000000",
  display_name: "Counter",
  role_name: "Counter",
  permissions: ["session.start"],
};
const MANAGER: SessionStaff = {
  ...COUNTER,
  display_name: "Manager",
  role_name: "Manager",
  permissions: ["session.start", "device.manage"],
};
const kiosk = (id: string, over: Partial<SnapshotDevice> = {}): SnapshotDevice => ({
  device_id: id,
  label: `PC-${id}`,
  class: "paired",
  zone: null,
  online: true,
  kind: "pc",
  group_id: "G0",
  status: "online",
  ...over,
});
const STATUS: SetupStatus = {
  opening_hours_set: false,
  backup_location_set: false,
  ad_consent: "missing",
  printer: "unset",
  products_count: 0,
  groups_without_rate: 0,
  manager_exists: false,
  test_session: "unset",
};

function snapshot(
  setup: SetupStatus,
  groups: number,
  paired: boolean,
  devices: SnapshotDevice[] = [],
): HubSnapshot {
  return {
    branch: { name: "Dev Branch", branch_code: "DEV01" },
    groups: Array.from({ length: groups }, (_, i) => ({
      id: `G${i}`,
      name: `Group ${i}`,
      kind: "pc",
      colour: "teal",
      sort_order: i,
      rev: 1,
    })),
    zones: [],
    devices: paired
      ? [{ device_id: "D1", label: "PC-1", class: "paired", zone: null, online: true }, ...devices]
      : devices,
    locale: {
      currency: "INR",
      exponent: 2,
      symbol: "₹",
      symbol_position: "before",
      number_format: "indian",
      tz: "Asia/Kolkata",
      day_start_min: 360,
      fy_start_month: 4,
      pending: null,
    },
    ui: { idle_lock_minutes: 5, hc: false },
    connectivity: { state: "online", checked_at: null },
    lan: null,
    paired: true,
    setup,
  };
}

type Reply = { status: number; body: unknown };

async function open(
  page: Page,
  body: HubSnapshot,
  staff: SessionStaff = COUNTER,
  intent: { posts: unknown[]; reply: Reply } = { posts: [], reply: { status: 200, body: {} } },
) {
  await page.addInitScript((t) => sessionStorage.setItem("arena.hub.token", t), "counter-token");
  await page.route(`${HUB}/**`, (route) => {
    const req = route.request();
    const json = (status: number, b: unknown) =>
      route.fulfill({
        status,
        contentType: "application/json",
        headers: CORS,
        body: JSON.stringify(b),
      });
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot":
        return json(200, body);
      case "/ui/me":
        return json(200, staff);
      case "/ui/intent":
        intent.posts.push(req.postDataJSON());
        return json(intent.reply.status, intent.reply.body);
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: "retry: 400\n\n",
        });
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/settings/setup");
  await expect(page.getByTestId("setup-list")).toBeVisible();
}

const row = (page: Page, key: keyof typeof en) =>
  page.getByTestId("setup-list").getByRole("listitem").filter({ hasText: en[key] });

test("a fresh branch shows every row not done, with links to its screens", async ({ page }) => {
  await open(page, snapshot(STATUS, 0, false));
  for (const key of [
    "hub.settings.setup.pair",
    "hub.settings.setup.groups",
    "hub.settings.setup.rates",
    "hub.settings.setup.printer",
    "hub.settings.setup.test_session",
  ] as const) {
    await expect(row(page, key)).toContainText(en["hub.settings.setup.todo"]);
  }
  const link = (key: keyof typeof en) =>
    row(page, key).getByRole("link", {
      name: en["hub.settings.setup.open"].replace("{screen}", en[key]),
    });
  await expect(link("hub.settings.setup.pair")).toHaveAttribute("href", "/settings/devices");
  await expect(link("hub.settings.setup.groups")).toHaveAttribute("href", "/settings/layout");
  await expect(link("hub.settings.setup.printer")).toHaveAttribute("href", "/settings/printer");
  await expect(row(page, "hub.settings.setup.test_session").getByRole("link")).toHaveCount(0);
});

test("groups made in the web wizard count, and rated groups and a printer tick the rest", async ({
  page,
}) => {
  await open(page, snapshot({ ...STATUS, printer: "skipped", test_session: "passed" }, 2, true));
  for (const key of [
    "hub.settings.setup.pair",
    "hub.settings.setup.groups",
    "hub.settings.setup.rates",
    "hub.settings.setup.printer",
    "hub.settings.setup.test_session",
  ] as const) {
    await expect(row(page, key)).toContainText(en["hub.settings.setup.done"]);
  }
});

test("a group without a rate leaves Rates not done", async ({ page }) => {
  await open(page, snapshot({ ...STATUS, groups_without_rate: 1 }, 1, true));
  await expect(row(page, "hub.settings.setup.groups")).toContainText(en["hub.settings.setup.done"]);
  await expect(row(page, "hub.settings.setup.rates")).toContainText(en["hub.settings.setup.todo"]);
});

test("the Setup section is in the Settings navigation and passes axe", async ({ page }) => {
  await open(page, snapshot(STATUS, 1, true));
  const nav = page.getByRole("navigation", { name: en["hub.settings.sections"] });
  await expect(nav.getByRole("link", { name: en["hub.settings.nav.setup"] })).toHaveAttribute(
    "aria-current",
    "page",
  );
  const results = await new AxeBuilder({ page }).analyze();
  const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  expect(bad.map((v) => v.id)).toEqual([]);
});

const pick = (page: Page) => page.getByLabel(en["hub.settings.setup.test_session.pick"]);
const startButton = (page: Page) =>
  page.getByRole("button", { name: en["hub.settings.setup.test_session.start"], exact: true });

test("a counter user sees the Test session row without a Start button", async ({ page }) => {
  await open(page, snapshot(STATUS, 1, true, [kiosk("D2")]));
  await expect(row(page, "hub.settings.setup.test_session")).toBeVisible();
  await expect(pick(page)).toHaveCount(0);
  await expect(startButton(page)).toHaveCount(0);
});

test("a manager picks an online grouped PC and Start sends device.test_session.start", async ({
  page,
}) => {
  const intent = { posts: [] as unknown[], reply: { status: 200, body: { applied: {} } } };
  await open(
    page,
    snapshot(STATUS, 1, true, [
      kiosk("D2"),
      kiosk("D3", { online: false }),
      kiosk("D4", { group_id: null }),
      kiosk("D5", { kind: "console" }),
      kiosk("D6", { status: "maintenance" }),
      kiosk("D7"),
    ]),
    MANAGER,
    intent,
  );
  await expect(pick(page).locator("option")).toHaveText(["PC-D2", "PC-D7"]);
  await pick(page).selectOption("D7");
  await startButton(page).click();
  await expect.poll(() => intent.posts.length).toBe(1);
  const sent = intent.posts[0] as { intent: { payload: unknown; kind: string } };
  expect(sent.intent.kind).toBe("device.test_session.start");
  expect(sent.intent.payload).toEqual({ kind: "device.test_session.start", device_id: "D7" });
});

test("a refusal shows inline and the list stays open", async ({ page }) => {
  const intent = {
    posts: [] as unknown[],
    reply: {
      status: 422,
      body: {
        code: "VALIDATION_FAILED",
        message: "a test session is already running",
        field: "device_id",
      },
    },
  };
  await open(page, snapshot(STATUS, 1, true, [kiosk("D2")]), MANAGER, intent);
  await startButton(page).click();
  await expect(page.getByTestId("test-session-refusal")).toContainText(
    "a test session is already running",
  );
  await expect(pick(page)).toBeVisible();
});

test("a running test shows its line and hides Start", async ({ page }) => {
  await open(
    page,
    snapshot({ ...STATUS, test_session: "running" }, 1, true, [kiosk("D2")]),
    MANAGER,
  );
  await expect(page.getByTestId("test-session-state")).toHaveText(
    en["hub.settings.setup.test_session.running"],
  );
  await expect(startButton(page)).toHaveCount(0);
});

test("a failed test shows its line and offers Start again", async ({ page }) => {
  await open(
    page,
    snapshot({ ...STATUS, test_session: "failed" }, 1, true, [kiosk("D2")]),
    MANAGER,
  );
  await expect(page.getByTestId("test-session-state")).toHaveText(
    en["hub.settings.setup.test_session.failed"],
  );
  await expect(startButton(page)).toBeVisible();
});

test("a passed test shows its line", async ({ page }) => {
  await open(
    page,
    snapshot({ ...STATUS, test_session: "passed" }, 1, true, [kiosk("D2")]),
    MANAGER,
  );
  await expect(page.getByTestId("test-session-state")).toHaveText(
    en["hub.settings.setup.test_session.passed"],
  );
});

test("the Test session form passes axe", async ({ page }) => {
  await open(page, snapshot(STATUS, 1, true, [kiosk("D2")]), MANAGER);
  await expect(startButton(page)).toBeVisible();
  const results = await new AxeBuilder({ page }).analyze();
  const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  expect(bad.map((v) => v.id)).toEqual([]);
});
