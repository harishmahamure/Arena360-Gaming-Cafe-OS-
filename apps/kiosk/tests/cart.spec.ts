import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import names from "../src-tauri/ipc-names.json" with { type: "json" };
import {
  cartCount,
  cartTotal,
  placeOf,
  refreshLines,
  stockProblem,
  stepCart,
  type CartLine,
} from "../src/lib/cart";

// Story 6.31: the cart on the Shop tab. The Tauri shell is stubbed in the page; `__service` holds
// what the Hub would answer to `shop_menu` and `shop_place` (a test changes it to play a Hub stock
// change or a refusal) and `__calls` records what the page asked of the shell.

type Item = {
  id: string;
  name: string;
  category: string;
  price_minor: number;
  bundle: boolean;
  available: boolean;
  max_qty?: number;
  has_image: boolean;
  popular: boolean;
  components: { name: string; qty: number }[];
};
type Menu = { enabled: boolean; items: Item[] };

const INR = {
  currency: "INR",
  exponent: 2,
  symbol: "₹",
  symbol_position: "prefix",
  number_format: "indian",
  tz: "Asia/Kolkata",
  day_start_min: 0,
  fy_start_month: 4,
  pending: null,
};

const item = (id: string, name: string, extra: Partial<Item> = {}): Item => ({
  id,
  name,
  category: "Drinks",
  price_minor: 4000,
  bundle: false,
  available: true,
  has_image: false,
  popular: false,
  components: [],
  ...extra,
});

const MENU: Menu = {
  enabled: true,
  items: [
    item("B1", "Gamer combo", {
      price_minor: 9000,
      bundle: true,
      components: [
        { name: "Cola", qty: 2 },
        { name: "Chips", qty: 1 },
      ],
    }),
    item("P1", "Cola", { max_qty: 2 }),
    item("P2", "Iced tea", { available: false, max_qty: 0 }),
    item("P4", "Chips"),
  ],
};

async function openShop(page: Page, menu: Menu | null = MENU, hub = "connected") {
  await page.addInitScript(
    ([commands, initial]: [typeof names.commands, Menu | null]) => {
      const callbacks = new Map<number, (e: unknown) => void>();
      const listeners: Record<string, number[]> = {};
      let next = 1;
      const w = window as unknown as Record<string, unknown>;
      w.__calls = [] as unknown[][];
      w.__service = { menu: initial, place: { ok: true, body: { outcome: "ok" } } };
      w.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
        unregisterListener: (event: string, id: number) => {
          listeners[event] = (listeners[event] ?? []).filter((x) => x !== id);
        },
      };
      w.__TAURI_INTERNALS__ = {
        transformCallback: (cb: (e: unknown) => void) => {
          const id = next++;
          callbacks.set(id, cb);
          return id;
        },
        invoke: async (cmd: string, args: Record<string, unknown>) => {
          const service = w.__service as { menu: unknown; place: unknown };
          switch (cmd) {
            case "plugin:event|listen":
              (listeners[args.event as string] ??= []).push(args.handler as number);
              return args.handler;
            case "plugin:event|unlisten":
              return null;
            case commands.shopMenu:
              return service.menu === null
                ? { ok: false, code: "HUB_SLOW", body: null }
                : { ok: true, body: service.menu };
            case commands.shopPlace:
              (w.__calls as unknown[][]).push(["shop_place", args.lines]);
              return service.place;
            case commands.shopImage:
              return { ok: true, body: { id: args.id, image: null } };
            case commands.games:
              return { ok: true, body: { games: [], running: false } };
            case commands.apps:
              return { ok: true, body: { apps: [] } };
            case commands.launchers:
              return { ok: true, body: { launchers: [] } };
            case commands.mostPlayed:
              return { ok: true, body: { top: [], last_played: [] } };
            case commands.staffMessages:
              return [];
            case commands.pcScreen:
            case commands.attract:
            case commands.hubStatus:
            case commands.takeRestoreState:
            case commands.reportAppState:
            case commands.heartbeat:
            case commands.adminExitStatus:
            case commands.theme:
            case commands.volumeGet:
              return null;
            default:
              throw new Error(`unexpected command ${cmd}`);
          }
        },
      };
      w.__shell = {
        emit: (event: string, payload: unknown) => {
          for (const id of listeners[event] ?? []) callbacks.get(id)?.({ event, id, payload });
        },
        listening: (event: string) => (listeners[event] ?? []).length > 0,
      };
    },
    [names.commands, menu] as [typeof names.commands, Menu | null],
  );
  await page.goto("/");
  for (const event of [names.events.pcScreen, names.events.attract, names.events.hubStatus]) {
    await page.waitForFunction(
      (e) =>
        (window as unknown as { __shell: { listening: (e: string) => boolean } }).__shell.listening(
          e,
        ),
      event,
    );
  }
  await page.evaluate(
    ([locale, state]) => {
      const w = window as unknown as {
        __shell: { emit: (e: string, p: unknown) => void };
      };
      w.__shell.emit("hub-status", { state });
      w.__shell.emit("attract", {
        view: {
          branch_name: "Arena",
          state: "ready",
          locale,
          rate_minor: 6000,
          media: [],
          zone: "Zone A",
        },
        logo: null,
      });
      w.__shell.emit("pc-screen", {
        locked: false,
        reason: null,
        session: { device_name: "PC 7", elapsed_ms: 0, username: "asha", remaining_ms: null },
      });
    },
    [INR, hub] as const,
  );
  await expect(page.getByTestId("shell")).toBeVisible();
  await page.getByTestId("tab-shop").click();
  await expect(page.getByTestId("shop")).toBeVisible();
}

