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
const GSTIN = "27AAPFU0939F1ZV";
const DEFAULT = {
  country: "IN",
  gst: false,
  gstin: "",
  inclusive: true,
  product_bp: 500,
  play_bp: 1800,
  sac: "",
  label: "Tax",
  rate_bp: 0,
};

type Hub = {
  tax: { value: string; rev: number };
  intents: { value: string; base_rev?: number; ticket?: string }[];
};

function snapshot(locked = false): HubSnapshot {
  return {
    branch: { name: "Dev Branch", branch_code: "DEV01", ...(locked ? { code_locked: true } : {}) },
    devices: [],
    groups: [],
    zones: [],
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
    setup: {
      opening_hours_set: false,
      backup_location_set: false,
      ad_consent: "missing",
      printer: "unset",
      products_count: 0,
      groups_without_rate: 0,
      manager_exists: false,
      test_session: "unset",
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

async function open(page: Page, token = "manager-token", locked = false) {
  const hub: Hub = { tax: { value: JSON.stringify(DEFAULT), rev: 1 }, intents: [] };
  await page.addInitScript((t) => sessionStorage.setItem("arena.hub.token", t), token);
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    const url = new URL(req.url());
    const auth = (req.headers()["authorization"] ?? "").replace(/^Bearer /, "");
    switch (url.pathname) {
      case "/ui/snapshot":
        return json(route, 200, snapshot(locked));
      case "/ui/me":
        return json(route, 200, STAFF[auth]);
      case "/ui/settings":
        return json(route, 200, [{ key: "branch.tax", ...hub.tax }]);
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: "retry: 60000\n\n",
        });
      case "/ui/elevate":
        return json(route, 200, { ticket: "T1", items: [], expires_in_s: 60 });
      case "/ui/elevate/drop":
        return route.fulfill({ status: 204, headers: CORS });
      case "/ui/intent": {
        const { intent, elevation_ticket } = req.postDataJSON() as {
          intent: { base_rev?: number; payload: { value: string } };
          elevation_ticket?: string;
        };
        hub.intents.push({
          value: intent.payload.value,
          base_rev: intent.base_rev,
          ticket: elevation_ticket,
        });
        if (intent.base_rev !== hub.tax.rev) {
          return json(route, 409, { code: "STALE", message: "stale", field: "base_rev" });
        }
        const t = JSON.parse(intent.payload.value);
        if (t.gst && t.gstin !== GSTIN) {
          return json(route, 422, {
            code: "VALIDATION_FAILED",
            message: "GSTIN_INVALID",
            field: "branch.tax.gstin",
          });
        }
        if (t.country === "IN" && t.product_bp > 2800) {
          return json(route, 422, {
            code: "VALIDATION_FAILED",
            message: "RATE_RANGE",
            field: "branch.tax.product_bp",
          });
        }
        if (t.country !== "IN" && t.rate_bp > 10000) {
          return json(route, 422, {
            code: "VALIDATION_FAILED",
            message: "RATE_RANGE",
            field: "branch.tax.rate_bp",
          });
        }
        hub.tax = { value: intent.payload.value, rev: hub.tax.rev + 1 };
        return json(route, 200, { applied: { intent_id: "I", changes: [] } });
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/settings/branch/tax");
  await expect(page.getByTestId("settings-tax")).toBeVisible();
  return hub;
}

test("the invoice prefix says it is locked once the first invoice has used it", async ({
  page,
}) => {
  await open(page, "manager-token", true);
  await expect(page.getByTestId("tax-prefix")).toHaveText("DEV01");
  await expect(page.getByTestId("tax-prefix-locked")).toHaveText(
    en["hub.settings.tax.prefix_locked"],
  );
});

test("the invoice prefix has no lock note before the first invoice", async ({ page }) => {
  await open(page);
  await expect(page.getByTestId("tax-prefix")).toHaveText("DEV01");
  await expect(page.getByTestId("tax-prefix-locked")).toHaveCount(0);
});

const save = (page: Page) => page.getByRole("button", { name: en["hub.settings.save"] });
const field = (page: Page, key: keyof typeof en) => page.getByLabel(en[key]);

test("an Indian branch registers for GST: the GSTIN is checked, then saved with rates in basis points", async ({
  page,
}) => {
  const hub = await open(page);
  await expect(page.getByTestId("tax-prefix")).toHaveText("DEV01");
  await expect(field(page, "hub.settings.tax.gstin")).toHaveCount(0);
  await expect(save(page)).toHaveAttribute("aria-disabled", "true");
  await field(page, "hub.settings.tax.gst").check();
  await field(page, "hub.settings.tax.gstin").fill("27AAPFU0939F1ZW");
  await field(page, "hub.settings.tax.product").fill("12.5");
  await save(page).click();
  await expect(page.getByText(en["hub.settings.tax.invalid.gstin"])).toBeVisible();
  expect(hub.tax.rev).toBe(1);
  await field(page, "hub.settings.tax.gstin").fill(GSTIN);
  await page.getByLabel(en["hub.settings.tax.inclusive_no"]).check();
  await save(page).click();
  await expect(page.getByTestId("tax-status")).toContainText(en["hub.settings.saved"]);
  expect(JSON.parse(hub.tax.value)).toEqual({
    ...DEFAULT,
    gst: true,
    gstin: GSTIN,
    inclusive: false,
    product_bp: 1250,
  });
  const axe = await new AxeBuilder({ page }).analyze();
  expect(axe.violations.filter((v) => v.impact === "serious" || v.impact === "critical")).toEqual(
    [],
  );
});

test("outside India there are no GST fields, only a label and one rate within 0 to 100", async ({
  page,
}) => {
  const hub = await open(page);
  await field(page, "hub.settings.tax.country").fill("AE");
  await expect(field(page, "hub.settings.tax.gst")).toHaveCount(0);
  await expect(field(page, "hub.settings.tax.product")).toHaveCount(0);
  await field(page, "hub.settings.tax.label").fill("VAT");
  await field(page, "hub.settings.tax.rate").fill("100.5");
  await save(page).click();
  await expect(page.getByText(en["hub.settings.tax.invalid.rate"])).toBeVisible();
  expect(hub.tax.rev).toBe(1);
  await field(page, "hub.settings.tax.rate").fill("5");
  await save(page).click();
  await expect(page.getByTestId("tax-status")).toContainText(en["hub.settings.saved"]);
  expect(JSON.parse(hub.tax.value)).toMatchObject({
    country: "AE",
    gst: false,
    label: "VAT",
    rate_bp: 500,
  });
});

test("a stale save is refused", async ({ page }) => {
  const hub = await open(page);
  hub.tax.rev = 2;
  await field(page, "hub.settings.tax.sac").fill("999692");
  await save(page).click();
  await expect(page.getByTestId("tax-status")).toContainText(en["hub.settings.branch.stale"]);
});

test("a counter member's Save opens the manager PIN in place", async ({ page }) => {
  const hub = await open(page, "counter-token");
  await expect(page.getByTestId("tax-needs-approval")).toBeVisible();
  await field(page, "hub.settings.tax.sac").fill("999692");
  await save(page).click();
  await expect(page.getByTestId("tax-approval")).toBeVisible();
  expect(hub.intents).toEqual([]);
});

test("a counter member approves with a PIN and the save carries the ticket", async ({ page }) => {
  const hub = await open(page, "counter-token");
  await field(page, "hub.settings.tax.sac").fill("999692");
  await save(page).click();
  const pin = page.getByTestId("tax-approval");
  for (const d of "1234") await pin.getByRole("button", { name: d, exact: true }).click();
  await pin.getByRole("button", { name: en["ui.pin.key_submit"] }).click();
  await expect(page.getByTestId("tax-status")).toContainText(en["hub.settings.saved"]);
  expect(hub.intents.map((i) => i.ticket)).toEqual(["T1"]);
});

test("a server refusal of the GST rate shows on the product rate field", async ({ page }) => {
  const hub = await open(page);
  await field(page, "hub.settings.tax.product").fill("28.01");
  await save(page).click();
  await expect(field(page, "hub.settings.tax.product")).toHaveAttribute("aria-invalid", "true");
  await field(page, "hub.settings.tax.play").fill("abc");
  await save(page).click();
  await expect(field(page, "hub.settings.tax.play")).toHaveAttribute("aria-invalid", "true");
  expect(hub.intents).toHaveLength(1);
});

test("after a stale refusal the form shows the Hub's value, not the old edit", async ({ page }) => {
  const hub = await open(page);
  hub.tax = { value: JSON.stringify({ ...DEFAULT, sac: "111111" }), rev: 2 };
  await field(page, "hub.settings.tax.sac").fill("999692");
  await save(page).click();
  await expect(field(page, "hub.settings.tax.sac")).toHaveValue("111111");
});
