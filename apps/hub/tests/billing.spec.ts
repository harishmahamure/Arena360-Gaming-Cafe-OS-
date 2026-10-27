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
const ROUNDING = "billing.rounding_min";
const MINIMUM = "billing.minimum_min";
const GRACE = "billing.grace_min";
const MODE = "billing.time_up_mode";
const CREDIT_PAUSE = "billing.credit_auto_pause";

type Row = { value: string; rev: number };
type Hub = {
  settings: Map<string, Row>;
  intents: { key: string; value: string; base_rev?: number; ticket?: string }[];
  elevations: { intent_kind: string; key?: string }[][];
  queue: string[];
};

function newHub(): Hub {
  return {
    settings: new Map([
      [ROUNDING, { value: "15", rev: 1 }],
      [MINIMUM, { value: "30", rev: 1 }],
      [GRACE, { value: "2", rev: 1 }],
      [MODE, { value: "clean_up", rev: 1 }],
      [CREDIT_PAUSE, { value: "true", rev: 1 }],
    ]),
    intents: [],
    elevations: [],
    queue: [],
  };
}

const LOCALE = {
  currency: "INR",
  exponent: 2,
  symbol: "₹",
  symbol_position: "before",
  number_format: "indian",
  tz: "Asia/Kolkata",
  day_start_min: 360,
  fy_start_month: 4,
  pending: null,
} as const;

function snapshot(hc = false): HubSnapshot {
  return {
    branch: { name: "Dev Branch", branch_code: "DEV01" },
    devices: [],
    groups: [],
    zones: [],
    locale: LOCALE,
    ui: { idle_lock_minutes: 5, hc },
    connectivity: { state: "online", checked_at: null },
    setup: {
      opening_hours_set: false,
      backup_location_set: false,
      ad_consent: "not_needed",
      printer: "unset",
      products_count: 0,
      groups_without_rate: 0,
      manager_exists: false,
      test_session: "unset",
      live: false,
    },
    lan: null,
    paired: true,
  };
}

function json(route: Route, status: number, body: unknown) {
  return route.fulfill({
    status,
    contentType: "application/json",
    headers: CORS,
    body: JSON.stringify(body),
  });
}

function changed(hub: Hub, keys: string[]) {
  const data = JSON.stringify({ locale: LOCALE, keys, ui: snapshot().ui });
  hub.queue.push(`event: settings.changed\ndata: ${data}\n\n`);
}

async function open(page: Page, token = "manager-token", hc = false) {
  const hub = newHub();
  await page.addInitScript((t) => sessionStorage.setItem("arena.hub.token", t), token);
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    const url = new URL(req.url());
    const auth = (req.headers()["authorization"] ?? "").replace(/^Bearer /, "");
    switch (url.pathname) {
      case "/ui/snapshot":
        return json(route, 200, snapshot(hc));
      case "/ui/me":
        return json(route, 200, STAFF[auth]);
      case "/ui/settings":
        return json(
          route,
          200,
          [...hub.settings].map(([key, row]) => ({ key, ...row })),
        );
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: "retry: 400\n\n" + hub.queue.splice(0).join(""),
        });
      case "/ui/elevate":
        hub.elevations.push((req.postDataJSON() as { items: Hub["elevations"][0] }).items);
        return json(route, 200, { ticket: "T1", items: [], expires_in_s: 60 });
      case "/ui/elevate/drop":
        return route.fulfill({ status: 204, headers: CORS });
      case "/ui/intent": {
        const { intent, elevation_ticket } = req.postDataJSON() as {
          intent: { base_rev?: number; payload: { key: string; value: string } };
          elevation_ticket?: string;
        };
        const { key, value } = intent.payload;
        hub.intents.push({ key, value, base_rev: intent.base_rev, ticket: elevation_ticket });
        const row = hub.settings.get(key)!;
        if (!STAFF[auth].permissions.includes("settings.edit") && !elevation_ticket) {
          return json(route, 403, { code: "PIN_REQUIRED", message: "pin" });
        }
        if (intent.base_rev !== row.rev) {
          return json(route, 409, { code: "STALE", message: "stale", field: "base_rev" });
        }
        hub.settings.set(key, { value, rev: row.rev + 1 });
        changed(hub, [key]);
        return json(route, 200, { applied: { intent_id: "I", changes: [] } });
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/settings/branch/billing");
  await expect(page.getByTestId("settings-billing")).toBeVisible();
  await expect(page.getByLabel(en["hub.settings.billing.rounding"])).toHaveValue("15");
  return hub;
}