const service = (page: Page, patch: { menu?: Menu | null; place?: unknown }) =>
  page.evaluate((p) => {
    Object.assign((window as unknown as { __service: object }).__service, p);
  }, patch);

const calls = (page: Page) =>
  page.evaluate(() => (window as unknown as { __calls: unknown[][] }).__calls);

const add = (page: Page, name: string) =>
  page
    .getByTestId("shop-item-card")
    .filter({ has: page.getByTestId("shop-item-name").getByText(name, { exact: true }) })
    .getByTestId("shop-item-add")
    .click();

const line = (page: Page, name: string) =>
  page
    .getByTestId("cart-line")
    .filter({ has: page.getByTestId("cart-line-name").getByText(name, { exact: true }) });

test("Add opens the cart as a right-side panel addressed to the PC", async ({ page }) => {
  await openShop(page);
  await expect(page.getByTestId("cart")).toHaveCount(0);
  await add(page, "Chips");
  const cart = page.getByTestId("cart");
  await expect(cart).toBeVisible();
  await expect(cart.getByRole("heading")).toHaveText(en["player.cart.title"]);
  await expect(page.getByTestId("cart-to")).toHaveText("To PC 7 · Zone A");
  await expect(line(page, "Chips").getByTestId("cart-qty")).toHaveText("1");
  await expect(page.getByTestId("cart-total")).toContainText("40.00");
  await expect(page.getByTestId("cart-tax-note")).toHaveText(en["player.cart.tax_note"]);
  // The panel sits on the right edge of the window.
  const box = await cart.boundingBox();
  const width = page.viewportSize()!.width;
  expect(box!.x + box!.width).toBeGreaterThan(width - 40);
  expect(box!.x).toBeGreaterThan(width / 2);
  expect(
    await new AxeBuilder({ page })
      .analyze()
      .then((r) => r.violations.filter((v) => v.impact === "serious" || v.impact === "critical")),
  ).toEqual([]);
});

test("steppers change the quantity and the last step down empties the cart", async ({ page }) => {
  await openShop(page);
  await add(page, "Chips");
  await line(page, "Chips").getByTestId("cart-more").click();
  await expect(line(page, "Chips").getByTestId("cart-qty")).toHaveText("2");
  await expect(page.getByTestId("cart-total")).toContainText("80.00");
  await line(page, "Chips").getByTestId("cart-less").click();
  await line(page, "Chips").getByTestId("cart-less").click();
  await expect(page.getByTestId("cart-empty")).toHaveText(en["player.cart.empty"]);
  await expect(page.getByTestId("cart-place")).toHaveCount(0);
  await expect(page.getByTestId("shop-cart-button")).toContainText("0");
});

test("a step past the live stock is refused with the reason inline and the quantity stays", async ({
  page,
}) => {
  await openShop(page);
  await add(page, "Cola");
  await line(page, "Cola").getByTestId("cart-more").click();
  await expect(line(page, "Cola").getByTestId("cart-qty")).toHaveText("2");
  await line(page, "Cola").getByTestId("cart-more").click();
  await expect(line(page, "Cola").getByTestId("cart-qty")).toHaveText("2");
  await expect(line(page, "Cola").getByTestId("cart-line-problem")).toContainText(
    en["player.cart.only_left"].replace("{count}", "2"),
  );
  // The stock is asked of the Hub again on each step up: a restock lets the step through.
  await service(page, {
    menu: {
      enabled: true,
      items: MENU.items.map((i) => (i.id === "P1" ? { ...i, max_qty: 5 } : i)),
    },
  });
  await line(page, "Cola").getByTestId("cart-more").click();
  await expect(line(page, "Cola").getByTestId("cart-qty")).toHaveText("3");
  await expect(line(page, "Cola").getByTestId("cart-line-problem")).toHaveCount(0);
});

