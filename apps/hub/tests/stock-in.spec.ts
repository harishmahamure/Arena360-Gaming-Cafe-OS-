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
const COUNTER: SessionStaff = {
  staff_id: "01JCOUNTER0000000000000000",
  display_name: "Counter",
  role_name: "Counter",
  permissions: ["stock.in"],
};

type Payload = Record<string, unknown> & { kind: string };
type Hub = {
  products: Product[];
  intents: Payload[];
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
  quantity: 0,
  avg_cost_minor: 0,
  available: false,
  reason: "out_of_stock",
  ...over,
});

const newHub = (): Hub => ({
  products: [
    product({}),
    product({ id: "P2".padEnd(26, "0"), name: "Water", track_stock: false }),
    product({ id: "P3".padEnd(26, "0"), name: "Combo", bundle: true, track_stock: false }),
  ],
  intents: [],
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
  if (p.kind === "stock.in" && row) {
    const qty = p.qty as number;
    const cost = p.unit_cost_minor as number;
    const total = row.quantity + qty;
    row.avg_cost_minor = Math.floor(
      (2 * (row.quantity * row.avg_cost_minor + qty * cost) + total) / (2 * total),
    );
    row.quantity = total;
    row.available = true;
    row.reason = undefined;
    return json(
      route,
      200,
      applied([
        { tbl: "stock", row_id: row.id },
        { tbl: "stock_moves", row_id: "M1" },
      ]),
    );
  }
  hub.products.forEach((x) => Object.assign(x, { quantity: 0, avg_cost_minor: 0 }));
  return json(route, 200, applied([{ tbl: "stock_moves", row_id: "M2" }]));
}

async function open(page: Page, hub: Hub) {
  await page.addInitScript(() => sessionStorage.setItem("arena.hub.token", "counter-token"));
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot":
        return json(route, 200, snapshot());
      case "/ui/me":
        return json(route, 200, COUNTER);
      case "/ui/products":
        return json(route, 200, hub.products);
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: `retry: 300\n\n`,
        });
      case "/ui/intent": {
        const { intent } = req.postDataJSON() as { intent: { payload: Payload } };
        hub.intents.push(intent.payload);
        if (hub.fail) return json(route, hub.fail.status, { message: "no", ...hub.fail });
        return apply(hub, intent.payload, route);
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/inventory/stock-in");
  await expect(page.getByTestId("stock-form")).toBeVisible();
}

async function expectNoSeriousAxe(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  expect(bad.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual(
    [],
  );
}

const record = async (page: Page, qty: string, cost: string, expiry = "") => {
  await page.getByLabel(en["hub.stock.product"]).selectOption({ label: "Cola" });
  await page.getByLabel(en["hub.stock.qty"]).fill(qty);
  await page.getByLabel(en["hub.stock.cost"]).fill(cost);
  await page.getByLabel(en["hub.stock.supplier"]).fill("Acme");
  if (expiry) await page.getByLabel(en["hub.stock.expiry"]).fill(expiry);
  await page.getByRole("button", { name: en["hub.stock.submit"] }).click();
};

test("only tracked non-bundle products can receive stock", async ({ page }) => {
  await open(page, newHub());
  const options = await page
    .getByLabel(en["hub.stock.product"])
    .locator("option")
    .allTextContents();
  expect(options).toEqual([en["hub.stock.product_pick"], "Cola"]);
  await expectNoSeriousAxe(page);
});

test("recording stock-in sends the payload and the list shows the weighted average", async ({
  page,
}) => {
  const hub = newHub();
  hub.products[0] = product({
    quantity: 10,
    avg_cost_minor: 2000,
    available: true,
    reason: undefined,
  });
  await open(page, hub);
  await expect(page.getByTestId("stock-row")).toContainText("10 on hand · average cost ₹20.00");
  await record(page, "30", "24", "2099-12-31");
  await expect(page.getByTestId("stock-status")).toContainText(en["hub.stock.recorded"]);
  expect(hub.intents).toEqual([
    {
      kind: "stock.in",
      product_id: "P1".padEnd(26, "0"),
      qty: 30,
      unit_cost_minor: 2400,
      supplier: "Acme",
      expiry: "2099-12-31",
    },
  ]);
  await expect(page.getByTestId("stock-row")).toContainText("40 on hand · average cost ₹23.00");
  await expect(page.getByLabel(en["hub.stock.qty"])).toHaveValue("");
  await expectNoSeriousAxe(page);
});

test("undo within 8 seconds sends stock.in.reverted for the move and then disappears", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub);
  await record(page, "5", "10");
  const undo = page.getByTestId("stock-undo");
  await expect(undo).toBeVisible();
  await expect(undo).toContainText("Undo (");
  await undo.click();
  await expect(page.getByTestId("stock-status")).toContainText(en["hub.stock.undone"]);
  expect(hub.intents[1]).toEqual({ kind: "stock.in.reverted", move_id: "M1" });
  await expect(undo).toBeHidden();
  await expect(page.getByTestId("stock-row")).toContainText("0 on hand");
});

test("the undo button goes away after the window", async ({ page }) => {
  await open(page, newHub());
  await record(page, "5", "10");
  await expect(page.getByTestId("stock-undo")).toBeVisible();
  await expect(page.getByTestId("stock-undo")).toBeHidden({ timeout: 12000 });
});

test("bad values are refused before anything is sent", async ({ page }) => {
  const hub = newHub();
  await open(page, hub);
  await page.getByRole("button", { name: en["hub.stock.submit"] }).click();
  await expect(page.getByText(en["hub.stock.invalid.product"])).toBeVisible();
  await expect(page.getByText(en["hub.stock.invalid.qty"])).toBeVisible();
  await expect(page.getByText(en["hub.stock.invalid.cost"])).toBeVisible();
  await page.getByLabel(en["hub.stock.product"]).selectOption({ label: "Cola" });
  await page.getByLabel(en["hub.stock.qty"]).fill("2.5");
  await page.getByLabel(en["hub.stock.cost"]).fill("-3");
  await page.getByRole("button", { name: en["hub.stock.submit"] }).click();
  await expect(page.getByText(en["hub.stock.invalid.qty"])).toBeVisible();
  await expect(page.getByText(en["hub.stock.invalid.cost"])).toBeVisible();
  expect(hub.intents).toEqual([]);
  await expectNoSeriousAxe(page);
});

test("a refused expiry shows on its field and nothing is recorded", async ({ page }) => {
  const hub = newHub();
  hub.fail = { status: 422, code: "VALIDATION_FAILED", field: "expiry" };
  await open(page, hub);
  await record(page, "5", "10", "2000-01-01");
  await expect(page.getByText(en["hub.stock.invalid.expiry"])).toBeVisible();
  await expect(page.getByTestId("stock-undo")).toBeHidden();
  await expectNoSeriousAxe(page);
});

test("a late undo says so", async ({ page }) => {
  const hub = newHub();
  await open(page, hub);
  await record(page, "5", "10");
  hub.fail = { status: 409, code: "EXPIRED" };
  await page.getByTestId("stock-undo").click();
  await expect(page.getByTestId("stock-status")).toContainText(en["hub.stock.undo_expired"]);
});
