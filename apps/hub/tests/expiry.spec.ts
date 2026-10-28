import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type { ExpiryView, SessionStaff, SnapshotAlert } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { CORS, HUB, device, json as ok, snapshot } from "./floor-fixture";

// Expiring soon and Expired stock (Story 5.7): the two Inventory lists, the "Warn before expiry"
// setting, the write-off with an inline manager PIN for a Counter, and the header alert whose
// pick opens the Expiring list. The Hub's loopback API is stubbed with `page.route`.

const STAFF: Record<string, SessionStaff> = {
  "manager-token": {
    staff_id: "01JMANAGER0000000000000000",
    display_name: "Manager",
    role_name: "Manager",
    permissions: ["settings.edit", "stock.adjust"],
  },
  "counter-token": {
    staff_id: "01JCOUNTER0000000000000000",
    display_name: "Counter",
    role_name: "Counter",
    permissions: ["stock.in"],
  },
};

type Payload = Record<string, unknown> & { kind: string };
type Hub = {
  view: ExpiryView;
  alerts: SnapshotAlert[];
  warnRev: number;
  intents: { payload: Payload; base_rev?: number; ticket?: string }[];
  elevations: { items: { intent_kind: string }[]; pin: string }[];
  fail: { status: number; code: string; field?: string } | null;
};

const lot = (id: string, product: string, qty: number, expiry: string, value = 1000) => ({
  lot_id: id,
  product_id: "P".padEnd(26, "0"),
  product,
  qty,
  expiry,
  value_minor: value,
});

const newHub = (): Hub => ({
  view: {
    today: "2027-01-15",
    warn_days: 3,
    expiring: [lot("L1", "Milk", 4, "2027-01-16", 60000), lot("L2", "Bread", 2, "2027-01-18")],
    expired: [lot("L3", "Yoghurt", 6, "2027-01-10", 24000)],
  },
  alerts: [],
  warnRev: 4,
  intents: [],
  elevations: [],
  fail: null,
});

const reply = (route: Route, status: number, body: unknown) =>
  route.fulfill({
    status,
    contentType: "application/json",
    headers: CORS,
    body: JSON.stringify(body),
  });

async function open(page: Page, hub: Hub, path: string, token = "manager-token") {
  await page.addInitScript((t) => sessionStorage.setItem("arena.hub.token", t), token);
  await page.addInitScript(() => {
    const w = window as unknown as { __plays: string[] };
    w.__plays = [];
    HTMLMediaElement.prototype.play = function (this: HTMLMediaElement) {
      w.__plays.push(this.src);
      return Promise.resolve();
    };
  });
  await page.route(`${HUB}/**`, async (route: Route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot": {
        const snap = snapshot([device(1, {})]);
        snap.alerts = hub.alerts;
        return ok(route, snap);
      }
      case "/ui/me":
        return ok(route, { ...STAFF[token], locked: false, has_pin: true });
      case "/ui/settings":
        return ok(route, [
          { key: "stock.expiry_warn_days", value: String(hub.view.warn_days), rev: hub.warnRev },
        ]);
      case "/ui/stock/expiry":
        return ok(route, hub.view);
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: `retry: 300\n\n`,
        });
      case "/ui/elevate": {
        const body = req.postDataJSON() as Hub["elevations"][number];
        hub.elevations.push(body);
        if (body.pin !== "2222") return reply(route, 403, { code: "PIN_REJECTED", message: "no" });
        return reply(route, 200, { ticket: "T1", items: body.items, expires_in_s: 60 });
      }
      case "/ui/elevate/drop":
        return route.fulfill({ status: 204, headers: CORS });
      case "/ui/intent": {
        const body = req.postDataJSON() as {
          intent: { payload: Payload; base_rev?: number };
          elevation_ticket?: string;
        };
        const { payload, base_rev } = body.intent;
        hub.intents.push({ payload, base_rev, ticket: body.elevation_ticket });
        if (hub.fail) return reply(route, hub.fail.status, { message: "no", ...hub.fail });
        if (payload.kind === "stock.write_off") {
          hub.view.expired = hub.view.expired.filter((l) => l.lot_id !== payload.lot_id);
        }
        if (payload.kind === "settings.set") {
          hub.view.warn_days = Number(payload.value);
          hub.warnRev++;
        }
        if (payload.kind === "alert.ack") {
          hub.alerts = hub.alerts.filter((a) => a.id !== payload.alert_id);
        }
        return reply(route, 200, { applied: { intent_id: "I", changes: [] } });
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto(path);
}

