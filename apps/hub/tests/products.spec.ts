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
    permissions: ["catalog.edit"],
  },
  "counter-token": {
    staff_id: "01JCOUNTER0000000000000000",
    display_name: "Counter",
    role_name: "Counter",
    permissions: ["session.start"],
  },
};
const GST_TAX = {
  country: "IN",
  gst: true,
  gstin: "27AAPFU0939F1ZV",
  inclusive: true,
  product_bp: 500,
  play_bp: 1800,
  sac: "",
  label: "Tax",
  rate_bp: 0,
};

const Buf = (globalThis as unknown as { Buffer: { from: (v: string | number[]) => never } }).Buffer;

type Payload = Record<string, unknown> & { kind: string };
type Hub = {
  products: Product[];
  events: string[];
  intents: { payload: Payload; base_rev?: number; ticket?: string }[];
  fail: { status: number; code: string; field?: string } | null;
  elevations: { items: { intent_kind: string }[]; pin: string }[];
};

const newHub = (): Hub => ({ products: [], events: [], intents: [], fail: null, elevations: [] });

const product = (over: Partial<Product>): Product => ({
  id: "P1".padEnd(26, "0"),
  name: "Cola",
  category: "Drinks",
  price_minor: 2500,
  tax_rate_bp: 500,
  hsn: "2202",
  track_stock: true,
  image: "",
  barcode: "890",
  kitchen: false,
  favourite: false,
  active: true,
  bundle: false,
  components: [],
  kitchen_notes: [],
  rev: 3,
  quantity: 0,
  avg_cost_minor: 0,
  available: false,
  reason: "out_of_stock",
  ...over,
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

function apply(hub: Hub, p: Payload, route: Route) {
  if (p.kind === "product.delete") {
    hub.products = hub.products.filter((x) => x.id !== p.id);
  } else {
    const row = {
      name: p.name as string,
      category: p.category as string,
      price_minor: p.price_minor as number,
      tax_rate_bp: (p.tax_rate_bp as number | undefined) ?? 500,
      hsn: p.hsn as string,
      track_stock: p.track_stock as boolean,
      image: p.image as string,
      barcode: p.barcode as string,
      kitchen: p.kitchen as boolean,
      favourite: p.favourite as boolean,
      active: p.active as boolean,
      bundle: p.bundle as boolean,
      components: (p.components as { product_id: string; qty: number }[]).map((c) => ({
        product_id: c.product_id,
        qty: c.qty,
        name: hub.products.find((x) => x.id === c.product_id)?.name ?? c.product_id,
        active: hub.products.find((x) => x.id === c.product_id)?.active ?? false,
      })),
    };
    const old = hub.products.find((x) => x.id === p.id);
    if (old) Object.assign(old, row, { rev: old.rev + 1 });
    else
      hub.products.push(
        product({ ...row, id: `P${hub.products.length + 2}`.padEnd(26, "0"), rev: 1 }),
      );
  }
  return json(route, 200, { applied: { intent_id: "I", changes: [] } });
}

async function stub(page: Page, hub: Hub, token: string) {
  await page.addInitScript((t) => sessionStorage.setItem("arena.hub.token", t), token);
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    const url = new URL(req.url());
    const auth = (req.headers()["authorization"] ?? "").replace(/^Bearer /, "");
    switch (url.pathname) {
      case "/ui/snapshot":
        return json(route, 200, snapshot());
      case "/ui/me":
        return STAFF[auth]
          ? json(route, 200, STAFF[auth])
          : json(route, 401, { code: "AUTH_FAILED", message: "no" });
      case "/ui/settings":
        return json(route, 200, [{ key: "branch.tax", value: JSON.stringify(GST_TAX), rev: 2 }]);
      case "/ui/products":
        return json(route, 200, hub.products);
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: `retry: 300\n\n` + hub.events.splice(0).join(""),
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
          intent: { base_rev?: number; payload: Payload };
          elevation_ticket?: string;
        };
        const { payload, base_rev } = body.intent;
        hub.intents.push({ payload, base_rev, ticket: body.elevation_ticket });
        if (!body.elevation_ticket && !STAFF[auth].permissions.includes("catalog.edit")) {
          return json(route, 403, { code: "PERMISSION_DENIED", message: "denied" });
        }
        if (hub.fail) return json(route, hub.fail.status, { message: "no", ...hub.fail });
        return apply(hub, payload, route);
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
}

async function open(page: Page, hub: Hub, token = "manager-token") {
  await stub(page, hub, token);
  await page.goto("/inventory");
  await expect(page.getByTestId("product-form")).toBeVisible();
}

async function expectNoSeriousAxe(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  expect(bad.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual(
    [],
  );
}

const fill = async (page: Page, name: string, price: string) => {
  await page.getByLabel(en["hub.products.name"]).fill(name);
  await page.getByLabel(en["hub.products.price"]).fill(price);
};
const add = (page: Page) => page.getByRole("button", { name: en["hub.products.add"] }).click();

test("a manager adds a product and the list shows it", async ({ page }) => {
  const hub = newHub();
  await open(page, hub);
  await expect(page.getByText(en["hub.products.tax_hint"].replace("{rate}", "5"))).toBeVisible();
  await fill(page, "Cola", "120.50");
  await page.getByLabel(en["hub.products.category"]).fill("Drinks");
  await page.getByLabel(en["hub.products.hsn"]).fill("2202");
  await page.getByLabel(en["hub.products.barcode"]).fill("890");
  await page.getByLabel(en["hub.products.kitchen"]).check();
  await expectNoSeriousAxe(page);
  await add(page);
  await expect(page.getByTestId("product-status")).toContainText(en["hub.settings.saved"]);
  expect(hub.intents[0]).toEqual({
    payload: {
      kind: "product.upsert",
      name: "Cola",
      category: "Drinks",
      price_minor: 12050,
      hsn: "2202",
      track_stock: true,
      image: "",
      barcode: "890",
      kitchen: true,
      favourite: false,
      active: true,
      bundle: false,
      components: [],
    },
    base_rev: undefined,
    ticket: undefined,
  });
  const row = page.getByTestId("product-row");
  await expect(row).toContainText("Cola");
  await expect(row).toContainText("₹120.50");
  await expect(row).toContainText(en["hub.products.badge.kitchen"]);
  await expect(row).toContainText(en["hub.products.badge.out_of_stock"]);
  await expectNoSeriousAxe(page);
});

test("a bad price or rate is refused before anything is sent", async ({ page }) => {
  const hub = newHub();
  await open(page, hub);
  await fill(page, "Cola", "12.345");
  await add(page);
  await expect(page.getByText(en["hub.products.invalid.price"])).toBeVisible();
  await fill(page, "Cola", "12");
  await page.getByLabel(en["hub.products.tax"]).fill("101");
  await add(page);
  await expect(page.getByText(en["hub.products.invalid.tax"])).toBeVisible();
  expect(hub.intents).toEqual([]);
});

test("server refusals name the field", async ({ page }) => {
  const hub = newHub();
  await open(page, hub);
  await fill(page, "Cola", "12");
  hub.fail = { status: 422, code: "HSN_REQUIRED", field: "hsn" };
  await add(page);
  await expect(page.getByText(en["hub.products.invalid.hsn"])).toBeVisible();
  hub.fail = { status: 409, code: "BARCODE_TAKEN", field: "barcode" };
  await add(page);
  await expect(page.getByText(en["hub.products.barcode_taken"])).toBeVisible();
  await expect(page.getByLabel(en["hub.products.barcode"])).toHaveAttribute("aria-invalid", "true");
  expect(hub.products).toEqual([]);
});

test("an image is shrunk to a small JPEG before it is sent", async ({ page }) => {
  const hub = newHub();
  await open(page, hub);
  const png = await page.evaluate(async () => {
    const c = document.createElement("canvas");
    c.width = 1024;
    c.height = 768;
    const ctx = c.getContext("2d")!;
    ctx.fillStyle = "#c33";
    ctx.fillRect(0, 0, 1024, 768);
    const blob = await new Promise<Blob>((r) => c.toBlob((b) => r(b!), "image/png"));
    return Array.from(new Uint8Array(await blob.arrayBuffer()));
  });
  await page
    .getByLabel(en["hub.products.image"])
    .setInputFiles({ name: "big.png", mimeType: "image/png", buffer: Buf.from(png) });
  const preview = page.getByTestId("product-image");
  await expect(preview).toBeVisible();
  const size = await preview.evaluate((img: HTMLImageElement) => ({
    w: img.naturalWidth,
    h: img.naturalHeight,
    bytes: Math.floor((img.src.length - img.src.indexOf(",") - 1) * 0.75),
    jpeg: img.src.startsWith("data:image/jpeg;base64,"),
  }));
  expect(size.jpeg).toBe(true);
  expect(Math.max(size.w, size.h)).toBeLessThanOrEqual(512);
  expect(size.bytes).toBeLessThanOrEqual(200 * 1024);
  await fill(page, "Cola", "12");
  await add(page);
  expect((hub.intents[0].payload.image as string).startsWith("data:image/jpeg;base64,")).toBe(true);
  await page
    .getByRole("button", { name: en["hub.products.edit"].replace("{name}", "Cola") })
    .click();
  await page.getByRole("button", { name: en["hub.products.image_remove"] }).click();
  await expect(preview).toHaveCount(0);
});

test("a large noisy picture still ends within 512 px and 200 KB, and a portrait keeps its shape", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub);
  await page.evaluate(async () => {
    const c = document.createElement("canvas");
    c.width = 1200;
    c.height = 1800;
    const ctx = c.getContext("2d")!;
    const data = ctx.createImageData(c.width, c.height);
    for (let i = 0; i < data.data.length; i++)
      data.data[i] = i % 4 === 3 ? 255 : Math.random() * 256;
    ctx.putImageData(data, 0, 0);
    const blob = await new Promise<Blob>((r) => c.toBlob((b) => r(b!), "image/png"));
    const transfer = new DataTransfer();
    transfer.items.add(new File([blob], "noise.png", { type: "image/png" }));
    const input = document.getElementById("product-image") as HTMLInputElement;
    input.files = transfer.files;
    input.dispatchEvent(new Event("change", { bubbles: true }));
  });
  const preview = page.getByTestId("product-image");
  await expect(preview).toBeVisible();
  const size = await preview.evaluate((img: HTMLImageElement) => ({
    w: img.naturalWidth,
    h: img.naturalHeight,
    bytes: Math.floor((img.src.length - img.src.indexOf(",") - 1) * 0.75),
  }));
  expect(size.h).toBe(512);
  expect(size.w).toBe(341);
  expect(size.bytes).toBeLessThanOrEqual(200 * 1024);
});

test("a broken picture file shows the inline error and keeps no image", async ({ page }) => {
  const hub = newHub();
  await open(page, hub);
  await page
    .getByLabel(en["hub.products.image"])
    .setInputFiles({ name: "broken.png", mimeType: "image/png", buffer: Buf.from("not a png") });
  await expect(page.getByText(en["hub.products.invalid.image"])).toBeVisible();
  await expect(page.getByTestId("product-image")).toHaveCount(0);
});

test("a file that is not a picture shows an inline error and no image", async ({ page }) => {
  const hub = newHub();
  await open(page, hub);
  await page
    .getByLabel(en["hub.products.image"])
    .setInputFiles({ name: "notes.txt", mimeType: "text/plain", buffer: Buf.from("hello") });
  await expect(page.getByText(en["hub.products.invalid.image"])).toBeVisible();
  await expect(page.getByTestId("product-image")).toHaveCount(0);
  await expectNoSeriousAxe(page);
});

test("editing sends base_rev, delete sends product.delete", async ({ page }) => {
  const hub = newHub();
  hub.products.push(product({}));
  await open(page, hub);
  await page
    .getByRole("button", { name: en["hub.products.edit"].replace("{name}", "Cola") })
    .click();
  await page.getByLabel(en["hub.products.name"]).fill("Cola 2");
  await page.getByRole("button", { name: en["hub.products.save"] }).click();
  await expect(page.getByTestId("product-row")).toContainText("Cola 2");
  expect(hub.intents.at(-1)).toMatchObject({
    payload: { kind: "product.upsert", id: "P1".padEnd(26, "0"), name: "Cola 2" },
    base_rev: 3,
  });
  await page
    .getByRole("button", { name: en["hub.products.delete"].replace("{name}", "Cola 2") })
    .click();
  await expect(page.getByTestId("product-list")).toHaveCount(0);
  expect(hub.intents.at(-1)).toMatchObject({
    payload: { kind: "product.delete", id: "P1".padEnd(26, "0") },
    base_rev: 4,
  });
});

test("a stale edit reloads the product and the retry carries the new rev", async ({ page }) => {
  const hub = newHub();
  hub.products.push(product({}));
  await open(page, hub);
  await page
    .getByRole("button", { name: en["hub.products.edit"].replace("{name}", "Cola") })
    .click();
  hub.products[0] = product({ name: "Cola Zero", rev: 9 });
  hub.fail = { status: 409, code: "STALE", field: "base_rev" };
  await page.getByLabel(en["hub.products.price"]).fill("30");
  await page.getByRole("button", { name: en["hub.products.save"] }).click();
  await expect(page.getByTestId("product-status")).toContainText(en["hub.products.stale"]);
  await expect(page.getByTestId("product-row")).toContainText("Cola Zero");
  await expect(page.getByLabel(en["hub.products.name"])).toHaveValue("Cola Zero");
  hub.fail = null;
  await page.getByRole("button", { name: en["hub.products.save"] }).click();
  await expect(page.getByTestId("product-status")).toContainText(en["hub.settings.saved"]);
  expect(hub.intents.at(-1)).toMatchObject({ base_rev: 9 });
});

test("the category filter, badges, and a catalog.changed push", async ({ page }) => {
  const hub = newHub();
  hub.products.push(
    product({ name: "Cola", category: "Drinks" }),
    product({
      id: "P2".padEnd(26, "0"),
      name: "Burger",
      category: "Food",
      active: false,
      favourite: true,
      reason: "inactive",
    }),
  );
  await open(page, hub);
  await expect(page.getByTestId("product-row")).toHaveCount(2);
  const burger = page.getByTestId("product-row").filter({ hasText: "Burger" });
  await expect(burger).toContainText(en["hub.products.badge.inactive"]);
  await expect(burger).toContainText(en["hub.products.badge.favourite"]);
  await page.getByLabel(en["hub.products.filter"]).selectOption("Food");
  await expect(page.getByTestId("product-row")).toHaveCount(1);
  await expect(page.getByTestId("product-row")).toContainText("Burger");
  hub.products.push(product({ id: "P3".padEnd(26, "0"), name: "Fries", category: "Food" }));
  hub.events.push('event: catalog.changed\ndata: {"products":[]}\n\n');
  await expect(page.getByTestId("product-row")).toHaveCount(2, { timeout: 10_000 });
  await expect(page.getByTestId("product-row").filter({ hasText: "Fries" })).toBeVisible();
});

test("a rate typed in percent is sent in basis points", async ({ page }) => {
  const hub = newHub();
  await open(page, hub);
  await fill(page, "Cola", "12");
  await page.getByLabel(en["hub.products.tax"]).fill("12.5");
  await add(page);
  await expect(page.getByTestId("product-status")).toContainText(en["hub.settings.saved"]);
  expect(hub.intents[0].payload.tax_rate_bp).toBe(1250);
});

test("a Counter's edit asks for a manager's PIN in place", async ({ page }) => {
  const hub = newHub();
  await open(page, hub, "counter-token");
  await fill(page, "Cola", "12");
  await add(page);
  await expect(page.getByTestId("product-approval")).toBeVisible();
  expect(hub.intents).toEqual([]);
  await page.keyboard.type("2222");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("product-status")).toContainText(en["hub.settings.saved"]);
  expect(hub.elevations.at(-1)).toEqual({
    items: [{ intent_kind: "product.upsert" }],
    pin: "2222",
  });
  expect(hub.intents[0].ticket).toBe("T1");
});

