import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Locator, type Page, type Route } from "@playwright/test";
import type {
  FoundPrinter,
  HubSnapshot,
  PrinterErrorKind,
  PrinterStatus,
  SessionStaff,
} from "@arena/ui/generated";
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
const AUTH_FAILED = { code: "AUTH_FAILED", message: "sign-in failed" };
const UNSET: PrinterStatus = { state: "unset", error_kind: null, error: null, at: null };
const AT = Date.UTC(2027, 0, 15, 6, 30);
const FOUND: FoundPrinter[] = [
  { id: "fake-usb", kind: "usb", name: "Fake USB printer", addr: null },
  {
    id: "lan:192.168.1.250:9100",
    kind: "lan",
    name: "Fake network printer",
    addr: "192.168.1.250:9100",
  },
];

type Sent = { kind: string; key?: string; value?: string; base_rev?: number };
type Hub = {
  settings: Map<string, { value: string; rev: number }>;
  intents: Sent[];
  calls: string[];
  found: FoundPrinter[];
  findDelayMs: number;
  printFails: PrinterErrorKind | null;
  notRecorded: boolean;
  refuse: { status: number; body: unknown } | null;
  hc: boolean;
};

function newHub(): Hub {
  return {
    settings: new Map([
      ["idle_lock_minutes", { value: "5", rev: 1 }],
      ["printer.selected", { value: "", rev: 1 }],
      ["printer.paper_mm", { value: "80", rev: 1 }],
      ["printer.status", { value: JSON.stringify(UNSET), rev: 1 }],
    ]),
    intents: [],
    calls: [],
    found: FOUND,
    findDelayMs: 0,
    printFails: null,
    notRecorded: false,
    refuse: null,
    hc: false,
  };
}

function printerState(hub: Hub): PrinterStatus["state"] {
  return (JSON.parse(hub.settings.get("printer.status")!.value) as PrinterStatus).state;
}

