import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type { HubSnapshot, SessionStaff } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { ORIGIN } from "./origin";

const HUB = "http://127.0.0.1:7409";
const KEY = "inventory.take_stock_at";
const CORS = {
  "access-control-allow-origin": ORIGIN,
  "access-control-allow-methods": "GET, POST",
  "access-control-allow-headers": "authorization, content-type",
};
const STAFF: Record<string, SessionStaff> = {
  "manager-token": {
    staff_id: "01JMANAGER0000000000000000",
    display_name: "Manager",
    role_name: "Manager",
    permissions: ["settings.edit"],
  },
  "counter-token": {
    staff_id: "01JCOUNTER0000000000000000",
    display_name: "Counter",
    role_name: "Counter",
    permissions: ["session.start", "pos.sell"],
  },
};

type Sent = { key?: string; value?: string; base_rev?: number; ticket?: string };

const json = (route: Route, status: number, body: unknown) =>
  route.fulfill({
    status,
    contentType: "application/json",
    headers: CORS,
    body: JSON.stringify(body),
  });

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
  setup: null,
};

async function open(page: Page, token = "manager-token", missing = false) {
  const rows = new Map([[KEY, { value: "served", rev: 3 }]]);
  const intents: Sent[] = [];
  await page.addInitScript((t) => sessionStorage.setItem("arena.hub.token", t), token);
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    const { pathname } = new URL(req.url());
    const staff = STAFF[(req.headers()["authorization"] ?? "").replace(/^Bearer /, "")];
    switch (pathname) {
      case "/ui/snapshot":
        return json(route, 200, SNAPSHOT);
      case "/ui/me":
        return json(route, 200, staff);
      case "/ui/settings":
        return json(route, 200, missing ? [] : [...rows].map(([key, row]) => ({ key, ...row })));
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: "retry: 400\n\n",
        });
      case "/ui/elevate": {
        const { items } = req.postDataJSON() as { items: unknown[] };
        return json(route, 200, { ticket: "T1", items, expires_in_s: 60 });
      }
      case "/ui/intent": {
        const { intent, elevation_ticket } = req.postDataJSON() as {
          elevation_ticket?: string;
          intent: { base_rev: number; payload: Sent };
        };
        const { key, value } = intent.payload;
        intents.push({ key, value, base_rev: intent.base_rev, ticket: elevation_ticket });
        rows.set(key!, { value: value!, rev: rows.get(key!)!.rev + 1 });
        return json(route, 200, { applied: { intent_id: "I", changes: [] } });
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/settings/inventory");
  return intents;
}

test("Settings has an Inventory section that offers Served and Order, Served by default", async ({
  page,
}) => {
  await open(page);
  await expect(
    page
      .getByRole("navigation", { name: en["hub.settings.sections"] })
      .getByRole("link", { name: en["hub.settings.nav.inventory"] }),
  ).toBeVisible();
  await expect(page.getByTestId("settings-inventory")).toBeVisible();
  const served = page.getByLabel(en["hub.settings.inventory.take_at.served"]);
  const order = page.getByLabel(en["hub.settings.inventory.take_at.order"]);
  await expect(served).toBeChecked();
  await expect(order).not.toBeChecked();
  await expect(page.getByRole("button", { name: en["hub.settings.save"] })).toHaveAttribute(
    "aria-disabled",
    "true",
  );
  const results = await new AxeBuilder({ page }).analyze();
  expect(
    results.violations.filter((v) => v.impact === "serious" || v.impact === "critical"),
  ).toEqual([]);
});

test("a manager switches to Order and saves with the setting's rev", async ({ page }) => {
  const intents = await open(page);
  await page.getByLabel(en["hub.settings.inventory.take_at.order"]).check();
  await page.getByRole("button", { name: en["hub.settings.save"] }).click();
  await expect(page.getByTestId("inventory-status")).toHaveText(en["hub.settings.saved"]);
  expect(intents).toEqual([{ key: KEY, value: "order", base_rev: 3, ticket: undefined }]);
  await expect(page.getByLabel(en["hub.settings.inventory.take_at.order"])).toBeChecked();
});

test("a counter needs a manager's PIN to save", async ({ page }) => {
  const intents = await open(page, "counter-token");
  await expect(page.getByTestId("inventory-needs-approval")).toBeVisible();
  await page.getByLabel(en["hub.settings.inventory.take_at.order"]).check();
  await page.getByRole("button", { name: en["hub.settings.save"] }).click();
  const pin = page.getByRole("group", { name: en["ui.pin.prompt"] });
  await expect(pin).toBeVisible();
  expect(intents).toHaveLength(0);
});

test("a Hub that does not report the setting shows the load failure with Retry", async ({
  page,
}) => {
  await open(page, "manager-token", true);
  await expect(page.getByTestId("inventory-load-failed")).toContainText(
    en["hub.settings.load_failed"],
  );
  await expect(page.getByRole("button", { name: en["common.retry"] })).toBeVisible();
});