const bundleRow = (id: string, over: Partial<Product> = {}) =>
  product({
    id: id.padEnd(26, "0"),
    name: "Combo",
    bundle: true,
    track_stock: false,
    price_minor: 5000,
    quantity: 0,
    available: true,
    reason: undefined,
    components: [
      { product_id: "P1".padEnd(26, "0"), name: "Cola", qty: 2, active: true },
      { product_id: "P2".padEnd(26, "0"), name: "Burger", qty: 1, active: true },
    ],
    ...over,
  });

test("a manager builds a bundle from components with quantities", async ({ page }) => {
  const hub = newHub();
  hub.products = [
    product({ id: "P1".padEnd(26, "0"), name: "Cola" }),
    product({ id: "P2".padEnd(26, "0"), name: "Burger" }),
  ];
  await open(page, hub);
  await fill(page, "Combo", "50");
  await page.getByLabel(en["hub.products.bundle"]).check();
  await expect(page.getByLabel(en["hub.products.track"])).toHaveCount(0);
  const add1 = page.getByRole("button", { name: en["hub.products.component_add"] });
  await add1.click();
  await add1.click();
  await page
    .getByLabel(en["hub.products.component"].replace("{n}", "1"), { exact: true })
    .selectOption({ label: "Cola" });
  await page.getByLabel(en["hub.products.component_qty"].replace("{n}", "1")).fill("2");
  await page
    .getByLabel(en["hub.products.component"].replace("{n}", "2"), { exact: true })
    .selectOption({ label: "Burger" });
  await expectNoSeriousAxe(page);
  await add(page);
  await expect(page.getByTestId("product-status")).toContainText(en["hub.settings.saved"]);
  expect(hub.intents[0].payload).toMatchObject({
    kind: "product.upsert",
    price_minor: 5000,
    track_stock: false,
    bundle: true,
    components: [
      { product_id: "P1".padEnd(26, "0"), qty: 2 },
      { product_id: "P2".padEnd(26, "0"), qty: 1 },
    ],
  });
  const row = page.getByTestId("product-row").filter({ hasText: "Combo" });
  await expect(row).toContainText(en["hub.products.badge.bundle"]);
  await expect(row).toContainText("2 × Cola, 1 × Burger");
  await expectNoSeriousAxe(page);
});