const save = (page: Page) => page.getByRole("button", { name: en["hub.settings.save"] });
const rounding = (page: Page) => page.getByLabel(en["hub.settings.billing.rounding"]);
const minimum = (page: Page) => page.getByLabel(en["hub.settings.billing.minimum"]);
const grace = (page: Page) => page.getByLabel(en["hub.settings.billing.grace"]);
const status = (page: Page) => page.getByTestId("billing-status");

async function axe(page: Page) {
  const result = await new AxeBuilder({ page }).analyze();
  expect(
    result.violations.filter((v) => v.impact === "serious" || v.impact === "critical"),
  ).toEqual([]);
}

test("the Billing tab shows the defaults with a one-line explanation each, and axe passes", async ({
  page,
}) => {
  await open(page);
  await expect(
    page.getByRole("link", { name: en["hub.settings.branch.nav.billing"] }),
  ).toHaveAttribute("aria-current", "page");
  await expect(rounding(page)).toHaveValue("15");
  await expect(minimum(page)).toHaveValue("30");
  await expect(grace(page)).toHaveValue("2");
  await expect(page.getByLabel(en["hub.settings.billing.time_up_clean_up"])).toBeChecked();
  for (const k of [
    "rounding_help",
    "minimum_help",
    "grace_help",
    "time_up_help",
    "rounding_hint",
    "minimum_hint",
    "grace_hint",
  ] as const) {
    await expect(page.getByText(en[`hub.settings.billing.${k}`])).toBeVisible();
  }
  await expect(save(page)).toHaveAttribute("aria-disabled", "true");
  await axe(page);
});

test("a valid save stores every changed key at its revision", async ({ page }) => {
  const hub = await open(page);
  await rounding(page).fill("10");
  await minimum(page).fill("0");
  await grace(page).fill("10");
  await page.getByLabel(en["hub.settings.billing.time_up_staff_handles"]).check();
  await save(page).click();
  await expect(status(page)).toContainText(en["hub.settings.saved"]);
  expect(hub.intents).toEqual([
    { key: ROUNDING, value: "10", base_rev: 1 },
    { key: MINIMUM, value: "0", base_rev: 1 },
    { key: GRACE, value: "10", base_rev: 1 },
    { key: MODE, value: "staff_handles", base_rev: 1 },
  ]);
  await expect(save(page)).toHaveAttribute("aria-disabled", "true");
});

test("auto-pause credit is on by default and can be turned off", async ({ page }) => {
  const hub = await open(page);
  await expect(page.getByText(en["hub.settings.billing.credit_pause"])).toBeVisible();
  await expect(page.getByText(en["hub.settings.billing.credit_pause_help"])).toBeVisible();
  await expect(
    page.getByRole("radio", { name: en["hub.settings.billing.credit_pause_on"], exact: true }),
  ).toBeChecked();
  await page
    .getByRole("radio", { name: en["hub.settings.billing.credit_pause_off"], exact: true })
    .check();
  await save(page).click();
  await expect(status(page)).toContainText(en["hub.settings.saved"]);
  expect(hub.intents).toEqual([{ key: CREDIT_PAUSE, value: "false", base_rev: 1 }]);
  await expect(
    page.getByRole("radio", { name: en["hub.settings.billing.credit_pause_off"], exact: true }),
  ).toBeChecked();
  await axe(page);
});

test("the bounds are accepted", async ({ page }) => {
  const hub = await open(page);
  await rounding(page).fill("60");
  await minimum(page).fill("240");
  await grace(page).fill("0");
  await save(page).click();
  await expect(status(page)).toContainText(en["hub.settings.saved"]);
  expect(hub.intents.map((i) => i.value)).toEqual(["60", "240", "0"]);
});

