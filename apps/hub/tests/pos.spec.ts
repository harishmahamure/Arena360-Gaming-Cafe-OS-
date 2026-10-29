import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Locator, type Page, type Route, type TestInfo } from "@playwright/test";
import type {
  KitchenTicketView,
  PaymentRow,
  PlayerHit,
  PosSaleView,
  PrinterReady,
  Product,
  ShiftState,
  SnapshotBill,
  SnapshotSession,
  WalletView,
} from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import type { Tax } from "../src/lib/tax";
import { CORS, HUB, device, json as ok, snapshot } from "./floor-fixture";

// The POS screen (Story 5.9): product grid with a category strip and favourites, search or scan,
// the cart with its steppers and live availability check, and the phone cart bar and sheet.
// The Hub's loopback API is stubbed with `page.route`.

const product = (id: string, name: string, extra: Partial<Product> = {}): Product => ({
  id,
  name,
  category: "Drinks",
  price_minor: 5000,
  tax_rate_bp: 0,
  hsn: "",
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
  avg_cost_minor: 0,
  available: true,
  ...extra,
});

const PRODUCTS: Product[] = [
  product("P1", "Cola", { favourite: true, quantity: 3, barcode: "8901", price_minor: 6000 }),
  product("P2", "Chips", { category: "Snacks", quantity: 20, barcode: "8902", price_minor: 3000 }),
  product("P3", "Water", {
    quantity: 0,
    available: false,
    reason: "out_of_stock",
    barcode: "8903",
  }),
  product("P4", "Milk", { quantity: 0, available: false, reason: "expired" }),
  product("P5", "Old Tea", { active: false, available: false, reason: "inactive" }),
  product("P6", "Combo", {
    category: "Combos",
    bundle: true,
    track_stock: false,
    quantity: 3,
    price_minor: 10000,
    components: [
      { product_id: "P1", name: "Cola", qty: 1, active: true },
      { product_id: "P2", name: "Chips", qty: 2, active: true },
    ],
  }),
  product("P7", "Gum", {
    category: "Snacks",
    track_stock: false,
    quantity: 0,
    barcode: "8907",
    price_minor: 1000,
  }),
];

const PLAYERS: PlayerHit[] = [
  {
    player_id: "PL1",
    name: "Asha",
    username: "asha",
    phone: "9876543210",
    tags: [],
    outstanding_minor: 0,
  },
];

const isPhone = (info: TestInfo) => info.project.name === "phone";

async function open(page: Page, products: Product[] = PRODUCTS, fail = false) {
  await page.addInitScript(() => sessionStorage.setItem("arena.hub.token", "manager-token"));
  await page.route(`${HUB}/**`, async (route: Route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot":
        return ok(route, snapshot([device(1, {})]));
      case "/ui/me":
        return ok(route, {
          staff_id: "01JMANAGER0000000000000000",
          display_name: "Manager",
          role_name: "Manager",
          permissions: [],
          locked: false,
          has_pin: true,
        });
      case "/ui/products":
        return fail ? route.fulfill({ status: 500, headers: CORS, body: "" }) : ok(route, products);
      case "/ui/players": {
        const digits = (new URL(req.url()).searchParams.get("q") ?? "").replace(/\D/g, "");
        return ok(route, digits ? PLAYERS.filter((p) => p.phone.includes(digits)) : []);
      }
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: `retry: 300\n\n`,
        });
      default:
        return route.fulfill({ status: 404, headers: CORS, body: "{}" });
    }
  });
  await page.goto("/pos");
  await expect(page.getByTestId("pos")).toBeVisible();
}

const tile = (page: Page, name: string): Locator =>
  page.getByTestId("pos-tile").filter({ hasText: name });
const lineOf = (page: Page, name: string): Locator =>
  page.getByTestId("cart-line").filter({ hasText: name });

/** The cart column on a desktop window; on a phone the cart opens as a sheet first. */
async function openCart(page: Page, info: TestInfo) {
  if (isPhone(info)) await page.getByTestId("phone-cart-bar").click();
}

test("opens with favourites first, the grid on the left, the cart on the right and no total", async ({
  page,
}, info) => {
  await open(page);
  await expect(page.locator("main h1")).toHaveText(en["hub.nav.pos"]);
  const strip = page.getByTestId("pos-categories").getByRole("button");
  await expect(strip).toHaveText([
    en["hub.pos.favourites"],
    en["hub.pos.all"],
    "Combos",
    "Drinks",
    "Snacks",
  ]);
  await expect(strip.first()).toHaveAttribute("aria-pressed", "true");
  await expect(page.getByTestId("pos-tile")).toHaveText([/Cola/]);
  await expect(page.getByLabel(en["hub.pos.search.label"])).toBeVisible();

  if (isPhone(info)) return; // an empty cart has no phone bar, so no cart on screen
  const grid = await page.getByTestId("pos-product-grid").boundingBox();
  const cart = await page.getByTestId("cart").boundingBox();
  expect(cart!.x - (grid!.x + grid!.width)).toBeGreaterThanOrEqual(39);
  await expect(page.getByTestId("cart-empty")).toHaveText(en["hub.pos.cart.empty"]);
  await expect(page.getByTestId("cart-for")).toHaveText(en["hub.pos.cart.walkup"]);
  await expect(page.getByTestId("settle-bar")).toBeVisible();
  await expect(page.getByTestId("settle-total")).toHaveCount(0);
});

test("a product that is inactive, out of stock or expired is disabled with a readable tag", async ({
  page,
}) => {
  await open(page);
  await page.getByRole("button", { name: en["hub.pos.all"], exact: true }).click();
  for (const [name, tag] of [
    ["Water", en["hub.pos.tag.out_of_stock"]],
    ["Milk", en["hub.pos.tag.expired"]],
    ["Old Tea", en["hub.pos.tag.inactive"]],
  ] as const) {
    await expect(tile(page, name)).toBeDisabled();
    await expect(tile(page, name).getByTestId("pos-tag")).toHaveText(tag);
  }
  await expect(tile(page, "Chips")).toBeEnabled();
  await expect(tile(page, "Gum")).toBeEnabled();
});

test("tapping a tile adds it, again adds one more, and the total follows", async ({
  page,
}, info) => {
  await open(page);
  await tile(page, "Cola").click();
  await tile(page, "Cola").click();
  await openCart(page, info);
  await expect(lineOf(page, "Cola").getByTestId("line-qty")).toHaveText("2");
  await expect(page.getByTestId("settle-total")).toContainText("120");
  await expect(page.getByTestId("cart-empty")).toHaveCount(0);
});