test("an empty or non-numeric component is refused before anything is sent", async ({ page }) => {
  const hub = newHub();
  hub.products = [product({ id: "P1".padEnd(26, "0"), name: "Cola" })];
  await open(page, hub);
  await fill(page, "Combo", "50");
  await page.getByLabel(en["hub.products.bundle"]).check();
  await add(page);
  await page.getByRole("button", { name: en["hub.products.component_add"] }).click();
  await add(page);
  await expect(page.getByText(en["hub.products.invalid.components"])).toBeVisible();
  await page
    .getByLabel(en["hub.products.component"].replace("{n}", "1"), { exact: true })
    .selectOption({ label: "Cola" });
  await page.getByLabel(en["hub.products.component_qty"].replace("{n}", "1")).fill("0");
  await add(page);
  await expect(page.getByText(en["hub.products.invalid.components"])).toBeVisible();
  expect(hub.intents).toEqual([]);
  hub.fail = { status: 422, code: "VALIDATION_FAILED", field: "components" };
  await page.getByLabel(en["hub.products.component_qty"].replace("{n}", "1")).fill("1");
  await add(page);
  await expect(page.getByText(en["hub.products.invalid.components"])).toBeVisible();
});

test("a bundle with an inactive component says which and is not available", async ({ page }) => {
  const hub = newHub();
  hub.products = [
    bundleRow("B1", {
      available: false,
      reason: "component_inactive",
      components: [
        { product_id: "P1".padEnd(26, "0"), name: "Cola", qty: 2, active: false },
        { product_id: "P2".padEnd(26, "0"), name: "Burger", qty: 1, active: true },
      ],
    }),
  ];
  await open(page, hub);
  const row = page.getByTestId("product-row");
  await expect(row).toContainText(en["hub.products.component_inactive"].replace("{names}", "Cola"));
  await row
    .getByRole("button", { name: en["hub.products.edit"].replace("{name}", "Combo") })
    .click();
  await expect(page.getByLabel(en["hub.products.bundle"])).toBeChecked();
  await expect(page.getByLabel(en["hub.products.component_qty"].replace("{n}", "2"))).toHaveValue(
    "1",
  );
  await expectNoSeriousAxe(page);
});

