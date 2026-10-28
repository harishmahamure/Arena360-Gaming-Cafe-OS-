import { expect, test } from "@playwright/test";
import type { Product } from "@arena/ui/generated";
import {
  ALL,
  FAVOURITES,
  MAX_LINE_QTY,
  SCAN_GAP_MS,
  ScanBurst,
  cartCount,
  cartTotal,
  findByBarcode,
  lineShortage,
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