test("a category narrows the grid; a search looks through every category, case-insensitively", async ({
  page,
}) => {
  await open(page);
  await page.getByRole("button", { name: "Snacks", exact: true }).click();
  await expect(page.getByTestId("pos-tile")).toHaveText([/Chips/, /Gum/]);
  await page.getByLabel(en["hub.pos.search.label"]).fill("CO");
  await expect(page.getByTestId("pos-tile")).toHaveText([/Cola/, /Combo/]);
  await page.getByLabel(en["hub.pos.search.label"]).fill("zzz");
  await expect(page.getByTestId("pos-none")).toHaveText(
    en["hub.pos.no_match"].replace("{query}", "zzz"),
  );
});

test("Ctrl+K focuses the search field unless focus is already in a text field", async ({
  page,
}) => {
  await open(page);
  await page.locator("body").focus();
  await page.keyboard.press("Control+k");
  await expect(page.getByLabel(en["hub.pos.search.label"])).toBeFocused();
  await expect(page).toHaveURL(/\/pos$/);
});

test("Enter in the search field adds the product with that barcode and clears the field", async ({
  page,
}, info) => {
  await open(page);
  const search = page.getByLabel(en["hub.pos.search.label"]);
  await search.fill("8902");
  await search.press("Enter");
  await expect(search).toHaveValue("");
  await expect(page.getByTestId("pos-notice")).toContainText("Chips");
  await search.fill("8902");
  await search.press("Enter");
  await openCart(page, info);
  await expect(lineOf(page, "Chips").getByTestId("line-qty")).toHaveText("2");
});

test("an unknown code says so and adds nothing; a barcode of an unavailable product refuses", async ({
  page,
}) => {
  await open(page);
  const search = page.getByLabel(en["hub.pos.search.label"]);
  await search.fill("0000");
  await search.press("Enter");
  await expect(page.getByTestId("pos-notice")).toHaveText(
    "⚠ " + en["hub.pos.barcode_unknown"].replace("{code}", "0000"),
  );
  await expect(page.getByTestId("phone-cart-bar")).toHaveCount(0);
  // A code that added nothing stays in the field so it can be corrected.
  await expect(search).toHaveValue("0000");
  await search.fill("8903");
  await search.press("Enter");
  await expect(page.getByTestId("pos-notice")).toContainText(
    en["hub.pos.tag.out_of_stock"].toLowerCase(),
  );
  await expect(page.getByTestId("phone-cart-bar")).toHaveCount(0);
});

test("a scanner burst outside a text field adds the product; slow typing does not", async ({
  page,
}, info) => {
  await open(page);
  await page.locator("body").focus();
  await page.keyboard.type("8907", { delay: 120 });
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("pos-notice")).toHaveText("");
  await page.keyboard.type("8907", { delay: 5 });
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("pos-notice")).toContainText("Gum");
  await openCart(page, info);
  await expect(lineOf(page, "Gum").getByTestId("line-qty")).toHaveText("1");
});

test("the steppers refuse a step past what is on hand, with the reason under the line", async ({
  page,
}, info) => {
  await open(page);
  await tile(page, "Cola").click();
  await openCart(page, info);
  const cola = lineOf(page, "Cola");
  await cola.getByTestId("line-inc").click();
  await cola.getByTestId("line-inc").click();
  await expect(cola.getByTestId("line-qty")).toHaveText("3");
  await cola.getByTestId("line-inc").click();
  await expect(cola.getByTestId("line-qty")).toHaveText("3");
  await expect(cola.getByTestId("line-refusal")).toContainText(
    en["hub.pos.cart.limit"].replace("{left}", "3").replace("{name}", "Cola"),
  );
  await cola.getByTestId("line-dec").click();
  await expect(cola.getByTestId("line-qty")).toHaveText("2");
  await expect(cola.getByTestId("line-refusal")).toHaveCount(0);
  await cola.getByTestId("line-dec").click();
  await cola.getByTestId("line-dec").click();
  // Emptying the cart on a phone closes its sheet; there is then no bar either.
  if (isPhone(info)) await expect(page.getByTestId("phone-cart-bar")).toHaveCount(0);
  else await expect(page.getByTestId("cart-empty")).toBeVisible();
});

test("a bundle shows its components under it and draws on their stock", async ({ page }, info) => {
  await open(page);
  await page.getByRole("button", { name: "Combos", exact: true }).click();
  await tile(page, "Combo").click();
  await openCart(page, info);
  const combo = lineOf(page, "Combo");
  await expect(combo.getByTestId("line-parts").getByRole("listitem")).toHaveText([
    "1 × Cola",
    "2 × Chips",
  ]);
  await combo.getByTestId("line-inc").click();
  await expect(combo.getByTestId("line-parts").getByRole("listitem")).toHaveText([
    "2 × Cola",
    "4 × Chips",
  ]);
  await combo.getByTestId("line-inc").click();
  await combo.getByTestId("line-inc").click();
  await expect(combo.getByTestId("line-qty")).toHaveText("3");
  await expect(combo.getByTestId("line-refusal")).toBeVisible();
});

test("the cart header says Walk-up, and Change attaches a player or goes back to a guest", async ({
  page,
}, info) => {
  await open(page);
  await page.getByRole("button", { name: en["hub.pos.all"], exact: true }).click();
  await tile(page, "Chips").click();
  await openCart(page, info);
  await expect(page.getByTestId("cart-for")).toHaveText(en["hub.pos.cart.walkup"]);
  await page.getByTestId("cart-change").click();
  const chooser = page.getByTestId("cart-chooser");
  await chooser.locator("[data-player-lookup]").fill("9876");
  await chooser.getByTestId("player-hits").getByRole("button", { name: /Asha/ }).click();
  await expect(page.getByTestId("cart-for")).toHaveText("Asha");
  await expect(chooser).toHaveCount(0);
  await page.getByTestId("cart-change").click();
  await page
    .getByTestId("cart-chooser")
    .getByRole("button", { name: en["hub.players.lookup.guest"] })
    .click();
  await expect(page.getByTestId("cart-for")).toHaveText(en["hub.pos.cart.walkup"]);
});

test("below 840px a bar shows the count and total; it opens the cart as a sheet that Esc closes", async ({
  page,
}, info) => {
  test.skip(!isPhone(info), "the phone cart bar is below 840px only");
  await open(page);
  await expect(page.getByTestId("phone-cart-bar")).toHaveCount(0);
  await tile(page, "Cola").click();
  const bar = page.getByTestId("phone-cart-bar");
  await expect(bar).toContainText("1 item");
  await expect(bar).toContainText("View");
  await tile(page, "Cola").click();
  await expect(bar).toContainText("2 items");
  await bar.click();
  const sheet = page.getByRole("dialog", { name: en["hub.pos.sheet.title"] });
  await expect(sheet).toBeVisible();
  await expect(sheet.getByTestId("cart")).toBeVisible();
  await expect(sheet.getByTestId("settle-bar")).toBeVisible();
  await expect(page.getByTestId("pos-tile")).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(sheet).toHaveCount(0);
  await expect(bar).toBeFocused();
  await expect(bar).toContainText("2 items");
});