test("a bundle line lists its components, scaled to the quantity", async ({ page }) => {
  await openShop(page);
  await add(page, "Gamer combo");
  await line(page, "Gamer combo").getByTestId("cart-more").click();
  await expect(
    line(page, "Gamer combo").getByTestId("cart-line-components").locator("li"),
  ).toHaveText(["4 × Cola", "2 × Chips"]);
});

test("a running session offers Add to session bill and Place order puts the cart on it", async ({
  page,
}) => {
  await openShop(page);
  await add(page, "Chips");
  await add(page, "Gamer combo");
  await expect(page.getByTestId("cart-pay-bill")).toBeChecked();
  await expect(page.getByTestId("cart-pay-note")).toHaveText(en["player.cart.pay_bill_note"]);
  await page.getByTestId("cart-place").click();
  await expect(page.getByTestId("cart-placed")).toHaveText(en["player.cart.placed"]);
  await expect(page.getByTestId("cart-lines")).toHaveCount(0);
  expect(await calls(page)).toEqual([
    [
      "shop_place",
      [
        { product_id: "P4", qty: 1 },
        { product_id: "B1", qty: 1 },
      ],
    ],
  ]);
  // The next Add starts a fresh cart.
  await add(page, "Chips");
  await expect(page.getByTestId("cart-placed")).toHaveCount(0);
  await expect(line(page, "Chips").getByTestId("cart-qty")).toHaveText("1");
});

test("a stock shortage at placement keeps the cart and marks the short line", async ({ page }) => {
  await openShop(page);
  await add(page, "Cola");
  await add(page, "Chips");
  await service(page, {
    place: {
      ok: true,
      body: {
        outcome: "refused",
        code: "OUT_OF_STOCK",
        message: "Only 0 Cola left.",
        product_id: "P1",
      },
    },
  });
  await page.getByTestId("cart-place").click();
  await expect(line(page, "Cola").getByTestId("cart-line-problem")).toHaveText(
    new RegExp(en["player.cart.out_of_stock"]),
  );
  await expect(line(page, "Chips").getByTestId("cart-line-problem")).toHaveCount(0);
  await expect(page.getByTestId("cart-lines")).toBeVisible();
  await expect(page.getByTestId("cart-place")).toBeEnabled();

  await service(page, {
    place: {
      ok: true,
      body: { outcome: "refused", code: "BUNDLE_SHORT", message: "x", product_id: "P1" },
    },
  });
  await page.getByTestId("cart-place").click();
  await expect(line(page, "Cola").getByTestId("cart-line-problem")).toContainText(
    en["player.cart.bundle_short"],
  );
});

test("another refusal says to ask at the counter and keeps the cart", async ({ page }) => {
  await openShop(page);
  await add(page, "Chips");
  await service(page, {
    place: {
      ok: true,
      body: { outcome: "refused", code: "VALIDATION_FAILED", message: "no", product_id: null },
    },
  });
  await page.getByTestId("cart-place").click();
  await expect(page.getByTestId("cart-refused")).toContainText(en["player.cart.refused"]);
  await expect(line(page, "Chips")).toBeVisible();
});

test("with the Hub unreachable Place order is disabled and says the counter is reconnecting", async ({
  page,
}) => {
  await openShop(page);
  await add(page, "Chips");
  await page.evaluate(() =>
    (window as unknown as { __shell: { emit: (e: string, p: unknown) => void } }).__shell.emit(
      "hub-status",
      { state: "reconnecting" },
    ),
  );
  await expect(page.getByTestId("cart-place")).toBeDisabled();
  await expect(page.getByTestId("cart-reconnecting")).toHaveText(en["player.cart.reconnecting"]);
  expect(await calls(page)).toEqual([]);
});

test("a placement the Hub never answers keeps the cart and says the counter is reconnecting", async ({
  page,
}) => {
  await openShop(page);
  await add(page, "Chips");
  await service(page, { place: { ok: false, code: "HUB_SLOW", body: null } });
  await page.getByTestId("cart-place").click();
  await expect(page.getByTestId("cart-reconnecting")).toBeVisible();
  await expect(line(page, "Chips")).toBeVisible();
  await expect(page.getByTestId("cart-placed")).toHaveCount(0);
});

