import { expect, test } from "@playwright/test";
import type { Product } from "@arena/ui/generated";
import {
  ALL,
  FAVOURITES,
  MAX_LINE_QTY,
  SCAN_GAP_MS,
  ScanBurst,
  DEFAULT_TAX,
  cartBill,
  cartCount,
  cartTotal,
  computeBill,
  lineRate,
  saleLines,
  findByBarcode,
  lineKey,
  lineShortage,
  normNotes,
  renote,
  step,
  unavailableTag,
  visibleProducts,
  withQty,
} from "../src/lib/pos";

// Unit tests for the POS rules (Story 5.9); no browser page is used.

const product = (id: string, extra: Partial<Product> = {}): Product => ({
  id,
  name: id,
  category: "Drinks",
  price_minor: 100,
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
  quantity: 5,
  avg_cost_minor: 0,
  available: true,
  ...extra,
});

const index = (...list: Product[]) => new Map(list.map((p) => [p.id, p]));

test("tags: an unavailable product says why, an available one has none", () => {
  expect(unavailableTag(product("a"))).toBeNull();
  expect(unavailableTag(product("a", { available: false, reason: "out_of_stock" }))).toBe(
    "hub.pos.tag.out_of_stock",
  );
  expect(unavailableTag(product("a", { available: false, reason: "expired" }))).toBe(
    "hub.pos.tag.expired",
  );
  expect(unavailableTag(product("a", { available: false, reason: "component_inactive" }))).toBe(
    "hub.pos.tag.unavailable",
  );
});

test("a search looks through every product; otherwise the category or favourites decide", () => {
  const list = [
    product("Cola", { favourite: true }),
    product("Chips", { category: "Snacks" }),
    product("COCOA"),
  ];
  expect(visibleProducts(list, ALL, "")).toHaveLength(3);
  expect(visibleProducts(list, FAVOURITES, "").map((p) => p.id)).toEqual(["Cola"]);
  expect(visibleProducts(list, "Snacks", "").map((p) => p.id)).toEqual(["Chips"]);
  expect(visibleProducts(list, "Snacks", " co ").map((p) => p.id)).toEqual(["Cola", "COCOA"]);
});

test("a barcode is matched exactly and never when blank", () => {
  const list = [product("a", { barcode: "123" }), product("b")];
  expect(findByBarcode(list, " 123 ")?.id).toBe("a");
  expect(findByBarcode(list, "12")).toBeUndefined();
  expect(findByBarcode(list, "")).toBeUndefined();
});

test("a step up past stock is refused, a step down never is, zero removes the line", () => {
  const cola = product("cola", { quantity: 2 });
  const by = index(cola);
  let cart = step([], cola, 1, by).cart;
  cart = step(cart, cola, 1, by).cart;
  const refused = step(cart, cola, 1, by);
  expect(refused.refusal).toEqual({ name: "cola", left: 2 });
  expect(refused.cart).toEqual(cart);
  const short = index(product("cola", { quantity: 1 }));
  expect(step(cart, cola, -1, short)).toEqual({
    cart: [{ productId: "cola", qty: 1 }],
    refusal: null,
  });
  expect(step([{ productId: "cola", qty: 1 }], cola, -1, by).cart).toEqual([]);
});

test("an untracked product has no stock limit but a line holds at most 999", () => {
  const gum = product("gum", { track_stock: false, quantity: 0 });
  const by = index(gum);
  const near = [{ productId: "gum", qty: MAX_LINE_QTY - 1 }];
  const full = step(near, gum, 1, by);
  expect(full.refusal).toBeNull();
  expect(step(full.cart, gum, 1, by).refusal).toEqual({
    name: "gum",
    left: MAX_LINE_QTY,
    cap: true,
  });
});

test("a bundle draws on its parts, together with the lines that use them", () => {
  const cola = product("cola", { quantity: 3 });
  const combo = product("combo", {
    bundle: true,
    track_stock: false,
    quantity: 3,
    components: [{ product_id: "cola", name: "cola", qty: 2, active: true }],
  });
  const by = index(cola, combo);
  const one = step([{ productId: "cola", qty: 2 }], combo, 1, by);
  expect(one.refusal).toEqual({ name: "cola", left: 3 });
  expect(step([{ productId: "cola", qty: 1 }], combo, 1, by).refusal).toBeNull();
});

test("count and total sum the lines; a vanished product adds nothing to the total", () => {
  const by = index(product("a", { price_minor: 250 }));
  const cart = [
    { productId: "a", qty: 2 },
    { productId: "gone", qty: 1 },
  ];
  expect(cartCount(cart)).toBe(3);
  expect(cartTotal(cart, by)).toBe(500);
  expect(withQty(cart, "a", 5)[0]).toEqual({ productId: "a", qty: 5 });
});

