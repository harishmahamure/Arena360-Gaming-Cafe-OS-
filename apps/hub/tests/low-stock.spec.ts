import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type { LowStockView, SessionStaff, SnapshotAlert } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { CORS, HUB, device, json as ok, snapshot } from "./floor-fixture";

// Low stock (Story 5.8): Inventory → Low stock with a threshold field per tracked product, the
// inline manager PIN for a Counter, and the header alert ("Low stock: <product> (<qty> left)")
// that sounds and, when picked, acknowledges and opens the page. The Hub's loopback API is
// stubbed with `page.route`.

const STAFF: Record<string, SessionStaff> = {
  "manager-token": {
    staff_id: "01JMANAGER0000000000000000",
    display_name: "Manager",
    role_name: "Manager",
    permissions: ["catalog.edit"],
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
  view: LowStockView;
  alerts: SnapshotAlert[];
  intents: { payload: Payload; ticket?: string }[];
  elevations: { items: { intent_kind: string }[]; pin: string }[];
  fail: { status: number; code: string; field?: string } | null;
};

const item = (id: string, product: string, qty: number, low: number | null) => ({
  product_id: id,
  product,
  qty,
  low_stock_qty: low,
});

const newHub = (): Hub => {
  const cola = item("P1", "Cola", 4, 5);
  const chips = item("P2", "Chips", 20, null);
  return {
    view: { low: [cola], tracked: [chips, cola] },
    alerts: [],
    intents: [],
    elevations: [],
    fail: null,
  };
};

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
      case "/ui/stock/low":
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
          intent: { payload: Payload };
          elevation_ticket?: string;
        };
        const { payload } = body.intent;
        hub.intents.push({ payload, ticket: body.elevation_ticket });
        if (hub.fail) return reply(route, hub.fail.status, { message: "no", ...hub.fail });
        if (payload.kind === "stock.threshold") {
          const set = payload.low_stock_qty as number | null;
          for (const row of hub.view.tracked) {
            if (row.product_id === payload.product_id) row.low_stock_qty = set;
          }
          hub.view.low = hub.view.tracked.filter(
            (r) => r.low_stock_qty !== null && r.qty <= r.low_stock_qty,
          );
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

test("Low stock lists the products at or under their threshold and a field per tracked product", async ({
  page,
}) => {
  await open(page, newHub(), "/inventory/low-stock");
  const low = page.getByTestId("low-stock-row");
  await expect(low).toHaveCount(1);
  await expect(low).toContainText("Cola");
  await expect(low).toContainText("4 left · threshold 5");
  const rows = page.getByTestId("threshold-row");
  await expect(rows).toHaveCount(2);
  await expect(page.getByLabel("Low-stock threshold for Cola")).toHaveValue("5");
  await expect(page.getByLabel("Low-stock threshold for Chips")).toHaveValue("");
  await expectNoSeriousAxe(page);
});

test("a manager sets a threshold and the product joins the list", async ({ page }) => {
  const hub = newHub();
  await open(page, hub, "/inventory/low-stock");
  await page.getByLabel("Low-stock threshold for Chips").fill("25");
  await page.getByRole("button", { name: "Save threshold for Chips" }).click();
  await expect(page.getByTestId("low-stock-status")).toContainText(en["hub.lowstock.saved"]);
  expect(hub.intents).toEqual([
    { payload: { kind: "stock.threshold", product_id: "P2", low_stock_qty: 25 } },
  ]);
  await expect(page.getByTestId("low-stock-row")).toHaveCount(2);
});

test("an empty field clears the threshold and a bad value is refused before sending", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub, "/inventory/low-stock");
  const cola = page.getByLabel("Low-stock threshold for Cola");
  await cola.fill("-1");
  await page.getByRole("button", { name: "Save threshold for Cola" }).click();
  await expect(page.getByText(en["hub.lowstock.invalid"])).toBeVisible();
  expect(hub.intents).toEqual([]);
  await cola.fill("");
  await page.getByRole("button", { name: "Save threshold for Cola" }).click();
  await expect(page.getByTestId("low-stock-status")).toContainText(en["hub.lowstock.saved"]);
  expect(hub.intents).toEqual([
    { payload: { kind: "stock.threshold", product_id: "P1", low_stock_qty: null } },
  ]);
  await expect(page.getByTestId("low-stock-row")).toHaveCount(0);
  await expect(page.getByText(en["hub.lowstock.empty"])).toBeVisible();
});

test("a Counter's change asks for a manager's PIN in place", async ({ page }) => {
  const hub = newHub();
  await open(page, hub, "/inventory/low-stock", "counter-token");
  await page.getByLabel("Low-stock threshold for Chips").fill("30");
  await page.getByRole("button", { name: "Save threshold for Chips" }).click();
  await expect(page.getByTestId("low-stock-approval")).toBeVisible();
  expect(hub.intents).toEqual([]);
  await page.keyboard.type("1111");
  await page.keyboard.press("Enter");
  await expect(page.getByText(en["ui.pin.rejected"])).toBeVisible();
  expect(hub.intents).toEqual([]);
  await page.keyboard.type("2222");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("low-stock-status")).toContainText(en["hub.lowstock.saved"]);
  expect(hub.elevations.at(-1)).toEqual({
    items: [{ intent_kind: "stock.threshold" }],
    pin: "2222",
  });
  expect(hub.intents).toEqual([
    {
      payload: { kind: "stock.threshold", product_id: "P2", low_stock_qty: 30 },
      ticket: "T1",
    },
  ]);
});

test("a threshold the product already has is reported", async ({ page }) => {
  const hub = newHub();
  hub.fail = { status: 409, code: "STALE", field: "low_stock_qty" };
  await open(page, hub, "/inventory/low-stock");
  await page.getByLabel("Low-stock threshold for Cola").fill("5");
  await page.getByRole("button", { name: "Save threshold for Cola" }).click();
  await expect(page.getByTestId("low-stock-status")).toContainText(en["hub.lowstock.stale"]);
});

test("the low-stock alert names the product, sounds, and opens Low stock when picked", async ({
  page,
}) => {
  const hub = newHub();
  hub.alerts = [
    {
      id: "A1",
      kind: "stock_low",
      device_id: null,
      raised_at: Date.now() - 60_000,
      product_id: "P1",
      product: "Cola",
      qty: 4,
      threshold: 5,
    },
  ] as SnapshotAlert[];
  await open(page, hub, "/floor");
  await expect(page.getByTestId("hub-alerts")).toBeVisible();
  await expect
    .poll(() => page.evaluate(() => (window as unknown as { __plays: string[] }).__plays.length))
    .toBeGreaterThan(0);
  await page.getByTestId("hub-alerts").click();
  const row = page.getByTestId("alert-row");
  await expect(row).toContainText("Low stock: Cola (4 left)");
  await row.getByRole("button").click();
  await expect(page).toHaveURL(/\/inventory\/low-stock$/);
  await expect(page.getByTestId("low-stock-row")).toHaveCount(1);
  expect(hub.intents.map((i) => i.payload)).toEqual([{ kind: "alert.ack", alert_id: "A1" }]);
});
