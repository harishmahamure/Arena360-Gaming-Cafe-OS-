import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type { HubSnapshot, Product, SessionStaff } from "@arena/ui/generated";
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
    permissions: ["stock.in", "stock.adjust"],
  },
  "counter-token": {
    staff_id: "01JCOUNTER0000000000000000",
    display_name: "Counter",
    role_name: "Counter",
    permissions: ["stock.in"],
  },
};

type Payload = Record<string, unknown> & { kind: string };
type Sent = { payload: Payload; reason?: string; ticket?: string };
type Hub = {
  products: Product[];
  intents: Sent[];
  elevations: { items: { intent_kind: string }[]; pin: string }[];
  fail: { status: number; code: string; field?: string } | null;
};

const product = (over: Partial<Product>): Product => ({
  id: "P1".padEnd(26, "0"),
  name: "Cola",
  category: "Drinks",
  price_minor: 2500,
  tax_rate_bp: 500,
  hsn: "2202",
  track_stock: true,
  image: "",
  barcode: "",
  kitchen: false,
  favourite: false,
  active: true,
  bundle: false,
  components: [],
  kitchen_notes: [],
  rev: 1,
  quantity: 10,
  avg_cost_minor: 2000,
  available: true,
  reason: undefined,
  ...over,
});

const newHub = (): Hub => ({
  products: [
    product({}),
    product({ id: "P2".padEnd(26, "0"), name: "Water", track_stock: false }),
    product({ id: "P3".padEnd(26, "0"), name: "Combo", bundle: true, track_stock: false }),
  ],
  intents: [],
  elevations: [],
  fail: null,
});

const snapshot = (): HubSnapshot => ({
  branch: { name: "Dev Branch", branch_code: "DEV01" },
  groups: [],
  zones: [],
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
  ui: { idle_lock_minutes: 5, hc: false },
  connectivity: { state: "online", checked_at: null },
  lan: null,
  paired: true,
});

const json = (route: Route, status: number, body: unknown) =>
  route.fulfill({
    status,
    contentType: "application/json",
    headers: CORS,
    body: JSON.stringify(body),
  });

const applied = (changes: { tbl: string; row_id: string }[]) => ({
  applied: { intent_id: "I", changes },
});

function apply(hub: Hub, p: Payload, route: Route) {
  const row = hub.products.find((x) => x.id === p.product_id);
  if (p.kind === "stock.waste" && row) {
    row.quantity -= p.qty as number;
    return json(
      route,
      200,
      applied([
        { tbl: "stock", row_id: row.id },
        { tbl: "stock_moves", row_id: "W1" },
      ]),
    );
  }
  hub.products.forEach((x) => (x.quantity = 10));
  return json(route, 200, applied([{ tbl: "stock_moves", row_id: "W2" }]));
}

async function open(page: Page, hub: Hub, token = "manager-token") {
  await page.addInitScript((t) => sessionStorage.setItem("arena.hub.token", t), token);
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot":
        return json(route, 200, snapshot());
      case "/ui/me":
        return json(route, 200, STAFF[token]);
      case "/ui/products":
        return json(route, 200, hub.products);
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
        if (body.pin !== "2222") return json(route, 403, { code: "PIN_REJECTED", message: "no" });
        return json(route, 200, { ticket: "T1", items: body.items, expires_in_s: 60 });
      }
      case "/ui/elevate/drop":
        return route.fulfill({ status: 204, headers: CORS });
      case "/ui/intent": {
        const body = req.postDataJSON() as {
          intent: { payload: Payload; reason?: string };
          elevation_ticket?: string;
        };
        const { payload, reason } = body.intent;
        hub.intents.push({ payload, reason, ticket: body.elevation_ticket });
        if (hub.fail) return json(route, hub.fail.status, { message: "no", ...hub.fail });
        return apply(hub, payload, route);
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/inventory/waste");
  await expect(page.getByTestId("waste-form")).toBeVisible();
}