test("the screen has no serious accessibility violations", async ({ page }, info) => {
  await open(page);
  await tile(page, "Cola").click();
  const results = await new AxeBuilder({ page }).analyze();
  expect(
    results.violations.filter((v) => v.impact === "serious" || v.impact === "critical"),
  ).toEqual([]);
  if (isPhone(info)) await page.getByTestId("phone-cart-bar").click();
  const sheet = await new AxeBuilder({ page }).analyze();
  expect(sheet.violations.filter((v) => v.impact === "serious" || v.impact === "critical")).toEqual(
    [],
  );
});

test("a product list that cannot be loaded offers Retry", async ({ page }) => {
  await open(page, [], true);
  await expect(page.getByRole("alert")).toContainText(en["hub.pos.load_failed"]);
  await expect(page.getByRole("button", { name: en["hub.pos.retry"] })).toBeVisible();
});

// ---- Story 5.11: sell at the counter ----

type Sent = { payload: Record<string, unknown> & { kind: string }; rev?: number; ticket?: string };
type Hub = {
  sent: Sent[];
  elevations: { items: unknown; pin: string }[];
  permissions: string[];
  shift: ShiftState;
  tax: Partial<Tax>;
  sales: PosSaleView[] | "fail";
  /** Refusals the next intents answer with, oldest first. */
  refuse: { status: number; code: string; message?: string }[];
  wallet: WalletView;
  /** The running sessions the snapshot lists (Story 5.12). */
  sessions?: SnapshotSession[];
  /** The open bills the snapshot lists, e.g. unpaid kiosk orders (Story 6.32). */
  bills?: SnapshotBill[];
  /** The open walk-up kitchen tickets the snapshot lists (Story 7.4). */
  counterTickets?: KitchenTicketView[];
  /** What `/ui/printer/ready` answers (Story 5.17); absent means the Hub does not answer. */
  printer?: PrinterReady;
};

const SHIFT: ShiftState = {
  open: {
    id: "SH1",
    staff_id: "01JMANAGER0000000000000000",
    staff_name: "Ravi",
    opened_at: Date.UTC(2026, 9, 28, 9, 30),
    float_minor: 5000,
    events: 1,
  },
  carried_minor: 5000,
};

const newHub = (extra: Partial<Hub> = {}): Hub => ({
  sent: [],
  elevations: [],
  permissions: ["pos.sell"],
  shift: SHIFT,
  tax: {},
  sales: [],
  refuse: [],
  wallet: { player_id: "PL1", balance_minor: 20000, txns: [] },
  ...extra,
});

const TAX: Tax = {
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

async function openHub(page: Page, hub: Hub, products: Product[] = PRODUCTS, path = "/pos") {
  await page.addInitScript(() => sessionStorage.setItem("arena.hub.token", "counter-token"));
  await page.route(`${HUB}/**`, async (route: Route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot": {
        const snap = snapshot(hub.sessions ? [1, 2, 3].map((n) => device(n, {})) : [device(1, {})]);
        snap.shift = hub.shift;
        snap.sessions = hub.sessions ?? [];
        snap.bills = hub.bills ?? [];
        snap.counter_tickets = hub.counterTickets ?? [];
        return ok(route, snap);
      }
      case "/ui/me":
        return ok(route, {
          staff_id: "01JCOUNTER0000000000000000",
          display_name: "Asha",
          role_name: "Counter",
          permissions: hub.permissions,
          locked: false,
          has_pin: true,
        });
      case "/ui/products":
        return ok(route, products);
      case "/ui/settings":
        return ok(route, [
          { key: "branch.tax", value: JSON.stringify({ ...TAX, ...hub.tax }), rev: 1 },
        ]);
      case "/ui/printer/ready":
        return hub.printer
          ? ok(route, hub.printer)
          : route.fulfill({ status: 500, headers: CORS, body: "" });
      case "/ui/pos/sales":
        return hub.sales === "fail"
          ? route.fulfill({ status: 500, headers: CORS, body: "" })
          : ok(route, { business_day: "2026-10-28", sales: hub.sales });
      case "/ui/players": {
        const digits = (new URL(req.url()).searchParams.get("q") ?? "").replace(/\D/g, "");
        return ok(route, digits ? PLAYERS.filter((p) => p.phone.includes(digits)) : []);
      }
      case "/ui/players/wallet":
        return ok(route, hub.wallet);
      case "/ui/elevate": {
        const body = req.postDataJSON() as { items: unknown; pin: string };
        hub.elevations.push(body);
        if (body.pin !== "2222") {
          return route.fulfill({
            status: 403,
            contentType: "application/json",
            headers: CORS,
            body: JSON.stringify({ code: "PIN_REJECTED", message: "no" }),
          });
        }
        return ok(route, { ticket: "T1", items: body.items, expires_in_s: 60 });
      }
      case "/ui/intent": {
        const body = req.postDataJSON() as {
          intent: { payload: Sent["payload"]; base_rev?: number };
          elevation_ticket?: string;
        };
        hub.sent.push({
          payload: body.intent.payload,
          rev: body.intent.base_rev,
          ticket: body.elevation_ticket,
        });
        const refusal = hub.refuse.shift();
        if (refusal) {
          return route.fulfill({
            status: refusal.status,
            contentType: "application/json",
            headers: CORS,
            body: JSON.stringify({ message: "refused", ...refusal }),
          });
        }
        const payments = (body.intent.payload.payments ?? []) as PaymentRow[];
        const now = Date.now();
        return ok(route, {
          applied: { intent_id: "I", changes: [] },
          settled:
            body.intent.payload.kind === "pos.sale"
              ? {
                  order_id: "O1",
                  settled_at: now,
                  undo_until: now + 8000,
                  order_rev: 1,
                  total_minor: payments.reduce((n, p) => n + p.amount_minor, 0),
                  irreversible: body.intent.payload.receipt === "print",
                }
              : undefined,
          placed:
            body.intent.payload.kind === "order.place"
              ? {
                  order_id: "O9",
                  order_rev: 2,
                  placement_id: "PL9",
                  placed_at: now,
                  undo_until: now + 8000,
                  total_minor: 12000,
                }
              : undefined,
        });
      }
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: `retry: 300\n\n`,
        });
      default:
        return route.fulfill({ status: 404, headers: CORS, body: "{}" });
    }
  });
  await page.goto(path);
  await expect(page.getByTestId("pos")).toBeVisible();
}