test("Esc closes the cart and the Shop button opens it again with the cart kept", async ({
  page,
}) => {
  await openShop(page);
  await add(page, "Chips");
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("cart")).toHaveCount(0);
  await expect(page.getByTestId("shop-cart-button")).toBeFocused();
  await expect(page.getByTestId("shop-cart-button")).toContainText("1");
  await page.getByTestId("shop-cart-button").click();
  await expect(line(page, "Chips").getByTestId("cart-qty")).toHaveText("1");
});

test("an unavailable card cannot be added", async ({ page }) => {
  await openShop(page);
  await expect(
    page.getByTestId("shop-item-card").filter({ hasText: "Iced tea" }).getByTestId("shop-item-add"),
  ).toBeDisabled();
});

// ---- the pure rules ----

const cola = item("P1", "Cola", { max_qty: 2 });

test("stepCart checks a step up against the fresh item and never mutates the cart", () => {
  const empty: CartLine[] = [];
  const one = stepCart(empty, "P1", 1, cola);
  expect(one.problem).toBeNull();
  expect(one.lines).toEqual([{ item: cola, qty: 1 }]);
  expect(empty).toEqual([]);
  const two = stepCart(one.lines, "P1", 1, cola);
  expect(two.lines[0].qty).toBe(2);
  const three = stepCart(two.lines, "P1", 1, cola);
  expect(three.problem).toEqual({ kind: "only", left: 2 });
  expect(three.lines).toBe(two.lines);
  expect(stepCart(two.lines, "P1", 1, undefined).problem).toEqual({ kind: "out_of_stock" });
  expect(stepCart(two.lines, "P1", 1, { ...cola, available: false }).problem).toEqual({
    kind: "out_of_stock",
  });
  expect(stepCart(one.lines, "P1", -1, cola).lines).toEqual([]);
  expect(stepCart(empty, "P1", -1, cola).lines).toBe(empty);
  expect(stepCart(empty, "P1", 1, { ...cola, max_qty: undefined }).problem).toBeNull();
});

test("the cart's count, total and refresh follow the Hub's latest listing", () => {
  const lines: CartLine[] = [
    { item: cola, qty: 2 },
    { item: item("P4", "Chips", { price_minor: 1500 }), qty: 3 },
  ];
  expect(cartCount(lines)).toBe(5);
  expect(cartTotal(lines)).toBe(2 * 4000 + 3 * 1500);
  const fresh = refreshLines(lines, [{ ...cola, price_minor: 5000, max_qty: 1 }]);
  expect(fresh[0].item.price_minor).toBe(5000);
  // A product that left the menu is marked unavailable, so its line shows out of stock.
  expect(fresh[1].item.available).toBe(false);
  expect(stockProblem(fresh[1].item, fresh[1].qty)).toEqual({ kind: "out_of_stock" });
});

test("a 51st distinct product is refused with the limit", () => {
  const lines: CartLine[] = Array.from({ length: 50 }, (_, i) => ({
    item: item(`X${i}`, `Item ${i}`),
    qty: 1,
  }));
  const next = item("X50", "One more");
  expect(stepCart(lines, "X50", 1, next)).toEqual({ lines, problem: { kind: "limit" } });
  expect(stepCart(lines, "X3", 1, item("X3", "Item 3")).lines[3].qty).toBe(2);
});

test("placeOf reads the Hub's typed answer and treats anything else as unreachable", () => {
  expect(placeOf({ ok: true, body: { outcome: "ok" } })).toEqual({ kind: "ok" });
  expect(
    placeOf({
      ok: true,
      body: { outcome: "refused", code: "OUT_OF_STOCK", message: "m", product_id: "P1" },
    }),
  ).toEqual({ kind: "refused", code: "OUT_OF_STOCK", message: "m", productId: "P1" });
  expect(placeOf({ ok: true, body: { outcome: "refused", code: "X", message: "m" } })).toEqual({
    kind: "refused",
    code: "X",
    message: "m",
    productId: null,
  });
  for (const bad of [
    null,
    {},
    { ok: false, code: "HUB_SLOW" },
    { ok: true, body: null },
    { ok: true, body: { outcome: "?" } },
  ]) {
    expect(placeOf(bad)).toEqual({ kind: "unreachable" });
  }
});