async function expectNoSeriousAxe(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  expect(bad.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual(
    [],
  );
}

test("Expiring lists each lot with product, quantity, date and cost", async ({ page }) => {
  await open(page, newHub(), "/inventory/expiring");
  const rows = page.getByTestId("expiring-row");
  await expect(rows).toHaveCount(2);
  await expect(rows.first()).toContainText("Milk");
  await expect(rows.first()).toContainText("4 · expires 2027-01-16");
  await expect(rows.first()).toContainText("₹600.00");
  await expect(page.getByLabel(en["hub.expiry.warn_days"])).toHaveValue("3");
  await expectNoSeriousAxe(page);
});

test("a manager changes the warning days and a bad value is refused before sending", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub, "/inventory/expiring");
  const field = page.getByLabel(en["hub.expiry.warn_days"]);
  await field.fill("31");
  await page.getByRole("button", { name: en["hub.expiry.warn_save"] }).click();
  await expect(page.getByText(en["hub.expiry.warn_invalid"])).toBeVisible();
  expect(hub.intents).toEqual([]);
  await field.fill("7");
  await page.getByRole("button", { name: en["hub.expiry.warn_save"] }).click();
  await expect(page.getByTestId("expiry-status")).toContainText(en["hub.settings.saved"]);
  expect(hub.intents).toEqual([
    { payload: { kind: "settings.set", key: "stock.expiry_warn_days", value: "7" }, base_rev: 4 },
  ]);
  await expect(field).toHaveValue("7");
});

test("without settings permission the warning days are shown, not edited", async ({ page }) => {
  await open(page, newHub(), "/inventory/expiring", "counter-token");
  await expect(page.getByTestId("expiry-warn-value")).toContainText("3");
  await expect(page.getByTestId("expiry-warn-form")).toHaveCount(0);
});

test("a manager writes off an expired lot and it leaves the list", async ({ page }) => {
  const hub = newHub();
  await open(page, hub, "/inventory/expired");
  await expect(page.getByTestId("expired-row")).toHaveCount(1);
  await expect(page.getByTestId("expired-row")).toContainText("Yoghurt");
  await expect(page.getByTestId("expired-row")).toContainText("6 · expired 2027-01-10");
  await expectNoSeriousAxe(page);
  await page.getByTestId("expired-write-off").click();
  await expect(page.getByTestId("expired-status")).toContainText(en["hub.expired.done"]);
  expect(hub.intents).toEqual([{ payload: { kind: "stock.write_off", lot_id: "L3" } }]);
  await expect(page.getByText(en["hub.expired.empty"])).toBeVisible();
});

test("a Counter's write-off asks for a manager's PIN in place", async ({ page }) => {
  const hub = newHub();
  await open(page, hub, "/inventory/expired", "counter-token");
  await page.getByTestId("expired-write-off").click();
  await expect(page.getByTestId("expired-approval")).toBeVisible();
  expect(hub.intents).toEqual([]);
  await page.keyboard.type("1111");
  await page.keyboard.press("Enter");
  await expect(page.getByText(en["ui.pin.rejected"])).toBeVisible();
  expect(hub.intents).toEqual([]);
  await page.keyboard.type("2222");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("expired-status")).toContainText(en["hub.expired.done"]);
  expect(hub.elevations.at(-1)).toEqual({
    items: [{ intent_kind: "stock.write_off" }],
    pin: "2222",
  });
  expect(hub.intents).toEqual([
    { payload: { kind: "stock.write_off", lot_id: "L3" }, ticket: "T1" },
  ]);
});

test("a lot already written off elsewhere is reported as changed", async ({ page }) => {
  const hub = newHub();
  hub.fail = { status: 409, code: "STALE", field: "lot_id" };
  await open(page, hub, "/inventory/expired");
  await page.getByTestId("expired-write-off").click();
  await expect(page.getByTestId("expired-status")).toContainText(en["hub.expired.stale"]);
});

test("the expiring-soon alert sounds, counts in the header and opens the Expiring list", async ({
  page,
}) => {
  const hub = newHub();
  hub.alerts = [
    { id: "A1", kind: "stock_expiring", device_id: null, raised_at: Date.now() - 60_000 },
  ] as SnapshotAlert[];
  await open(page, hub, "/floor");
  await expect(page.getByTestId("hub-alerts")).toBeVisible();
  await expect
    .poll(() => page.evaluate(() => (window as unknown as { __plays: string[] }).__plays.length))
    .toBeGreaterThan(0);
  await page.getByTestId("hub-alerts").click();
  const row = page.getByTestId("alert-row");
  await expect(row).toContainText(en["hub.alerts.kind.stock_expiring"]);
  await row.getByRole("button").click();
  await expect(page).toHaveURL(/\/inventory\/expiring$/);
  await expect(page.getByTestId("expiring-row")).toHaveCount(2);
  expect(hub.intents.map((i) => i.payload)).toEqual([{ kind: "alert.ack", alert_id: "A1" }]);
});