const cashInput = (page: Page): Locator => page.locator('[data-row="cash"] input');

test("a walk-up sale: the cash row is prefilled and Ctrl+Enter settles it", async ({
  page,
}, info) => {
  const hub = newHub();
  await openHub(page, hub);
  await tile(page, "Cola").click();
  await tile(page, "Cola").click();
  await openCart(page, info);
  await expect(page.getByTestId("settle-total")).toContainText("120.00");
  await expect(cashInput(page)).toHaveValue("120.00");
  await page.keyboard.press("Control+Enter");
  await expect(page.getByTestId("undo-toast")).toBeVisible();
  await expect(page.getByTestId("undo-message")).toContainText("120.00");
  expect(hub.sent).toHaveLength(1);
  expect(hub.sent[0].payload).toMatchObject({
    kind: "pos.sale",
    lines: [{ product_id: "P1", qty: 2 }],
    payments: [{ method: "cash", amount_minor: 12000 }],
  });
  expect(hub.sent[0].payload.player_id).toBeUndefined();
  expect(hub.sent[0].payload.stock_override).toBeUndefined();
  expect(hub.sent[0].rev).toBeUndefined();
  // The cart is empty again.
  if (isPhone(info)) await expect(page.getByTestId("phone-cart-bar")).toHaveCount(0);
  else await expect(page.getByTestId("cart-empty")).toBeVisible();
});

test("a mixed sale sends one cash row and one UPI row with its reference", async ({
  page,
}, info) => {
  const hub = newHub();
  await openHub(page, hub);
  await page.getByRole("button", { name: en["hub.pos.all"], exact: true }).click();
  await tile(page, "Cola").click();
  await tile(page, "Chips").click();
  await openCart(page, info);
  await expect(cashInput(page)).toHaveValue("90.00");
  await cashInput(page).fill("50");
  await page.locator('[data-method="upi"]').click();
  await expect(page.locator('[data-row="upi"] input').first()).toHaveValue("40.00");
  await page.locator('[data-row="upi"] [data-reference] input').fill("1234");
  await page.getByRole("button", { name: en["hub.settle.submit"], exact: true }).click();
  await expect(page.getByTestId("undo-toast")).toBeVisible();
  expect(hub.sent[0].payload.payments).toEqual([
    { method: "cash", amount_minor: 5000 },
    { method: "upi", amount_minor: 4000, reference: "1234" },
  ]);
});

test("with no shift open there is no Cash row; UPI settles the sale", async ({ page }, info) => {
  const hub = newHub({ shift: { open: null, carried_minor: 0 } });
  await openHub(page, hub);
  await tile(page, "Cola").click();
  await openCart(page, info);
  await expect(page.getByTestId("settle-shift-closed")).toBeVisible();
  await expect(page.locator('[data-method="cash"]')).toHaveCount(0);
  await expect(page.locator('[data-row="cash"]')).toHaveCount(0);
  await page.locator('[data-method="upi"]').click();
  await page.locator('[data-row="upi"] [data-reference] input').fill("4321");
  await page.keyboard.press("Control+Enter");
  await expect(page.getByTestId("undo-toast")).toBeVisible();
  expect(hub.sent[0].payload.payments).toEqual([
    { method: "upi", amount_minor: 6000, reference: "4321" },
  ]);
});

test("an exclusive branch tax is added to the total and shown per rate", async ({ page }, info) => {
  const hub = newHub({ tax: { inclusive: false, rate_bp: 500 } });
  await openHub(page, hub);
  await tile(page, "Cola").click();
  await openCart(page, info);
  await expect(page.getByTestId("pos-subtotal")).toContainText("60.00");
  await expect(page.getByTestId("pos-tax-rate")).toContainText("Tax 5%");
  await expect(page.getByTestId("pos-tax-rate")).toContainText("3.00");
  await expect(page.getByTestId("settle-total")).toContainText("63.00");
  await expect(cashInput(page)).toHaveValue("63.00");
  await page.keyboard.press("Control+Enter");
  await expect(page.getByTestId("undo-toast")).toBeVisible();
  expect(hub.sent[0].payload.payments).toEqual([{ method: "cash", amount_minor: 6300 }]);
});

test("wallet and credit appear once a player is attached, and the sale names them", async ({
  page,
}, info) => {
  const hub = newHub({
    wallet: {
      player_id: "PL1",
      balance_minor: 20000,
      txns: [],
      credit: {
        limit_minor: 50000,
        outstanding_minor: 0,
        paused: false,
        reminder_days: [3, 7, 14],
      } as WalletView["credit"],
    },
  });
  await openHub(page, hub);
  await tile(page, "Cola").click();
  await openCart(page, info);
  await expect(page.locator('[data-method="wallet"]')).toHaveCount(0);
  await expect(page.locator('[data-method="credit"]')).toHaveCount(0);
  await page.getByTestId("cart-change").click();
  const chooser = page.getByTestId("cart-chooser");
  await chooser.locator("[data-player-lookup]").fill("9876");
  await chooser.getByTestId("player-hits").getByRole("button", { name: /Asha/ }).click();
  await expect(page.locator('[data-method="wallet"]')).toHaveCount(1);
  await expect(page.locator('[data-method="credit"]')).toHaveCount(1);
  await cashInput(page).fill("10");
  await page.locator('[data-method="wallet"]').click();
  await expect(page.locator('[data-row="wallet"] input').first()).toHaveValue("50.00");
  await page.keyboard.press("Control+Enter");
  await expect(page.getByTestId("undo-toast")).toBeVisible();
  expect(hub.sent[0].payload).toMatchObject({
    player_id: "PL1",
    payments: [
      { method: "cash", amount_minor: 1000 },
      { method: "wallet", amount_minor: 5000 },
    ],
  });
});

test("a refused short bundle offers a manager PIN and sells with the override", async ({
  page,
}, info) => {
  const hub = newHub({
    refuse: [{ status: 409, code: "BUNDLE_SHORT", message: "Not enough Chips for Combo." }],
  });
  await openHub(page, hub);
  await page.getByRole("button", { name: "Combos", exact: true }).click();
  await tile(page, "Combo").click();
  await openCart(page, info);
  await page.keyboard.press("Control+Enter");
  const panel = page.getByTestId("pos-override");
  await expect(panel).toBeVisible();
  await expect(panel.getByTestId("pos-short-message")).toHaveText("Not enough Chips for Combo.");
  await expect(panel.locator("[data-component=inline-pin]")).toBeVisible();
  expect(hub.sent).toHaveLength(1);
  expect(hub.sent[0].payload.stock_override).toBeUndefined();
  // A wrong PIN is refused and nothing is sent.
  await page.keyboard.type("1111");
  await page.keyboard.press("Enter");
  await expect(panel).toContainText(en["ui.pin.rejected"]);
  expect(hub.sent).toHaveLength(1);
  await page.keyboard.type("2222");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("undo-toast")).toBeVisible();
  expect(hub.elevations.at(-1)).toMatchObject({
    items: [{ intent_kind: "pos.sale" }],
    pin: "2222",
  });
  expect(hub.sent).toHaveLength(2);
  expect(hub.sent[1].payload).toMatchObject({
    kind: "pos.sale",
    stock_override: true,
    lines: [{ product_id: "P6", qty: 1 }],
  });
  expect(hub.sent[1].ticket).toBe("T1");
  await expect(panel).toHaveCount(0);
});