function snapshot(hub: Hub): HubSnapshot {
  return {
    branch: { name: "Dev Branch", branch_code: "DEV01" },
    groups: [],
    devices: [],
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
    ui: { idle_lock_minutes: 5, hc: hub.hc },
    connectivity: { state: "online", checked_at: null },
    lan: null,
    paired: true,
    setup: {
      opening_hours_set: false,
      backup_location_set: false,
      ad_consent: "missing",
      printer: printerState(hub),
    },
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

function record(hub: Hub, status: PrinterStatus) {
  const row = hub.settings.get("printer.status")!;
  hub.settings.set("printer.status", { value: JSON.stringify(status), rev: row.rev + 1 });
  return { outcome: "recorded", status };
}

async function stub(page: Page, hub: Hub, token: string) {
  await page.addInitScript((t) => sessionStorage.setItem("arena.hub.token", t), token);
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    const url = new URL(req.url());
    const auth = (req.headers()["authorization"] ?? "").replace(/^Bearer /, "");
    const staff = STAFF[auth];
    switch (url.pathname) {
      case "/ui/snapshot":
        return json(route, 200, snapshot(hub));
      case "/ui/me":
        return staff ? json(route, 200, staff) : json(route, 401, AUTH_FAILED);
      case "/ui/settings":
        if (!staff) return json(route, 401, AUTH_FAILED);
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
          body: "retry: 400\n\n",
        });
      case "/ui/printer/find":
      case "/ui/printer/test":
      case "/ui/printer/skip": {
        if (!staff) return json(route, 401, AUTH_FAILED);
        hub.calls.push(url.pathname);
        if (!staff.permissions.includes("settings.edit")) {
          return json(route, 403, { code: "PERMISSION_DENIED", message: "denied" });
        }
        if (url.pathname === "/ui/printer/find") {
          await new Promise((r) => setTimeout(r, hub.findDelayMs));
          return json(route, 200, { printers: hub.found });
        }
        if (url.pathname === "/ui/printer/skip") {
          return json(
            route,
            200,
            record(hub, { state: "skipped", error_kind: null, error: null, at: AT }),
          );
        }
        if (!hub.settings.get("printer.selected")!.value) {
          return json(route, 422, {
            code: "VALIDATION_FAILED",
            message: "choose a printer",
            field: "printer.selected",
          });
        }
        if (hub.notRecorded) {
          return json(route, 200, { outcome: "printed_not_recorded", status: null });
        }
        const status: PrinterStatus = hub.printFails
          ? {
              state: "failed",
              error_kind: hub.printFails,
              error: "The printer at 192.168.1.250:9100 refused the connection",
              at: AT,
            }
          : { state: "ok", error_kind: null, error: null, at: AT };
        return json(route, 200, record(hub, status));
      }
      case "/ui/intent": {
        if (!staff) return json(route, 401, AUTH_FAILED);
        const { intent } = req.postDataJSON() as {
          intent: { base_rev?: number; payload: { kind: string; key?: string; value?: string } };
        };
        const p = intent.payload;
        hub.intents.push({ ...p, base_rev: intent.base_rev });
        if (hub.refuse) {
          const answer = hub.refuse;
          hub.refuse = null;
          return json(route, answer.status, answer.body);
        }
        if (!staff.permissions.includes("settings.edit")) {
          return json(route, 403, { code: "PERMISSION_DENIED", message: "denied" });
        }
        const row = hub.settings.get(p.key!)!;
        if (intent.base_rev !== row.rev) {
          return json(route, 409, { code: "STALE", message: "stale", field: "base_rev" });
        }
        let value = p.value!;
        if (p.key === "printer.selected" && value) {
          const sel = JSON.parse(value) as { kind: string; id_or_addr: string; name: string };
          if (sel.kind === "lan" && !sel.id_or_addr.includes(":")) sel.id_or_addr += ":9100";
          value = JSON.stringify(sel);
        }
        hub.settings.set(p.key!, { value, rev: row.rev + 1 });
        return json(route, 200, { applied: { intent_id: "I", changes: [] } });
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
}

async function open(page: Page, hub: Hub, token = "manager-token") {
  await stub(page, hub, token);
  await page.goto("/settings/printer");
  await expect(page).toHaveURL(/\/settings\/printer$/);
  await expect(page.getByTestId("settings-printer")).toBeVisible();
}

const button = (page: Page, key: keyof typeof en) => page.getByRole("button", { name: en[key] });

async function expectNoSeriousAxe(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  expect(bad.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual(
    [],
  );
}

async function expectTarget(target: Locator) {
  const box = await target.boundingBox();
  expect(box!.height).toBeGreaterThanOrEqual(44);
}

test("Find lists the printers, picking one and 58 mm saves them, and Test print reports OK", async ({
  page,
}) => {
  const hub = newHub();
  hub.findDelayMs = 600;
  await open(page, hub);
  const sections = page.getByRole("navigation", { name: en["hub.settings.sections"] });
  await expect(
    sections.getByRole("link", { name: en["hub.settings.nav.printer"] }),
  ).toHaveAttribute("aria-current", "page");
  await expect(page.getByTestId("printer-selected")).toHaveText(
    en["hub.settings.printer.none_selected"],
  );
  await expect(page.getByTestId("printer-last-status")).toHaveText(
    en["hub.settings.printer.status.unset"],
  );
  await expect(button(page, "hub.settings.printer.test")).toHaveAttribute("aria-disabled", "true");
  await expect(page.getByText(en["hub.settings.printer.choose_first"])).toBeVisible();
  const find = button(page, "hub.settings.printer.find");
  await expectTarget(find);
  await find.click();
  await expect(find).toHaveAttribute("aria-disabled", "true");
  await expect(page.getByTestId("printer-find-status")).toHaveText(
    en["hub.settings.printer.finding"],
  );
  const list = page.getByTestId("printer-found");
  await expect(list.getByRole("radio")).toHaveCount(2);
  await expect(find).not.toHaveAttribute("aria-disabled", "true");
  const usb = page.getByLabel("Fake USB printer · USB");
  const lan = page.getByLabel("Fake network printer · Network · 192.168.1.250");
  await expect(usb).toBeVisible();
  await expectTarget(page.locator("label").filter({ hasText: "Fake network printer" }));
  await expectNoSeriousAxe(page);
  await lan.check();
  await expect(page.getByTestId("printer-selected")).toHaveText("Printer: Fake network printer");
  const narrow = page.getByLabel(en["hub.settings.printer.paper_58"]);
  await narrow.focus();
  await page.keyboard.press("Space");
  await expect(narrow).toBeChecked();
  await expect.poll(() => hub.intents.length).toBe(2);
  await expect(page.getByTestId("printer-status")).toHaveText(en["hub.settings.saved"]);
  await expect(narrow).toBeFocused();
  await expect(narrow).toBeEnabled();
  expect(hub.intents).toEqual([
    {
      kind: "settings.set",
      key: "printer.selected",
      value: JSON.stringify({
        kind: "lan",
        id_or_addr: "192.168.1.250:9100",
        name: "Fake network printer",
      }),
      base_rev: 1,
    },
    { kind: "settings.set", key: "printer.paper_mm", value: "58", base_rev: 1 },
  ]);
  await expect(lan).toBeChecked();
  await button(page, "hub.settings.printer.test").click();
  const status = page.getByTestId("printer-status");
  await expect(status).toHaveAttribute("role", "status");
  await expect(status).toHaveText(en["hub.settings.printer.result.ok"]);
  expect(hub.calls).toEqual(["/ui/printer/find", "/ui/printer/test"]);
  await expect(page.getByTestId("printer-last-status")).toContainText(
    en["hub.settings.printer.status.ok"].split(",")[0],
  );
});

test("Find with nothing on the network says so", async ({ page }) => {
  const hub = newHub();
  hub.found = [];
  await open(page, hub);
  await button(page, "hub.settings.printer.find").click();
  await expect(page.getByTestId("printer-find-status")).toHaveText(
    en["hub.settings.printer.found_none"],
  );
  await expect(page.getByTestId("printer-found")).toHaveCount(0);
});

test("Add by IP shows the Hub's address error inline, then adds the printer", async ({ page }) => {
  const hub = newHub();
  await open(page, hub);
  const field = page.getByLabel(en["hub.settings.printer.ip"]);
  await expectTarget(field);
  const add = button(page, "hub.settings.printer.add");
  await add.click();
  await expect(page.locator("#printer-ip-error")).toContainText(
    en["hub.settings.printer.ip_empty"],
  );
  expect(hub.intents).toEqual([]);
  await field.fill("abc");
  hub.refuse = {
    status: 422,
    body: { code: "VALIDATION_FAILED", message: "bad address", field: "ip" },
  };
  await add.click();
  await expect(page.locator("#printer-ip-error")).toContainText(
    en["hub.settings.printer.invalid.ip"],
  );
  await expect(field).toHaveAttribute("aria-invalid", "true");
  await expect(field).toBeFocused();
  await expectNoSeriousAxe(page);
  await field.fill("192.168.1.50");
  await field.press("Enter");
  await expect(page.getByTestId("printer-selected")).toHaveText("Printer: 192.168.1.50");
  await expect(page.locator("#printer-ip-error")).toHaveText("");
  expect(hub.intents.at(-1)).toEqual({
    kind: "settings.set",
    key: "printer.selected",
    value: JSON.stringify({ kind: "lan", id_or_addr: "192.168.1.50", name: "192.168.1.50" }),
    base_rev: 1,
  });
  expect(JSON.parse(hub.settings.get("printer.selected")!.value).id_or_addr).toBe(
    "192.168.1.50:9100",
  );
});

test("a failed test print shows the printer's error inline", async ({ page }) => {
  const hub = newHub();
  hub.settings.set("printer.selected", {
    value: JSON.stringify({ kind: "lan", id_or_addr: "192.168.1.250:9100", name: "192.168.1.250" }),
    rev: 2,
  });
  hub.printFails = "refused";
  await open(page, hub);
  await button(page, "hub.settings.printer.test").click();
  const status = page.getByTestId("printer-status");
  await expect(status).toContainText(
    en["hub.settings.printer.result.failed"].replace(
      "{error}",
      en["hub.settings.printer.error.refused"],
    ),
  );
  await expect(page.getByTestId("printer-last-status")).toContainText(
    en["hub.settings.printer.error.refused"],
  );
  await expect(page.getByText("The printer at 192.168.1.250:9100", { exact: false })).toHaveCount(
    0,
  );
  expect(printerState(hub)).toBe("failed");
  await expectNoSeriousAxe(page);
  hub.printFails = "timeout";
  await button(page, "hub.settings.printer.test").click();
  await expect(status).toContainText(en["hub.settings.printer.error.timeout"]);
});

test("a page that printed but wasn't recorded says so", async ({ page }) => {
  const hub = newHub();
  hub.settings.set("printer.selected", {
    value: JSON.stringify({ kind: "usb", id_or_addr: "fake-usb", name: "Fake USB printer" }),
    rev: 2,
  });
  hub.notRecorded = true;
  await open(page, hub);
  await button(page, "hub.settings.printer.test").click();
  await expect(page.getByTestId("printer-status")).toContainText(
    en["hub.settings.printer.result.printed_not_recorded"],
  );
  expect(printerState(hub)).toBe("unset");
});

test("Skip printer for now records skipped and says nothing else needs a printer", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub);
  await expect(page.getByTestId("printer-skip-note")).toHaveText(
    en["hub.settings.printer.skip_note"],
  );
  const skip = button(page, "hub.settings.printer.skip");
  await expectTarget(skip);
  await skip.click();
  await expect(page.getByTestId("printer-status")).toHaveText(
    en["hub.settings.printer.result.skipped"],
  );
  expect(hub.calls).toEqual(["/ui/printer/skip"]);
  expect(printerState(hub)).toBe("skipped");
});

test("Payments says UPI and card are recorded by reference and asks for no keys", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub);
  await expect(
    page.getByRole("heading", { name: en["hub.settings.payments.title"] }),
  ).toBeVisible();
  await expect(page.getByTestId("payments-text")).toHaveText(en["hub.settings.payments.text"]);
  await expect(page.getByRole("textbox")).toHaveCount(1);
  await expect(page.getByRole("textbox")).toHaveAccessibleName(en["hub.settings.printer.ip"]);
  await expect(page.locator('input[type="password"]')).toHaveCount(0);
});