test("a burst of fast keys ending in Enter is a scan; slow or short ones are not", () => {
  const burst = new ScanBurst();
  let t = 1000;
  for (const key of "8901") burst.key(key, (t += 10));
  expect(burst.key("Enter", (t += 10))).toBe("8901");
  // Typed by a person.
  for (const key of "8901") burst.key(key, (t += 200));
  expect(burst.key("Enter", (t += 10))).toBeNull();
  // Too short.
  for (const key of "12") burst.key(key, (t += 10));
  expect(burst.key("Enter", (t += 10))).toBeNull();
  // Enter long after the last key.
  for (const key of "8901") burst.key(key, (t += 10));
  expect(burst.key("Enter", (t += 500))).toBeNull();
  // A modifier in the middle does not break the burst.
  burst.key("1", (t += 1000));
  burst.key("Shift", (t += 5));
  burst.key("2", (t += 5));
  burst.key("3", (t += 5));
  expect(burst.key("Enter", (t += 5))).toBe("123");
});

test("a line whose stock dropped under it reports the shortage; a line within stock does not", () => {
  const cola = product("cola", { quantity: 1 });
  const cart = [{ productId: "cola", qty: 3 }];
  expect(lineShortage(cart, cola, index(cola))).toEqual({ name: "cola", left: 1 });
  const ok = product("cola", { quantity: 3 });
  expect(lineShortage(cart, ok, index(ok))).toBeNull();
});

test("keys exactly the gap apart are typing, not a scan", () => {
  const burst = new ScanBurst();
  let t = 1000;
  for (const key of "8901") burst.key(key, (t += SCAN_GAP_MS));
  expect(burst.key("Enter", (t += SCAN_GAP_MS))).toBeNull();
});

// ---- Story 5.11: the tax port and the sale lines ----

const rate = (rate_bp: number, amount_minor: number) => ({ rate_bp, amount_minor });

test("tax inside the price is carved out; the golden bill matches the Hub", () => {
  const bill = computeBill([rate(500, 31_000)], true);
  expect([bill.subtotal_minor, bill.tax_minor, bill.total_minor]).toEqual([29_524, 1_476, 31_000]);
  expect(bill.breakup).toEqual([{ rate_bp: 500, taxable_minor: 29_524, tax_minor: 1_476 }]);
});

test("tax outside the price is added after the subtotal", () => {
  const bill = computeBill([rate(500, 31_000)], false);
  expect([bill.subtotal_minor, bill.tax_minor, bill.total_minor]).toEqual([31_000, 1_550, 32_550]);
});

test("tax is computed once per rate, never per line", () => {
  const lines = [rate(500, 110), rate(500, 110), rate(500, 110), rate(1800, 100), rate(1800, 100)];
  const bill = computeBill(lines, true);
  expect(bill.breakup.map((r) => [r.rate_bp, r.tax_minor])).toEqual([
    [500, 16],
    [1800, 31],
  ]);
  const perLine = lines.reduce((n, l) => n + computeBill([l], true).tax_minor, 0);
  expect(perLine).toBe(15 + 30);
});

test("half a minor unit rounds up, and an empty bill is zero", () => {
  expect(computeBill([rate(500, 1_000)], false).tax_minor).toBe(50);
  expect(computeBill([rate(100, 1_050)], false).tax_minor).toBe(11);
  expect(computeBill([rate(50, 10)], false).tax_minor).toBe(0);
  expect(computeBill([rate(5000, 1)], false).tax_minor).toBe(1);
  expect(computeBill([], true)).toEqual({
    subtotal_minor: 0,
    tax_minor: 0,
    total_minor: 0,
    breakup: [],
  });
});

test("large lines keep every minor unit", () => {
  const amount = 999 * 1_000_000_000;
  const bill = computeBill([rate(1800, amount)], false);
  const exact = (BigInt(amount) * 1800n + 5000n) / 10_000n;
  expect(BigInt(bill.tax_minor)).toBe(exact);
  expect(bill.total_minor).toBe(amount + bill.tax_minor);
});