test("staff who hold stock.override sell a short bundle anyway without a PIN", async ({
  page,
}, info) => {
  const hub = newHub({
    permissions: ["pos.sell", "stock.override"],
    refuse: [{ status: 409, code: "BUNDLE_SHORT", message: "Not enough Chips for Combo." }],
  });
  await openHub(page, hub);
  await page.getByRole("button", { name: "Combos", exact: true }).click();
  await tile(page, "Combo").click();
  await openCart(page, info);
  await page.keyboard.press("Control+Enter");
  const panel = page.getByTestId("pos-override");
  await expect(panel.locator("[data-component=inline-pin]")).toHaveCount(0);
  await panel.getByRole("button", { name: en["hub.pos.override.sell"] }).click();
  await expect(page.getByTestId("undo-toast")).toBeVisible();
  expect(hub.elevations).toHaveLength(0);
  expect(hub.sent[1].payload.stock_override).toBe(true);
  expect(hub.sent[1].ticket).toBeUndefined();
});

test("a plain refusal says why and keeps the cart", async ({ page }, info) => {
  const hub = newHub({ refuse: [{ status: 409, code: "OUT_OF_STOCK", message: "only 1 left" }] });
  await openHub(page, hub);
  await tile(page, "Cola").click();
  await openCart(page, info);
  await page.keyboard.press("Control+Enter");
  await expect(page.getByTestId("pos-error")).toHaveText(
    new RegExp(en["hub.pos.err.out_of_stock"]),
  );
  await expect(page.getByTestId("undo-toast")).toHaveCount(0);
  await expect(lineOf(page, "Cola")).toBeVisible();
});

test("Undo sends pos.sale.reverted with the order rev and puts the cart and player back", async ({
  page,
}, info) => {
  const hub = newHub();
  await openHub(page, hub);
  await tile(page, "Cola").click();
  await tile(page, "Cola").click();
  await openCart(page, info);
  await page.getByTestId("cart-change").click();
  const chooser = page.getByTestId("cart-chooser");
  await chooser.locator("[data-player-lookup]").fill("9876");
  await chooser.getByTestId("player-hits").getByRole("button", { name: /Asha/ }).click();
  await page.keyboard.press("Control+Enter");
  await expect(page.getByTestId("undo-toast")).toBeVisible();
  if (isPhone(info)) await expect(page.getByTestId("phone-cart-bar")).toHaveCount(0);
  else await expect(page.getByTestId("cart-empty")).toBeVisible();

  await page.getByTestId("undo-action").click();
  await expect(page.getByTestId("undo-notice")).toHaveText(en["hub.undo.done.pos"]);
  expect(hub.sent[1].payload).toEqual({ kind: "pos.sale.reverted", order_id: "O1" });
  expect(hub.sent[1].rev).toBe(1);
  await openCart(page, info);
  await expect(lineOf(page, "Cola").getByTestId("line-qty")).toHaveText("2");
  await expect(page.getByTestId("cart-for")).toHaveText("Asha");
  await expect(page.getByTestId("settle-total")).toContainText("120.00");
});

test("Recent sales lists the day's counter sales with a read-only detail", async ({
  page,
}, info) => {
  const sale = (id: string, extra: Partial<PosSaleView>): PosSaleView => ({
    order_id: id,
    settled_at: Date.UTC(2026, 9, 28, 9, 45),
    staff: "Asha",
    lines: [
      {
        product_id: "P1",
        name: "Cola",
        qty: 2,
        unit_price_minor: 6000,
        rate_bp: 0,
        amount_minor: 12000,
      },
    ],
    payments: [{ method: "cash", amount_minor: 12000 }],
    subtotal_minor: 12000,
    tax_minor: 0,
    total_minor: 12000,
    invoice_no: null,
    ...extra,
  });
  const hub = newHub({
    sales: [
      sale("O2", {
        player: "asha_9",
        total_minor: 6300,
        subtotal_minor: 6000,
        tax_minor: 300,
        lines: [
          {
            product_id: "P2",
            name: "Chips",
            qty: 2,
            unit_price_minor: 3000,
            rate_bp: 500,
            amount_minor: 6000,
          },
        ],
        payments: [
          { method: "cash", amount_minor: 3300 },
          { method: "upi", amount_minor: 3000 },
        ],
      }),
      sale("O1", {}),
    ],
  });
  await openHub(page, hub);
  await page.getByTestId("recent-toggle").click();
  const rows = page.getByTestId("recent-sale");
  await expect(rows).toHaveCount(2);
  await expect(rows.first()).toContainText("asha_9");
  await expect(rows.first()).toContainText("63.00");
  await expect(rows.nth(1)).toContainText(en["hub.pos.cart.walkup"]);
  await expect(rows.first().getByTestId("recent-invoice")).toHaveText(
    en["hub.pos.recent.invoice_pending"],
  );
  await expect(page.getByTestId("sale-detail")).toHaveCount(0);
  await rows.first().getByTestId("recent-toggle-detail").click();
  const detail = rows.first().getByTestId("sale-detail");
  await expect(detail.getByTestId("sale-detail-line")).toHaveText([/2 × Chips at ₹30.00/]);
  await expect(detail.getByTestId("sale-detail-payment")).toHaveText([/Cash/, /UPI/]);
  await expect(detail).toContainText("63.00");
  // Read-only: nothing in the detail changes the sale.
  await expect(detail.getByRole("button")).toHaveCount(0);
  await page.getByTestId("recent-toggle").click();
  await expect(page.getByTestId("pos-tiles")).toBeVisible();
});

test("Recent sales says so when there are none", async ({ page }) => {
  await openHub(page, newHub());
  await page.getByTestId("recent-toggle").click();
  await expect(page.getByTestId("recent-empty")).toHaveText(en["hub.pos.recent.empty"]);
});

test("Recent sales offers Retry when the list cannot be loaded", async ({ page }) => {
  await openHub(page, newHub({ sales: "fail" }));
  await page.getByTestId("recent-toggle").click();
  await expect(
    page.getByRole("alert").filter({ hasText: en["hub.pos.recent.load_failed"] }),
  ).toBeVisible();
  await expect(page.getByRole("button", { name: en["hub.pos.retry"] })).toBeVisible();
});

