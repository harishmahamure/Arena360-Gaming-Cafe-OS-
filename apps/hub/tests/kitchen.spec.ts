import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type { HubSnapshot, SessionStaff } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { ORIGIN } from "./origin";

const HUB = "http://127.0.0.1:7409";
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

function settings(flags: string) {
  return new Map([
    ["licence.flags", { value: flags, rev: 1 }],
    ["kitchen.kot_mode", { value: "receipt", rev: 1 }],
    ["kitchen.kot_printer", { value: "", rev: 1 }],
    ["kitchen.kot_paper_mm", { value: "80", rev: 1 }],
  ]);
}

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
  devices: [],
  locale: null,
  ui: { idle_lock_minutes: 5, hc: false },
  connectivity: { state: "online", checked_at: null },
  lan: null,
  paired: true,
  setup: null,
};

async function open(page: Page, flags: string, token = "manager-token", stale = false) {
  const rows = settings(flags);
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
        return json(
          route,
          200,
          [...rows].map(([key, row]) => ({ key, ...row })),
        );
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
        if (stale) return json(route, 409, { code: "STALE", message: "stale", field: "base_rev" });
        const { key, value } = intent.payload;
        intents.push({ key, value, base_rev: intent.base_rev, ticket: elevation_ticket });
        rows.set(key!, { value: value!, rev: rows.get(key!)!.rev + 1 });
        return json(route, 200, { applied: { intent_id: "I", changes: [] } });
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/settings/kitchen");
  return intents;
}

async function expectNoSeriousAxe(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  expect(bad.map((v) => v.id)).toEqual([]);
}

test("a Free branch sees the tier lock and no fields", async ({ page }) => {
  await open(page, "");
  const lock = page.getByTestId("kitchen-tier-lock");
  await expect(lock).toContainText(en["hub.settings.kitchen.locked"]);
  await expect(lock).toContainText(en["hub.settings.kitchen.locked_note"]);
  await expect(page.getByRole("radio")).toHaveCount(0);
  await expect(page.getByRole("button", { name: en["hub.settings.save"] })).toHaveCount(0);
  await expect(page.getByText(/upgrade/i)).toHaveCount(0);
  await expectNoSeriousAxe(page);
});

test("a Basic branch edits the KOT printer and saves with base_rev", async ({ page }) => {
  const intents = await open(page, "kitchen");
  await expect(page.getByTestId("settings-kitchen")).toBeVisible();
  await expect(page.getByTestId("kitchen-displays")).toHaveText(
    en["hub.settings.kitchen.displays_none"],
  );
  await expect(page.getByRole("button", { name: en["hub.settings.kitchen.pair"] })).toHaveAttribute(
    "aria-disabled",
    "true",
  );
  await expectNoSeriousAxe(page);
  await page.getByLabel(en["hub.settings.kitchen.mode.none"]).check();
  await page.getByRole("button", { name: en["hub.settings.save"] }).click();
  await expect(page.getByTestId("kitchen-status")).toHaveText(en["hub.settings.saved"]);
  expect(intents).toEqual([
    { key: "kitchen.kot_mode", value: "none", base_rev: 1, ticket: undefined },
  ]);
  await page.getByLabel(en["hub.settings.kitchen.mode.another"]).check();
  await page.getByLabel(en["hub.settings.printer.ip"]).fill("192.168.1.60");
  await page.getByRole("button", { name: en["hub.settings.printer.add"] }).click();
  await page.getByLabel(en["hub.settings.printer.paper_58"]).check();
  await page.getByRole("button", { name: en["hub.settings.save"] }).click();
  await expect.poll(() => intents.length).toBe(4);
  await expect(page.getByTestId("kitchen-selected")).toContainText("192.168.1.60");
  expect(intents.map((i) => i.key)).toEqual([
    "kitchen.kot_mode",
    "kitchen.kot_mode",
    "kitchen.kot_paper_mm",
    "kitchen.kot_printer",
  ]);
});

test("a counter's Save asks for a manager PIN", async ({ page }) => {
  const intents = await open(page, "kitchen", "counter-token");
  await page.getByLabel(en["hub.settings.kitchen.mode.none"]).check();
  await expect(page.getByTestId("kitchen-needs-approval")).toBeVisible();
  await page.getByRole("button", { name: en["hub.settings.save"] }).click();
  await expect(page.getByText(en["ui.pin.prompt"])).toBeVisible();
  expect(intents).toEqual([]);
});

test("a manager PIN approves a counter's save with one ticket", async ({ page }) => {
  const intents = await open(page, "kitchen", "counter-token");
  await page.getByLabel(en["hub.settings.kitchen.mode.none"]).check();
  await page.getByRole("button", { name: en["hub.settings.save"] }).click();
  await page.getByLabel(en["ui.pin.label"]).fill("2222");
  await page.getByLabel(en["ui.pin.label"]).press("Enter");
  await expect(page.getByTestId("kitchen-status")).toHaveText(en["hub.settings.saved"]);
  expect(intents).toEqual([{ key: "kitchen.kot_mode", value: "none", base_rev: 1, ticket: "T1" }]);
});

test("a stale save says so and keeps the edit", async ({ page }) => {
  await open(page, "kitchen", "manager-token", true);
  await page.getByLabel(en["hub.settings.kitchen.mode.none"]).check();
  await page.getByRole("button", { name: en["hub.settings.save"] }).click();
  await expect(page.getByTestId("kitchen-status")).toContainText(en["hub.settings.branch.stale"]);
  await expect(page.getByLabel(en["hub.settings.kitchen.mode.none"])).toBeChecked();
});