test("GST mode taxes each line at its product's rate, simple mode at the branch rate", () => {
  const cola = product("Cola", { price_minor: 1_180, tax_rate_bp: 1800 });
  const chips = product("Chips", { price_minor: 105, tax_rate_bp: 500 });
  const byId = index(cola, chips);
  const cart = [
    { productId: "Cola", qty: 1 },
    { productId: "Chips", qty: 1 },
  ];
  const gst = { ...DEFAULT_TAX, gst: true };
  expect(lineRate(cola, gst)).toBe(1800);
  const bill = cartBill(cart, byId, gst);
  expect([bill.subtotal_minor, bill.tax_minor, bill.total_minor]).toEqual([1_100, 185, 1_285]);
  expect(bill.breakup.map((r) => r.rate_bp)).toEqual([500, 1800]);
  // Outside India a "gst" flag means nothing: the branch's simple rate applies.
  const simple = { ...DEFAULT_TAX, country: "AE", gst: true, rate_bp: 700, inclusive: false };
  expect(lineRate(cola, simple)).toBe(700);
  expect(cartTotal(cart, byId, simple)).toBe(1_285 + 90);
  // Before the branch tax loads the cart is priced as listed.
  expect(cartTotal(cart, byId)).toBe(1_285);
});

test("a cart becomes the lines of pos.sale", () => {
  expect(
    saleLines([
      { productId: "A", qty: 2 },
      { productId: "B", qty: 1 },
    ]),
  ).toEqual([
    { product_id: "A", qty: 2 },
    { product_id: "B", qty: 1 },
  ]);
});

// Attaching the cart to a running session (Story 5.12).
import type { SnapshotDevice, SnapshotSession } from "@arena/ui/generated";
import { cartAmount, orderSessions, takesOrder } from "../src/lib/pos-session";

const session = (id: string, extra: Partial<SnapshotSession> = {}): SnapshotSession => ({
  id,
  device_id: `d-${id}`,
  state: "active",
  started_at: 1,
  rev: 1,
  paused_total_ms: 0,
  mode: "postpaid",
  ...extra,
});

test("only a running postpaid session outside a group bill, or a block session, takes an order", () => {
  expect(takesOrder(session("a"))).toBe(true);
  expect(takesOrder(session("a", { state: "paused" }))).toBe(true);
  expect(takesOrder(session("a", { mode: null }))).toBe(true);
  expect(takesOrder(session("a", { mode: "block" }))).toBe(true);
  expect(takesOrder(session("a", { group_bill_id: "G" }))).toBe(false);
  for (const mode of ["pack", "pass", "wallet"]) {
    expect(takesOrder(session("a", { mode }))).toBe(false);
  }
  for (const state of ["ended", "expired", "orphaned"]) {
    expect(takesOrder(session("a", { state }))).toBe(false);
  }
});

test("order sessions are listed in start order with the player and the PC name", () => {
  const devices = [
    { device_id: "d-b", label: "PC 2" },
    { device_id: "d-a", label: "PC 1" },
  ] as SnapshotDevice[];
  const list = orderSessions(
    [
      session("b", { started_at: 20, username: "asha" }),
      session("a", { started_at: 10, guest_no: 3 }),
      session("c", { mode: "wallet" }),
    ],
    devices,
  );
  expect(list.map((s) => [s.session.id, s.player, s.device])).toEqual([
    ["a", null, "PC 1"],
    ["b", "asha", "PC 2"],
  ]);
});

test("an order adds each line at the product price with no tax", () => {
  const cola = product("cola", { price_minor: 4000 });
  const chips = product("chips", { price_minor: 1000 });
  expect(
    cartAmount(
      [
        { productId: "cola", qty: 2 },
        { productId: "chips", qty: 3 },
        { productId: "gone", qty: 9 },
      ],
      index(cola, chips),
    ),
  ).toBe(11000);
});

test("cart lines of one product are apart by kitchen notes and merge when the notes match", () => {
  const tea = product("tea", { price_minor: 4000, kitchen: true, kitchen_notes: ["No ice"] });
  const byId = index(tea);
  const one = step([], tea, 1, byId, ["No ice"]);
  const two = step(one.cart, tea, 1, byId);
  expect(two.cart).toEqual([
    { productId: "tea", qty: 1, notes: ["No ice"] },
    { productId: "tea", qty: 1 },
  ]);
  expect(lineKey(two.cart[0])).not.toBe(lineKey(two.cart[1]));
  expect(saleLines(two.cart)).toEqual([
    { product_id: "tea", qty: 1, kitchen_notes: ["No ice"] },
    { product_id: "tea", qty: 1 },
  ]);
  // No kitchen on the plan: notes stay out of the intent.
  expect(saleLines(two.cart, false)).toEqual([
    { product_id: "tea", qty: 1 },
    { product_id: "tea", qty: 1 },
  ]);
  expect(renote(two.cart, "tea", [], ["No ice"])).toEqual([
    { productId: "tea", qty: 2, notes: ["No ice"] },
  ]);
  expect(withQty(two.cart, "tea", 0, ["No ice"])).toEqual([{ productId: "tea", qty: 1 }]);
  expect(normNotes([" b ", "a", "b", ""])).toEqual(["a", "b"]);
});
