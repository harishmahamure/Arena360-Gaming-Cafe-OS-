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
const MANAGER: SessionStaff = {
  staff_id: "01JMANAGER0000000000000000",
  display_name: "Manager",
  role_name: "Manager",
  permissions: ["settings.edit"],
};
const GST = JSON.stringify({
  country: "IN",
  gst: true,
  gstin: "27ABCDE1234F1Z5",
  inclusive: true,
  product_bp: 1800,
  play_bp: 1800,
  sac: "999692",
  label: "GST",
  rate_bp: 1800,
});
const PRINTER = JSON.stringify({ kind: "lan", id_or_addr: "192.168.1.50:9100", name: "Counter" });

type Sent = { key: string; value: string; base_rev: number };
type Hub = { intents: Sent[]; tests: number; rows: Map<string, { value: string; rev: number }> };

function rows(tier: string, extra: Record<string, string> = {}) {
  const base: Record<string, string> = {
    "licence.tier": tier,
    "branch.name": "Arena 360 Pune",
    "receipt.footer": "",
    "receipt.address": "",
    "receipt.phone": "",
    "receipt.default": "print",
    "receipt.drawer": "false",
    "printer.selected": PRINTER,
    "printer.paper_mm": "80",
    "notify.whatsapp_receipt": "false",
    "notify.credit_reminder": "false",
    "notify.low_balance": "true",
    "notify.session_ending": "false",
    ...extra,
  };
  return new Map(Object.entries(base).map(([k, value]) => [k, { value, rev: 1 }]));
}

const json = (route: Route, status: number, body: unknown) =>
  route.fulfill({
    status,
    contentType: "application/json",
    headers: CORS,
    body: JSON.stringify(body),
  });

const SNAPSHOT: HubSnapshot = {
  branch: { name: "Arena 360 Pune", branch_code: "DEV01" },
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

async function open(
  page: Page,
  path: string,
  tier: string,
  extra: Record<string, string> = {},
  refuse: { status: number; code: string } | null = null,
): Promise<Hub> {
  const hub: Hub = { intents: [], tests: 0, rows: rows(tier, extra) };
  await page.addInitScript(() => sessionStorage.setItem("arena.hub.token", "manager-token"));
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot":
        return json(route, 200, SNAPSHOT);
      case "/ui/me":
        return json(route, 200, MANAGER);
      case "/ui/settings":
        return json(
          route,
          200,
          [...hub.rows].map(([key, row]) => ({ key, ...row })),
        );
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: "retry: 400\n\n",
        });
      case "/ui/printer/test":
        hub.tests++;
        return json(route, 200, {
          status: { state: "ok", error_kind: null, error: null, at: 1 },
        });
      case "/ui/intent": {
        const { intent } = req.postDataJSON() as {
          intent: { base_rev: number; payload: { key: string; value: string } };
        };
        if (refuse) return json(route, refuse.status, { code: refuse.code, message: "no" });
        const { key, value } = intent.payload;
        hub.intents.push({ key, value, base_rev: intent.base_rev });
        hub.rows.set(key, { value, rev: hub.rows.get(key)!.rev + 1 });
        return json(route, 200, { applied: { intent_id: "I", changes: [] } });
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto(path);
  return hub;
}

async function expectNoSeriousAxe(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  expect(bad.map((v) => v.id)).toEqual([]);
}

const save = (page: Page) => page.getByRole("button", { name: en["hub.settings.save"] });
const previewLines = async (page: Page) =>
  ((await page.getByTestId("receipt-preview").textContent()) ?? "").split("\n");

test("Receipts shows the printer, paper, header, footer, default and a preview", async ({
  page,
}) => {
  await open(page, "/settings/receipt", "basic", { "branch.tax": GST });
  await expect(page.getByTestId("receipt-printer-name")).toContainText("Counter");
  await expect(
    page.getByRole("link", { name: en["hub.settings.receipt.printer_find"] }),
  ).toHaveAttribute("href", "/settings/printer");
  await expect(page.getByLabel(en["hub.settings.printer.paper_80"])).toBeChecked();
  await expect(page.getByTestId("receipt-branch-name")).toContainText("Arena 360 Pune");
  await expect(page.getByLabel(en["hub.settings.receipt.default_print"])).toBeChecked();
  const preview = page.getByTestId("receipt-preview");
  await expect(preview).toContainText("GSTIN 27ABCDE1234F1Z5");
  await expect(preview).toContainText("Arena 360 Pune");
  await expectNoSeriousAxe(page);
});

test("the preview wraps at the paper width exactly as it prints", async ({ page }) => {
  await open(page, "/settings/receipt", "free");
  const long = "Thank you for playing at Arena 360 and see you again soon";
  await page.getByLabel(en["hub.settings.receipt_footer"]).fill(long);
  await page.getByLabel(en["hub.settings.printer.paper_58"]).check();
  let lines = await previewLines(page);
  expect(lines.every((l) => l.length <= 32)).toBe(true);
  expect(lines.some((l) => l.trim() === "Thank you for playing at Arena")).toBe(true);
  await page.getByLabel(en["hub.settings.printer.paper_80"]).check();
  lines = await previewLines(page);
  expect(lines.every((l) => l.length <= 48)).toBe(true);
  expect(lines.some((l) => l.trim() === long)).toBe(false);
  expect(lines.some((l) => l.trim().startsWith("Thank you for playing at Arena 360"))).toBe(true);
});

