import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type { HubSnapshot, SessionStaff, SetupStatus } from "@arena/ui/generated";
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

function snapshot(setup: SetupStatus, groups: number, paired: boolean): HubSnapshot {
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
      ? [{ device_id: "D1", label: "PC-1", class: "paired", zone: null, online: true }]
      : [],
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

async function open(page: Page, body: HubSnapshot) {
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
        return json(200, COUNTER);
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