test("editing a bundle sends the replaced components and its rev", async ({ page }) => {
  const hub = newHub();
  hub.products = [
    product({ id: "P1".padEnd(26, "0"), name: "Cola" }),
    product({ id: "P2".padEnd(26, "0"), name: "Burger" }),
    bundleRow("B1", { rev: 7 }),
  ];
  await open(page, hub);
  const row = page.getByTestId("product-row").filter({ hasText: "Combo" });
  await row
    .getByRole("button", { name: en["hub.products.edit"].replace("{name}", "Combo") })
    .click();
  const options = await page
    .getByLabel(en["hub.products.component"].replace("{n}", "1"), { exact: true })
    .locator("option")
    .allTextContents();
  expect(options).toEqual([en["hub.products.component_pick"], "Cola", "Burger"]);
  await page.getByLabel(en["hub.products.component_qty"].replace("{n}", "1")).fill(" 3 ");
  await page
    .getByRole("button", { name: en["hub.products.component_remove"].replace("{n}", "2") })
    .click();
  await page.getByRole("button", { name: en["hub.products.save"] }).click();
  await expect(page.getByTestId("product-status")).toContainText(en["hub.settings.saved"]);
  expect(hub.intents[0].base_rev).toBe(7);
  expect(hub.intents[0].payload).toMatchObject({
    bundle: true,
    track_stock: false,
    components: [{ product_id: "P1".padEnd(26, "0"), qty: 3 }],
  });
});