test("scan, pay cash and settle takes well under 10 s", async ({ page }, info) => {
  const hub = newHub();
  await openHub(page, hub);
  const started = Date.now();
  await page.getByLabel(en["hub.pos.search.label"]).fill("8901");
  await page.keyboard.press("Enter");
  await openCart(page, info);
  await page.keyboard.press("Control+Enter");
  await expect(page.getByTestId("undo-toast")).toBeVisible();
  expect(Date.now() - started).toBeLessThan(10_000);
  expect(hub.sent[0].payload).toMatchObject({ lines: [{ product_id: "P1", qty: 1 }] });
});

test("the live settle bar has no serious accessibility violations", async ({ page }, info) => {
  await openHub(page, newHub());
  await tile(page, "Cola").click();
  await openCart(page, info);
  const results = await new AxeBuilder({ page }).analyze();
  const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  expect(bad.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual(
    [],
  );
});

// ---- Story 5.12: attach an order to a running session ----

const running = (id: string, device_id: string, extra: Partial<SnapshotSession> = {}) =>
  ({
    id,
    device_id,
    state: "active",
    started_at: 1,
    rev: 1,
    paused_total_ms: 0,
    mode: "postpaid",
    ...extra,
  }) as SnapshotSession;

const SESSIONS: SnapshotSession[] = [
  running("S1", device(1, {}).device_id, { username: "asha", player_id: "PL1" }),
  running("S2", device(2, {}).device_id, { guest_no: 4, started_at: 2 }),
  running("S3", device(3, {}).device_id, { mode: "wallet", started_at: 3 }),
];

const ordering = (extra: Partial<Hub> = {}) => newHub({ sessions: SESSIONS, ...extra });
const placeOf = (hub: Hub) => hub.sent.filter((s) => s.payload.kind === "order.place");

// The snapshot lists three PCs for these sessions.
async function openOrdering(page: Page, hub: Hub, session: string | null) {
  await openHub(page, hub, PRODUCTS, session ? `/pos?session=${session}` : "/pos");
}

test("a tile's Order opens the POS attached to the session, and the header names it", async ({
  page,
}, info) => {
  const hub = ordering();
  await openOrdering(page, hub, "S1");
  await tile(page, "Cola").click();
  await openCart(page, info);
  await expect(page.getByTestId("cart-for")).toHaveText(/In session · asha · PC \d/);
  // The settle bar is replaced by "Add to session bill"; there is no payment form.
  await expect(page.getByTestId("order-bar")).toBeVisible();
  await expect(page.getByTestId("settle-bar")).toHaveCount(0);
  await expect(page.getByTestId("order-bar").getByRole("button")).toHaveText(
    en["hub.pos.session.add"],
  );
  await expect(page.getByTestId("cart-placed-total")).toContainText("60.00");
});

test("Add to session bill sends order.place, shows the Undo and keeps the session attached", async ({
  page,
}, info) => {
  const hub = ordering();
  await openOrdering(page, hub, "S1");
  await tile(page, "Cola").click();
  await tile(page, "Cola").click();
  await openCart(page, info);
  await page.getByTestId("order-bar").getByRole("button").click();
  await expect(page.getByTestId("undo-toast")).toBeVisible();
  expect(placeOf(hub)).toHaveLength(1);
  expect(placeOf(hub)[0].payload).toEqual({
    kind: "order.place",
    session_id: "S1",
    lines: [{ product_id: "P1", qty: 2 }],
  });
  expect(placeOf(hub)[0].rev).toBeUndefined();
  // A second placement goes to the same session.
  await page.getByRole("button", { name: en["hub.pos.all"], exact: true }).click();
  await tile(page, "Chips").click();
  await openCart(page, info);
  await expect(page.getByTestId("cart-for")).toContainText("asha");
  await page.getByTestId("order-bar").getByRole("button").click();
  await expect.poll(() => placeOf(hub).length).toBe(2);
  expect(placeOf(hub)[1].payload).toMatchObject({ session_id: "S1" });
});

test("Undo of an order sends order.place.reverted with the order rev and puts the cart back", async ({
  page,
}, info) => {
  const hub = ordering();
  await openOrdering(page, hub, "S2");
  await tile(page, "Cola").click();
  await openCart(page, info);
  await expect(page.getByTestId("cart-for")).toContainText("Guest 4 · PC 2");
  await page.getByTestId("order-bar").getByRole("button").click();
  await expect(page.getByTestId("undo-toast")).toContainText("120.00");
  await page.getByTestId("undo-action").click();
  await expect(page.getByTestId("undo-notice")).toHaveText(en["hub.undo.done.order"]);
  const undone = hub.sent.at(-1);
  expect(undone?.payload).toEqual({
    kind: "order.place.reverted",
    order_id: "O9",
    placement_id: "PL9",
  });
  expect(undone?.rev).toBe(2);
  await openCart(page, info);
  await expect(lineOf(page, "Cola").getByTestId("line-qty")).toHaveText("1");
  await expect(page.getByTestId("cart-for")).toContainText("Guest 4");
});

test("Change attaches any running session by device or player, or goes back to a guest", async ({
  page,
}, info) => {
  const hub = ordering();
  await openOrdering(page, hub, null);
  await page.getByRole("button", { name: en["hub.pos.all"], exact: true }).click();
  await tile(page, "Chips").click();
  await openCart(page, info);
  await expect(page.getByTestId("cart-for")).toHaveText(en["hub.pos.cart.walkup"]);
  await expect(page.getByTestId("settle-bar")).toBeVisible();
  await page.getByTestId("cart-change").click();
  const options = page.getByTestId("cart-session-option");
  // Only sessions that take an order are offered (the wallet session is not).
  await expect(options).toHaveCount(2);
  await options.filter({ hasText: "Guest 4" }).click();
  await expect(page.getByTestId("cart-for")).toContainText("Guest 4");
  await expect(page.getByTestId("order-bar")).toBeVisible();
  // Looking a player up attaches their running session.
  await page.getByTestId("cart-change").click();
  const chooser = page.getByTestId("cart-chooser");
  await chooser.locator("[data-player-lookup]").fill("9876");
  await chooser.getByTestId("player-hits").getByRole("button", { name: /Asha/ }).click();
  await expect(page.getByTestId("cart-for")).toContainText("asha");
  // Guest goes back to the counter sale.
  await page.getByTestId("cart-change").click();
  await page
    .getByTestId("cart-chooser")
    .getByRole("button", { name: en["hub.players.lookup.guest"], exact: true })
    .click();
  await expect(page.getByTestId("cart-for")).toHaveText(en["hub.pos.cart.walkup"]);
  await expect(page.getByTestId("settle-bar")).toBeVisible();
  await expect(page.getByTestId("order-bar")).toHaveCount(0);
});

test("a short bundle on an order offers the manager PIN and orders with the override", async ({
  page,
}, info) => {
  const hub = ordering({
    refuse: [{ status: 409, code: "BUNDLE_SHORT", message: "Not enough Chips for Combo." }],
  });
  await openOrdering(page, hub, "S1");
  await page.getByRole("button", { name: "Combos", exact: true }).click();
  await tile(page, "Combo").click();
  await openCart(page, info);
  await page.getByTestId("order-bar").getByRole("button").click();
  const panel = page.getByTestId("pos-override");
  await expect(panel.getByTestId("pos-short-message")).toHaveText("Not enough Chips for Combo.");
  await expect(panel.locator("[data-component=inline-pin]")).toBeVisible();
  await page.keyboard.type("2222");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("undo-toast")).toBeVisible();
  expect(hub.elevations.at(-1)).toMatchObject({
    items: [{ intent_kind: "order.place" }],
    pin: "2222",
  });
  expect(placeOf(hub)).toHaveLength(2);
  expect(placeOf(hub)[1].payload).toMatchObject({ stock_override: true, session_id: "S1" });
  expect(placeOf(hub)[1].ticket).toBe("T1");
});