test("a Counter sees the printer read-only and can't find, test or skip", async ({ page }) => {
  const hub = newHub();
  await open(page, hub, "counter-token");
  for (const key of [
    "hub.settings.printer.find",
    "hub.settings.printer.add",
    "hub.settings.printer.test",
    "hub.settings.printer.skip",
  ] as const) {
    await expect(button(page, key)).toHaveAttribute("aria-disabled", "true");
    await button(page, key).click({ force: true });
  }
  await expect(page.getByText(en["hub.settings.printer.manager_only"]).first()).toBeVisible();
  await expect(page.getByLabel(en["hub.settings.printer.ip"])).toBeDisabled();
  await expect(page.getByLabel(en["hub.settings.printer.paper_58"])).toBeDisabled();
  expect(hub.calls).toEqual([]);
  expect(hub.intents).toEqual([]);
});

for (const hc of [false, true]) {
  test(`the printer and payments screen passes axe in the ${hc ? "high-contrast" : "dark"} theme`, async ({
    page,
  }) => {
    const hub = newHub();
    hub.hc = hc;
    await open(page, hub);
    await expect
      .poll(() => page.evaluate(() => document.documentElement.dataset.theme))
      .toBe(hc ? "hc" : "dark");
    await expectNoSeriousAxe(page);
    await button(page, "hub.settings.printer.find").click();
    await expect(page.getByTestId("printer-found").getByRole("radio")).toHaveCount(2);
    await expectNoSeriousAxe(page);
  });
}
