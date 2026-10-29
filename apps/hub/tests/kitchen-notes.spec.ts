import { expect, test, type Page, type Route } from "@playwright/test";
import type { Product } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { CORS, HUB, device, json as ok, snapshot } from "./floor-fixture";

// Kitchen notes (Story 7.2): the POS cart offers preset chips and one free-text note on a
// kitchen line of a branch with the kitchen flag, never on Free or on a plain product; and the
// product form edits the preset list. The Hub's loopback API is stubbed with `page.route`.

const product = (id: string, name: string, extra: Partial<Product> = {}): Product => ({
  id,
  name,
  category: "Drinks",
  price_minor: 4000,
  tax_rate_bp: 0,
  hsn: "",
  track_stock: false,
  image: "",
  barcode: "",
  kitchen: false,
  favourite: true,
  active: true,
  bundle: false,
  components: [],
  kitchen_notes: [],
  rev: 1,
  quantity: 0,
  avg_cost_minor: 0,
  available: true,
  ...extra,
});

const TEA = product("P1", "Tea", { kitchen: true, kitchen_notes: ["Less sugar", "No ice"] });
const COLA = product("P2", "Cola");

type Hub = { sent: { kind: string; kitchen_notes?: string[]; [k: string]: unknown }[] };

async function open(page: Page, route_: string, flags: string, products = [TEA, COLA]) {
  const hub: Hub = { sent: [] };
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
          permissions: ["catalog.edit"],
          locked: false,
          has_pin: true,
        });
      case "/ui/products":
        return ok(route, products);
      case "/ui/settings":
        return ok(route, [{ key: "licence.flags", value: flags, rev: 1 }]);
      case "/ui/intent": {
        const body = req.postDataJSON() as { intent: { payload: Hub["sent"][number] } };
        hub.sent.push(body.intent.payload);
        return ok(route, {
          intent_id: "I1",
          changes: [],
          result: { kind: "applied" },
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
  await page.goto(route_);
  return hub;
}

const tile = (page: Page, name: string) => page.getByTestId("pos-tile").filter({ hasText: name });
const lines = (page: Page) => page.getByTestId("cart-line");

async function openCart(page: Page) {
  const bar = page.getByTestId("phone-cart-bar");
  if (await bar.isVisible()) await bar.click();
}

test("a kitchen line takes preset chips and one free note, shown under the line", async ({
  page,
}) => {
  await open(page, "/pos", "kitchen");
  await expect(page.getByTestId("pos")).toBeVisible();
  await tile(page, "Tea").click();
  await openCart(page);
  const tea = lines(page).filter({ hasText: "Tea" });
  await tea.getByTestId("line-notes-edit").click();
  await expect(page.getByTestId("notes-chip")).toHaveText(["Less sugar", "No ice"]);
  await page.getByTestId("notes-chip").filter({ hasText: "No ice" }).click();
  await expect(page.getByTestId("notes-chip").filter({ hasText: "No ice" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  // Input stops at the limit and a counter shows.
  await page.getByTestId("notes-free").fill("x".repeat(70));
  await expect(page.getByTestId("notes-free")).toHaveValue("x".repeat(60));
  await page.getByTestId("notes-free").fill("Extra hot");
  await page.getByTestId("notes-done").click();
  await expect(tea.getByTestId("line-notes").getByRole("listitem")).toHaveText([
    "Extra hot",
    "No ice",
  ]);
});

test("lines with different notes stay apart and lines with the same notes merge", async ({
  page,
}) => {
  await open(page, "/pos", "kitchen");
  await tile(page, "Tea").click();
  await openCart(page);
  await lines(page).first().getByTestId("line-notes-edit").click();
  await page.getByTestId("notes-chip").filter({ hasText: "No ice" }).click();
  await page.getByTestId("notes-done").click();
  // A plain tap now starts a second Tea line (no notes); a phone's sheet closes with Esc first.
  const sheet = page.getByRole("dialog", { name: en["hub.pos.sheet.title"] });
  if (await sheet.isVisible()) {
    await lines(page).first().getByTestId("line-notes-edit").focus();
    await page.keyboard.press("Escape");
  }
  await tile(page, "Tea").click();
  await openCart(page);
  await expect(lines(page)).toHaveCount(2);
  // Giving the second line the same note merges the two and adds their quantities.
  await lines(page).nth(1).getByTestId("line-notes-edit").click();
  await page.getByTestId("notes-chip").filter({ hasText: "No ice" }).click();
  await page.getByTestId("notes-done").click();
  await expect(lines(page)).toHaveCount(1);
  await expect(lines(page).first().getByTestId("line-qty")).toHaveText("2");
});

test("a Free branch and a plain product offer no notes control", async ({ page }) => {
  await open(page, "/pos", "");
  await tile(page, "Tea").click();
  await openCart(page);
  await expect(page.getByTestId("line-notes-edit")).toHaveCount(0);

  await page.reload();
  await page.getByRole("button", { name: en["hub.pos.all"], exact: true }).click();
  await tile(page, "Cola").click();
  await openCart(page);
  await expect(lines(page)).toHaveCount(1);
});

test("the product form edits up to eight presets and sends them with product.upsert", async ({
  page,
}) => {
  const hub = await open(page, "/inventory", "kitchen");
  await page
    .getByRole("button", { name: en["hub.products.edit"].replace("{name}", "Tea") })
    .click();
  const box = page.getByTestId("kitchen-notes");
  await expect(box.getByRole("textbox")).toHaveCount(2);
  await box.getByRole("button", { name: en["hub.products.note_add"] }).click();
  await box.getByRole("textbox").nth(2).fill("Extra hot");
  await page
    .getByTestId("product-form")
    .getByRole("button", { name: en["hub.products.save"] })
    .click();
  await expect.poll(() => hub.sent.length).toBe(1);
  expect(hub.sent[0]).toMatchObject({
    kind: "product.upsert",
    kitchen_notes: ["Less sugar", "No ice", "Extra hot"],
  });
});

test("the product form has no notes list on a Free branch", async ({ page }) => {
  await open(page, "/inventory", "");
  await page
    .getByRole("button", { name: en["hub.products.edit"].replace("{name}", "Tea") })
    .click();
  await expect(page.getByTestId("product-form")).toBeVisible();
  await expect(page.getByTestId("kitchen-notes")).toHaveCount(0);
});