test("a session that no longer takes an order says so and keeps the cart", async ({
  page,
}, info) => {
  const hub = ordering({
    refuse: [
      {
        status: 400,
        code: "VALIDATION_FAILED",
        message: "an order goes on a running session",
      },
    ],
  });
  await openOrdering(page, hub, "S1");
  await tile(page, "Cola").click();
  await openCart(page, info);
  await page.getByTestId("order-bar").getByRole("button").click();
  await expect(page.getByTestId("pos-error")).toBeVisible();
  await expect(page.getByTestId("undo-toast")).toHaveCount(0);
  await expect(lineOf(page, "Cola")).toBeVisible();
});

test("Add to session bill is disabled with a reason while the cart is empty", async ({
  page,
}, info) => {
  test.skip(isPhone(info), "the phone cart bar shows only once the cart has a line");
  await openOrdering(page, ordering(), "S1");
  const bar = page.getByTestId("order-bar");
  await expect(bar.getByRole("button")).toHaveAttribute("aria-disabled", "true");
  await expect(bar).toContainText(en["hub.pos.session.add_empty"]);
});

test("the attached POS has no serious accessibility violations", async ({ page }, info) => {
  await openOrdering(page, ordering(), "S1");
  await tile(page, "Cola").click();
  await openCart(page, info);
  await page.getByTestId("cart-change").click();
  const results = await new AxeBuilder({ page }).analyze();
  expect(
    results.violations.filter((v) => v.impact === "serious" || v.impact === "critical"),
  ).toEqual([]);
});

// ---- Story 5.17: the receipt choice at settle, and Reprint ----

const READY: PrinterReady = { ready: true, default_choice: "print" };

async function cartOfOneCola(page: Page, info: TestInfo) {
  await tile(page, "Cola").click();
  await openCart(page, info);
}

test("with a printer ready the bar says so, preselects the branch default and a Print settle has no Undo", async ({
  page,
}, info) => {
  const hub = newHub({ printer: READY });
  await openHub(page, hub);
  await cartOfOneCola(page, info);
  await expect(page.getByTestId("printer-ready")).toHaveText(en["hub.settle.printer_ready"]);
  await expect(page.locator('[data-receipt="print"]')).toHaveAttribute("aria-pressed", "true");
  await expect(page.locator('[data-receipt="none"]')).toHaveAttribute("aria-pressed", "false");
  await page.getByRole("button", { name: en["hub.settle.submit"], exact: true }).click();
  await expect.poll(() => hub.sent.length).toBe(1);
  expect(hub.sent[0].payload).toMatchObject({ kind: "pos.sale", receipt: "print" });
  // A printed bill is irreversible: the Hub says so and no Undo toast is shown.
  await expect(page.getByTestId("undo-toast")).toHaveCount(0);
});

test("choosing None sends none and the sale keeps its Undo", async ({ page }, info) => {
  const hub = newHub({ printer: READY });
  await openHub(page, hub);
  await cartOfOneCola(page, info);
  await page.locator('[data-receipt="none"]').click();
  await expect(page.locator('[data-receipt="none"]')).toHaveAttribute("aria-pressed", "true");
  await page.getByRole("button", { name: en["hub.settle.submit"], exact: true }).click();
  await expect(page.getByTestId("undo-toast")).toBeVisible();
  expect(hub.sent[0].payload).toMatchObject({ kind: "pos.sale", receipt: "none" });
});

test("a branch whose default is None starts on None", async ({ page }, info) => {
  const hub = newHub({ printer: { ready: true, default_choice: "none" } });
  await openHub(page, hub);
  await cartOfOneCola(page, info);
  await expect(page.locator('[data-receipt="none"]')).toHaveAttribute("aria-pressed", "true");
});

test("with no printer Print is not offered, the bar says nothing will print and settle still works", async ({
  page,
}, info) => {
  const hub = newHub({ printer: { ready: false, default_choice: "print", reason: "unset" } });
  await openHub(page, hub);
  await cartOfOneCola(page, info);
  await expect(page.getByTestId("printer-missing")).toContainText(en["hub.settle.no_printer"]);
  await expect(page.locator("[data-receipt]")).toHaveCount(0);
  await page.getByRole("button", { name: en["hub.settle.submit"], exact: true }).click();
  await expect(page.getByTestId("undo-toast")).toBeVisible();
  expect(hub.sent[0].payload).toMatchObject({ kind: "pos.sale", receipt: "none" });
});

test("a Hub that does not answer the printer check never blocks the settle", async ({
  page,
}, info) => {
  const hub = newHub();
  await openHub(page, hub);
  await cartOfOneCola(page, info);
  await expect(page.getByTestId("printer-missing")).toBeVisible();
  await page.getByRole("button", { name: en["hub.settle.submit"], exact: true }).click();
  await expect(page.getByTestId("undo-toast")).toBeVisible();
});