test("unticking Bundle sends no components and brings Track stock back", async ({ page }) => {
  const hub = newHub();
  hub.products = [product({ id: "P1".padEnd(26, "0"), name: "Cola" }), bundleRow("B1")];
  await open(page, hub);
  const row = page.getByTestId("product-row").filter({ hasText: "Combo" });
  await row
    .getByRole("button", { name: en["hub.products.edit"].replace("{name}", "Combo") })
    .click();
  await page.getByLabel(en["hub.products.bundle"]).uncheck();
  await expect(page.getByLabel(en["hub.products.track"])).toBeVisible();
  await page.getByRole("button", { name: en["hub.products.save"] }).click();
  await expect(page.getByTestId("product-status")).toContainText(en["hub.settings.saved"]);
  expect(hub.intents[0].payload).toMatchObject({ bundle: false, components: [] });
});

test("a deleted component stays visible by name so it can be removed, and the bundle error maps", async ({
  page,
}) => {
  const hub = newHub();
  hub.products = [
    product({ id: "P1".padEnd(26, "0"), name: "Cola" }),
    bundleRow("B1", {
      available: false,
      reason: "component_inactive",
      components: [
        { product_id: "GONE".padEnd(26, "0"), name: "Burger", qty: 1, active: false },
        { product_id: "P1".padEnd(26, "0"), name: "Cola", qty: 1, active: true },
      ],
    }),
  ];
  await open(page, hub);
  const row = page.getByTestId("product-row").filter({ hasText: "Combo" });
  await row
    .getByRole("button", { name: en["hub.products.edit"].replace("{name}", "Combo") })
    .click();
  await expect(
    page.getByLabel(en["hub.products.component"].replace("{n}", "1"), { exact: true }),
  ).toHaveValue("GONE".padEnd(26, "0"));
  hub.fail = { status: 422, code: "VALIDATION_FAILED", field: "bundle" };
  await page.getByRole("button", { name: en["hub.products.save"] }).click();
  await expect(page.getByText(en["hub.products.invalid.bundle"])).toBeVisible();
});