async function expectNoSeriousAxe(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  expect(bad.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual(
    [],
  );
}

const record = async (page: Page, qty: string, reason = "Spoiled") => {
  await page.getByLabel(en["hub.stock.product"]).selectOption({ label: "Cola" });
  await page.getByLabel(en["hub.stock.qty"]).fill(qty);
  await page.getByLabel(en["hub.waste.reason"]).selectOption({ label: reason });
  await page.getByRole("button", { name: en["hub.waste.submit"] }).click();
};

test("the reasons are the four waste reasons and only tracked products are listed", async ({
  page,
}) => {
  await open(page, newHub());
  expect(
    await page.getByLabel(en["hub.stock.product"]).locator("option").allTextContents(),
  ).toEqual([en["hub.stock.product_pick"], "Cola"]);
  expect(await page.getByLabel(en["hub.waste.reason"]).locator("option").allTextContents()).toEqual(
    [
      en["hub.waste.reason_pick"],
      en["reason.spoiled"],
      en["reason.broken"],
      en["reason.free"],
      en["reason.cancelled_order"],
    ],
  );
  await expectNoSeriousAxe(page);
});

test("a manager records waste with its reason and the list shows the new quantity", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub);
  await expect(page.getByTestId("waste-row")).toContainText("10 on hand");
  await record(page, "3", en["reason.broken"]);
  await expect(page.getByTestId("waste-status")).toContainText(en["hub.waste.recorded"]);
  expect(hub.intents).toEqual([
    {
      payload: { kind: "stock.waste", product_id: "P1".padEnd(26, "0"), qty: 3 },
      reason: "broken",
    },
  ]);
  expect(hub.elevations).toEqual([]);
  await expect(page.getByTestId("waste-row")).toContainText("7 on hand");
  await expect(page.getByLabel(en["hub.stock.qty"])).toHaveValue("");
  await expectNoSeriousAxe(page);
});

test("a Counter's waste asks for a manager's PIN in place and one PIN covers the undo", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub, "counter-token");
  await record(page, "2", en["reason.cancelled_order"]);
  await expect(page.getByTestId("waste-approval")).toBeVisible();
  expect(hub.intents).toEqual([]);
  await page.keyboard.type("2222");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("waste-status")).toContainText(en["hub.waste.recorded"]);
  expect(hub.elevations.at(-1)).toEqual({
    items: [{ intent_kind: "stock.waste" }, { intent_kind: "stock.waste.reverted" }],
    pin: "2222",
  });
  expect(hub.intents[0]).toEqual({
    payload: { kind: "stock.waste", product_id: "P1".padEnd(26, "0"), qty: 2 },
    reason: "cancelled_order",
    ticket: "T1",
  });
  await page.getByTestId("waste-undo").click();
  await expect(page.getByTestId("waste-status")).toContainText(en["hub.waste.undone"]);
  expect(hub.intents[1]).toEqual({
    payload: { kind: "stock.waste.reverted", move_id: "W1" },
    ticket: "T1",
  });
  await expect(page.getByTestId("waste-row")).toContainText("10 on hand");
});

test("a wrong PIN sends nothing", async ({ page }) => {
  const hub = newHub();
  await open(page, hub, "counter-token");
  await record(page, "2");
  await page.keyboard.type("1111");
  await page.keyboard.press("Enter");
  await expect(page.getByText(en["ui.pin.rejected"])).toBeVisible();
  expect(hub.intents).toEqual([]);
});

test("more than the stock on hand is refused on the quantity", async ({ page }) => {
  const hub = newHub();
  hub.fail = { status: 409, code: "OUT_OF_STOCK", field: "qty" };
  await open(page, hub);
  await record(page, "50");
  await expect(page.getByText(en["hub.waste.out_of_stock"])).toBeVisible();
  await expect(page.getByTestId("waste-undo")).toBeHidden();
  await expectNoSeriousAxe(page);
});

test("bad values are refused before anything is sent", async ({ page }) => {
  const hub = newHub();
  await open(page, hub);
  await page.getByRole("button", { name: en["hub.waste.submit"] }).click();
  await expect(page.getByText(en["hub.waste.invalid.product"])).toBeVisible();
  await expect(page.getByText(en["hub.waste.invalid.qty"])).toBeVisible();
  await expect(page.getByText(en["hub.waste.invalid.reason"])).toBeVisible();
  expect(hub.intents).toEqual([]);
  await expectNoSeriousAxe(page);
});

test("a late undo says so and the undo button goes away after the window", async ({ page }) => {
  const hub = newHub();
  await open(page, hub);
  await record(page, "1", en["reason.spoiled"]);
  hub.fail = { status: 409, code: "EXPIRED" };
  await page.getByTestId("waste-undo").click();
  await expect(page.getByTestId("waste-status")).toContainText(en["hub.waste.undo_expired"]);
  hub.fail = null;
  await record(page, "1", en["reason.spoiled"]);
  await expect(page.getByTestId("waste-undo")).toBeVisible();
  await expect(page.getByTestId("waste-undo")).toBeHidden({ timeout: 12000 });
});