for (const bad of ["", "abc", "1e1", "+5", "-1", "12.5", "0", "61", "015"]) {
  test(`rounding ${JSON.stringify(bad)} shows an inline error and writes nothing`, async ({
    page,
  }) => {
    const hub = await open(page);
    await rounding(page).fill(bad);
    await save(page).click();
    await expect(rounding(page)).toHaveAttribute("aria-invalid", "true");
    await expect(page.getByText(en["hub.settings.billing.invalid.rounding"])).toBeVisible();
    expect(hub.intents).toEqual([]);
  });
}

test("minimum above 240 and grace above 10 are refused before anything is written", async ({
  page,
}) => {
  const hub = await open(page);
  await minimum(page).fill("241");
  await grace(page).fill("11");
  await save(page).click();
  await expect(minimum(page)).toHaveAttribute("aria-invalid", "true");
  await expect(grace(page)).toHaveAttribute("aria-invalid", "true");
  await expect(page.getByText(en["hub.settings.billing.invalid.minimum"])).toBeVisible();
  await expect(page.getByText(en["hub.settings.billing.invalid.grace"])).toBeVisible();
  expect(hub.intents).toEqual([]);
  await axe(page);
});

test("a stale save is refused and the form reloads the Hub's value", async ({ page }) => {
  const hub = await open(page);
  hub.settings.set(GRACE, { value: "4", rev: 2 });
  await grace(page).fill("6");
  await save(page).click();
  await expect(status(page)).toContainText(en["hub.settings.branch.stale"]);
  await expect(grace(page)).toHaveValue("4");
});

test("a pushed settings.changed refreshes a clean form and keeps a dirty field", async ({
  page,
}) => {
  const hub = await open(page);
  hub.settings.set(MINIMUM, { value: "45", rev: 2 });
  changed(hub, [MINIMUM]);
  await expect(minimum(page)).toHaveValue("45");
  await rounding(page).fill("20");
  hub.settings.set(GRACE, { value: "5", rev: 2 });
  changed(hub, [GRACE]);
  await expect(grace(page)).toHaveValue("5");
  await expect(rounding(page)).toHaveValue("20");
});

test("a Counter sees Save, opens the manager PIN, and the save carries one ticket item per changed key", async ({
  page,
}) => {
  const hub = await open(page, "counter-token");
  await expect(page.getByTestId("billing-needs-approval")).toBeVisible();
  await rounding(page).fill("10");
  await grace(page).fill("5");
  await save(page).click();
  const pin = page.getByTestId("billing-approval");
  await expect(pin).toBeVisible();
  expect(hub.intents).toEqual([]);
  for (const d of "1234") await pin.getByRole("button", { name: d, exact: true }).click();
  await pin.getByRole("button", { name: en["ui.pin.key_submit"] }).click();
  await expect(status(page)).toContainText(en["hub.settings.saved"]);
  expect(hub.elevations).toEqual([
    [
      { intent_kind: "settings.set", key: ROUNDING },
      { intent_kind: "settings.set", key: GRACE },
    ],
  ]);
  expect(hub.intents.map((i) => [i.key, i.ticket])).toEqual([
    [ROUNDING, "T1"],
    [GRACE, "T1"],
  ]);
});

test("a Counter's invalid value never opens the PIN", async ({ page }) => {
  const hub = await open(page, "counter-token");
  await rounding(page).fill("0");
  await save(page).click();
  await expect(rounding(page)).toHaveAttribute("aria-invalid", "true");
  await expect(page.getByTestId("billing-approval")).toHaveCount(0);
  expect(hub.elevations).toEqual([]);
  expect(hub.intents).toEqual([]);
});

test("axe passes in high contrast, including the error state", async ({ page }) => {
  await open(page, "manager-token", true);
  await rounding(page).fill("0");
  await save(page).click();
  await expect(rounding(page)).toHaveAttribute("aria-invalid", "true");
  await axe(page);
});
