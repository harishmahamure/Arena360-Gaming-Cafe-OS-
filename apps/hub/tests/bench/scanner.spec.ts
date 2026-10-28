import { expect, test } from "@playwright/test";
import type { Product } from "@arena/ui/generated";
import { CORS, HUB, device, json, snapshot } from "../floor-fixture";

// The scanner check of the monthly hardware job (Story 5.39): a real USB keyboard-wedge scanner,
// a scan of the bench sheet's "Cola" barcode (8901) with the POS open, and the product lands in
// the cart. Skipped unless `ARENA_HW_SCANNER` names the scanner model; run it headed on the
// test-café machine so the window has the keyboard focus. The Hub is stubbed: this proves the
// scanner and the POS screen, not the Hub. The result is printed as one `HW_SCANNER {...}` line
// for `scripts/hardware-compat.mjs`.

const env = (globalThis as { process?: { env: Record<string, string | undefined> } }).process?.env;
const SCANNER = env?.ARENA_HW_SCANNER ?? "";
const WAIT_MS = Number(env?.ARENA_HW_SCAN_WAIT_MS ?? 60_000);

const COLA: Product = {
  id: "P1",
  name: "Cola",
  category: "Drinks",
  price_minor: 6000,
  tax_rate_bp: 0,
  hsn: "",
  track_stock: true,
  image: "",
  barcode: "8901",
  kitchen: false,
  favourite: true,
  active: true,
  bundle: false,
  components: [],
  rev: 1,
  quantity: 10,
  avg_cost_minor: 0,
  available: true,
};

test.skip(!SCANNER, "set ARENA_HW_SCANNER to the scanner model to run on the bench");

test("a keyboard-wedge scanner adds the scanned product to the cart", async ({ page }) => {
  await page.addInitScript(() => sessionStorage.setItem("arena.hub.token", "manager-token"));
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot":
        return json(route, snapshot([device(1, {})]));
      case "/ui/me":
        return json(route, {
          staff_id: "01JMANAGER0000000000000000",
          display_name: "Manager",
          role_name: "Manager",
          permissions: [],
          locked: false,
          has_pin: true,
        });
      case "/ui/products":
        return json(route, [COLA]);
      case "/ui/players":
        return json(route, []);
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
  await page.locator("body").click();

  const began = Date.now();
  console.log(`Scan barcode 8901 with the ${SCANNER} within ${WAIT_MS / 1000} s`);
  let ok = true;
  try {
    await expect(page.getByTestId("cart-line").filter({ hasText: "Cola" })).toBeVisible({
      timeout: WAIT_MS,
    });
  } catch {
    ok = false;
  }
  console.log(`HW_SCANNER ${JSON.stringify({ name: SCANNER, ok, took_ms: Date.now() - began })}`);
  expect(ok, "the scanned product reached the cart").toBe(true);
});
