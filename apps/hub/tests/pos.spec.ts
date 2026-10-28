import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Locator, type Page, type Route, type TestInfo } from "@playwright/test";
import type { PlayerHit, Product } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
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