test("Recent sales offers Reprint for a numbered sale only, and sends receipt.reprint", async ({
  page,
}) => {
  const sale = (id: string, extra: Partial<PosSaleView>): PosSaleView => ({
    order_id: id,
    settled_at: Date.UTC(2026, 9, 28, 9, 45),
    staff: "Asha",
    lines: [],
    payments: [{ method: "cash", amount_minor: 6000 }],
    subtotal_minor: 6000,
    tax_minor: 0,
    total_minor: 6000,
    invoice_no: null,
    ...extra,
  });
  const hub = newHub({
    printer: READY,
    sales: [
      sale("O2", { invoice_no: "DEV01/26-27/E1k/000002", print_state: "failed" }),
      sale("O1", { whatsapp_state: "queued" }),
    ],
  });
  await openHub(page, hub);
  await page.getByTestId("recent-toggle").click();
  const rows = page.getByTestId("recent-sale");
  await expect(rows).toHaveCount(2);
  await expect(rows.nth(1).getByTestId("recent-whatsapp-queued")).toHaveText(
    en["hub.pos.recent.whatsapp_queued"],
  );
  await expect(rows.first().getByTestId("recent-whatsapp-queued")).toHaveCount(0);
  await expect(rows.first().getByTestId("recent-print-failed")).toBeVisible();
  await expect(rows.first().getByTestId("recent-reprint")).toBeVisible();
  await expect(rows.nth(1).getByTestId("recent-reprint")).toHaveCount(0);
  await rows.first().getByTestId("recent-reprint").click();
  await expect(page.getByTestId("reprint-note")).toHaveText(en["hub.pos.recent.reprint_queued"]);
  expect(hub.sent).toHaveLength(1);
  expect(hub.sent[0].payload).toEqual({ kind: "receipt.reprint", order_id: "O2" });
});

// ---- Story 6.32: Open orders ----

const counterOrder = (id: string, extra: Partial<SnapshotBill> = {}): SnapshotBill => ({
  session_id: "",
  device_id: "D1",
  order_id: id,
  order_rev: 3,
  lines: [],
  items: [{ line_id: "L1", served: true, label: "Cola", qty: 2, amount_minor: 12000 }],
  subtotal_minor: 12000,
  paid_minor: 0,
  remaining_minor: 12000,
  ...extra,
});

test("Open orders lists the kiosk orders left unpaid and settles one with any payment mix", async ({
  page,
}) => {
  const hub = newHub({ bills: [counterOrder("OU1")] });
  await openHub(page, hub);
  await expect(page.getByTestId("open-toggle")).toHaveText(
    en["hub.pos.open.toggle_count"].replace("{count}", "1"),
  );
  await page.getByTestId("open-toggle").click();
  const row = page.getByTestId("open-order");
  await expect(row).toHaveCount(1);
  await expect(page.getByTestId("open-order-items")).toContainText("2 × Cola");
  await expect(page.getByTestId("open-order-total")).toContainText("120.00");

  await row.getByRole("button", { name: en["hub.pos.open.load"] }).click();
  await expect(page.getByTestId("settle-total")).toContainText("120.00");
  await expect(cashInput(page)).toHaveValue("120.00");
  await cashInput(page).fill("70");
  await page.locator('[data-method="upi"]').click();
  await expect(page.locator('[data-row="upi"] input').first()).toHaveValue("50.00");
  await page.locator('[data-row="upi"] [data-reference] input').fill("1234");
  await page
    .getByTestId("open-orders")
    .getByRole("button", { name: en["hub.settle.submit"], exact: true })
    .click();
  await expect.poll(() => hub.sent.length).toBe(1);
  expect(hub.sent[0].payload).toMatchObject({
    kind: "session.settle",
    order_id: "OU1",
    payments: [
      { method: "cash", amount_minor: 7000 },
      { method: "upi", amount_minor: 5000, reference: "1234" },
    ],
  });
  expect(hub.sent[0].rev).toBe(3);
});

test("Open orders says so when no kiosk order waits", async ({ page }) => {
  await openHub(page, newHub());
  await expect(page.getByTestId("open-toggle")).toHaveText(en["hub.pos.open.toggle"]);
  await page.getByTestId("open-toggle").click();
  await expect(page.getByTestId("open-orders-empty")).toHaveText(en["hub.pos.open.empty"]);
});

const counterTicket = (number: number, stage: string, rev = 3): KitchenTicketView => ({
  id: `T-${number}`,
  order_id: `O-${number}`,
  number,
  seat: "Counter",
  stage,
  placed_at: Date.now() - 60_000,
  rev,
  lines: [{ line_id: `L-${number}`, label: "Burger", qty: 2, notes: ["No onion"] }],
});

test("the open-tickets strip lists walk-up tickets and a Ready one moves on from its detail", async ({
  page,
}) => {
  const hub = newHub({
    permissions: ["pos.sell", "kitchen.use"],
    counterTickets: [counterTicket(2042, "preparing"), counterTicket(2043, "ready", 5)],
  });
  await openHub(page, hub);
  const strip = page.getByTestId("open-tickets");
  await expect(strip.getByTestId("open-ticket")).toHaveCount(2);
  await expect(strip.getByTestId("open-ticket").nth(1)).toHaveText("K-2043 READY");
  await expect(strip.getByTestId("open-ticket").nth(0)).toHaveText("K-2042 PREPARING");
  await strip.getByTestId("open-ticket").nth(1).click();
  await expect(strip.getByTestId("open-ticket-detail")).toContainText("2 × Burger");
  await expect(strip.getByTestId("open-ticket-detail")).toContainText("No onion");
  await strip
    .getByTestId("open-ticket-detail")
    .getByRole("button", { name: en["hub.order.mark_served"] })
    .click();
  await expect.poll(() => hub.sent.length).toBe(1);
  expect(hub.sent[0].payload).toEqual({
    kind: "kitchen.advance",
    ticket_id: "T-2043",
    to: "served",
  });
  expect(hub.sent[0].rev).toBe(5);
});

test("the open-tickets strip is absent when no walk-up ticket is open (a Free branch)", async ({
  page,
}) => {
  await openHub(page, newHub());
  await expect(page.getByTestId("pos")).toBeVisible();
  await expect(page.getByTestId("open-tickets")).toHaveCount(0);
});

test("the open-tickets strip shows KOT not printed and Reprint KOT sends kot.reprint", async ({
  page,
}) => {
  const failed = { ...counterTicket(2042, "preparing"), kot: "failed" };
  const hub = newHub({
    permissions: ["pos.sell", "kitchen.use"],
    counterTickets: [failed],
  });
  await openHub(page, hub);
  const strip = page.getByTestId("open-tickets");
  await strip.getByTestId("open-ticket").click();
  await expect(strip.getByTestId("open-ticket-kot-failed")).toHaveText(en["hub.order.kot_failed"]);
  await strip
    .getByTestId("open-ticket-detail")
    .getByRole("button", { name: en["hub.order.kot_reprint"] })
    .click();
  await expect.poll(() => hub.sent.length).toBe(1);
  expect(hub.sent[0].payload).toEqual({ kind: "kot.reprint", ticket_id: "T-2042" });
});