test("a footer over three lines cannot be saved", async ({ page }) => {
  const hub = await open(page, "/settings/receipt", "free");
  await page.getByLabel(en["hub.settings.receipt_footer"]).fill("a\nb\nc\nd");
  await expect(
    page.getByText(en["hub.settings.receipt.lines_over"].replace("{max}", "3")),
  ).toBeVisible();
  await expect(save(page)).toHaveAttribute("aria-disabled", "true");
  expect(hub.intents).toEqual([]);
});

test("Save sends one settings.set per changed key with base_rev", async ({ page }) => {
  const hub = await open(page, "/settings/receipt", "free");
  await expect(save(page)).toHaveAttribute("aria-disabled", "true");
  await page.getByLabel(en["hub.settings.receipt_footer"]).fill("Thanks!\nCome again");
  await page.getByLabel(en["hub.settings.receipt.address"]).fill("1 High St");
  await page.getByLabel(en["hub.settings.receipt.phone"]).fill("+91 98765 43210");
  await page.getByLabel(en["hub.settings.receipt.default_none"]).check();
  await page.getByLabel(en["hub.settings.printer.paper_58"]).check();
  await save(page).click();
  await expect(page.getByTestId("settings-status")).toHaveText(en["hub.settings.saved"]);
  expect(hub.intents).toEqual([
    { key: "receipt.footer", value: "Thanks!\nCome again", base_rev: 1 },
    { key: "receipt.address", value: "1 High St", base_rev: 1 },
    { key: "receipt.phone", value: "+91 98765 43210", base_rev: 1 },
    { key: "receipt.default", value: "none", base_rev: 1 },
    { key: "printer.paper_mm", value: "58", base_rev: 1 },
  ]);
  await expect(save(page)).toHaveAttribute("aria-disabled", "true");
});

test("the cash drawer switch is off by default and saves with base_rev", async ({ page }) => {
  const hub = await open(page, "/settings/receipt", "free");
  const drawer = page.getByLabel(en["hub.settings.receipt.drawer"]);
  await expect(drawer).not.toBeChecked();
  await expect(page.getByText(en["hub.settings.receipt.drawer_hint"])).toBeVisible();
  await drawer.check();
  await save(page).click();
  await expect(page.getByTestId("settings-status")).toHaveText(en["hub.settings.saved"]);
  expect(hub.intents).toEqual([{ key: "receipt.drawer", value: "true", base_rev: 1 }]);
  await expect(drawer).toBeChecked();
});

test("Test print runs on the saved paper width only", async ({ page }) => {
  const hub = await open(page, "/settings/receipt", "free");
  const run = page.getByRole("button", { name: en["hub.settings.printer.test"] });
  await run.click();
  await expect(page.getByTestId("settings-status")).toContainText(
    en["hub.settings.printer.result.ok"],
  );
  expect(hub.tests).toBe(1);
  await page.getByLabel(en["hub.settings.printer.paper_58"]).check();
  await expect(run).toHaveAttribute("aria-disabled", "true");
});

test("a Free branch sees a tier lock on each message and no switch", async ({ page }) => {
  await open(page, "/settings/notifications", "free");
  await expect(page.getByTestId("notifications-intro")).toHaveText(
    en["hub.settings.notifications.intro"],
  );
  for (const id of ["whatsapp_receipt", "credit_reminder", "low_balance", "session_ending"]) {
    await expect(page.getByTestId(`notify-lock-${id}`)).toContainText(
      en["hub.settings.notifications.locked"],
    );
    await expect(page.getByTestId(`notify-preview-${id}`)).toContainText("Asha");
  }
  await expect(page.getByRole("switch")).toHaveCount(0);
  await expect(page.getByRole("textbox")).toHaveCount(0);
  await expectNoSeriousAxe(page);
});

test("a paid branch switches a message on and off with base_rev", async ({ page }) => {
  const hub = await open(page, "/settings/notifications", "basic");
  await expect(page.getByRole("switch")).toHaveCount(4);
  await expect(page.getByTestId("notify-low_balance").getByRole("switch")).toBeChecked();
  await expect(page.getByTestId("notify-preview-low_balance")).toContainText("Arena 360 Pune");
  await page.getByTestId("notify-whatsapp_receipt").getByRole("switch").check();
  await expect(page.getByTestId("settings-status")).toHaveText(en["hub.settings.saved"]);
  await page.getByTestId("notify-low_balance").getByRole("switch").uncheck();
  await expect.poll(() => hub.intents.length).toBe(2);
  expect(hub.intents).toEqual([
    { key: "notify.whatsapp_receipt", value: "true", base_rev: 1 },
    { key: "notify.low_balance", value: "false", base_rev: 1 },
  ]);
  await expectNoSeriousAxe(page);
});

test("a lapse refuses the write with the plan message and shows what the Hub holds", async ({
  page,
}) => {
  await open(page, "/settings/notifications", "basic", {}, { status: 403, code: "TIER_LOCKED" });
  await page.getByTestId("notify-whatsapp_receipt").getByRole("switch").check();
  await expect(page.getByTestId("settings-status")).toContainText(
    en["hub.settings.notifications.failed_locked"],
  );
  await expect(page.getByTestId("notify-whatsapp_receipt").getByRole("switch")).not.toBeChecked();
});
