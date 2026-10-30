import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Locator, type Page } from "@playwright/test";
import type { CatalogueRow } from "@arena/ui/generated";
import { PAID, liveWith, stubCloud, t } from "./stub";

const ULID = /^[0-9A-HJKMNP-TV-Z]{26}$/;
const ONE = [PAID[0]];
const TWO = [PAID[0], { ...PAID[0], id: "b3", name: "Pixel Arena Koramangala" }];
const B1 = PAID[0].name;
const B3 = "Pixel Arena Koramangala";

/** A catalogue row the way the Hub publishes it. */
const row = (
  id: string,
  name: string,
  fields: Record<string, unknown> | undefined,
  more: Partial<CatalogueRow> = {},
): CatalogueRow => ({ id, name, rev: 1, active: true, ...(fields ? { fields } : {}), ...more });

const product = (extra: Record<string, unknown>) => ({
  category: "",
  price_minor: 1000,
  tax_rate_bp: 0,
  hsn: "",
  track_stock: false,
  barcode: "",
  kitchen: false,
  favourite: false,
  bundle: false,
  ...extra,
});

const COLA = row(
  "pr1",
  "Cola",
  product({
    category: "Drinks",
    price_minor: 5000,
    tax_rate_bp: 1200,
    hsn: "2202",
    track_stock: true,
    barcode: "890123",
    low_stock_qty: 4,
  }),
  { rev: 3, detail: "Drinks" },
);
const NOODLES = row(
  "pr2",
  "Noodles",
  product({ category: "Food", price_minor: 12_000, tax_rate_bp: 500, kitchen: true }),
  { rev: 2, detail: "Food" },
);
const SOLD_OUT = row(
  "pr3",
  "Chips",
  product({ category: "Snacks", price_minor: 3000, track_stock: true, barcode: "555000" }),
  { rev: 1, active: false, detail: "Snacks" },
);
const COMBO = row(
  "bu1",
  "Combo",
  product({
    category: "Combos",
    price_minor: 15_000,
    tax_rate_bp: 500,
    kitchen: true,
    bundle: true,
    components: [
      { product_id: "pr1", qty: 2 },
      { product_id: "pr2", qty: 1 },
    ],
  }),
  { rev: 5, detail: "Combos" },
);

const B1_CATALOGUE = { products: [COLA, NOODLES, SOLD_OUT], bundles: [COMBO] };
const B3_CATALOGUE = {
  products: [
    row(
      "h1",
      "cola",
      product({
        category: "Drinks",
        price_minor: 5500,
        tax_rate_bp: 1200,
        hsn: "2202",
        track_stock: true,
        barcode: "890123",
      }),
      { rev: 7, detail: "Drinks" },
    ),
  ],
  bundles: [],
};

const preview = (page: Page) => page.getByRole("region", { name: t("owner.cmd.preview.title") });
const sendIn = (where: Locator) =>
  where.getByRole("button", { name: t("owner.cmd.send"), exact: true });
const formSend = (form: Locator) =>
  form.getByRole("button", { name: t("owner.pricing.form.send"), exact: true });
const items = (page: Page, kind: string) =>
  page.locator(`[data-component="pricing-row"][data-kind="${kind}"]`);
const item = (page: Page, kind: string, name: string) =>
  items(page, kind).and(page.locator(`[data-name="${name}"]`));
const posts = (calls: { bodies: Record<string, unknown[]> }) =>
  (calls.bodies["POST /v1/commands"] ?? []) as {
    branch_id: string;
    idempotency_key: string;
    kind: string;
    payload: Record<string, unknown>;
    base_rev?: number;
  }[];

async function open(page: Page, branches = ONE, catalogues = true) {
  const calls = await stubCloud(page, branches, {
    live: catalogues
      ? { b1: liveWith("b1", B1_CATALOGUE), b3: liveWith("b3", B3_CATALOGUE, 600_000) }
      : {},
  });
  await page.goto("/manage?area=products");
  await expect(page.getByRole("heading", { name: t("owner.products.title") })).toBeVisible();
  return calls;
}

test("Manage opens Pricing; Products is its own area in the URL", async ({ page }) => {
  await stubCloud(page, ONE, { live: { b1: liveWith("b1", B1_CATALOGUE) } });
  await page.goto("/manage");
  const areas = page.getByRole("tablist", { name: t("owner.manage.areas") });
  await expect(areas.getByRole("tab", { selected: true })).toHaveText(
    t("owner.manage.area.pricing"),
  );
  await expect(page.getByRole("tablist", { name: t("owner.pricing.tabs") })).toBeVisible();

  await areas.getByRole("tab", { name: t("owner.manage.area.products") }).click();
  await expect(page).toHaveURL(/[?&]area=products/);
  await expect(page).not.toHaveURL(/[?&]tab=/);
  await expect(item(page, "product", "Cola")).toBeVisible();
  await page.reload();
  await expect(areas.getByRole("tab", { selected: true })).toHaveText(
    t("owner.manage.area.products"),
  );
});

test("Products lists products and bundles per branch with category, price, tax, flags and data age", async ({
  page,
}) => {
  await open(page, TWO);
  await expect(page.getByText(`${B1} · Updated 2 min ago`)).toBeVisible();
  await expect(page.getByText(`${B3} · Updated 10 min ago`)).toBeVisible();

  const cola = item(page, "product", "Cola");
  await expect(cola).toContainText("Drinks");
  await expect(cola).toContainText("890123");
  await expect(cola).toContainText(`${B1}: ₹50.00 · 12%`);
  await expect(cola).toContainText(`${B3}: ₹55.00 · 12%`);
  await expect(cola).toContainText(t("owner.products.threshold_value", { qty: 4 }));
  await expect(
    item(page, "product", "Noodles").getByText(t("hub.products.badge.kitchen")),
  ).toBeVisible();
  await expect(
    item(page, "product", "Chips").getByText(t("hub.products.badge.inactive")),
  ).toBeVisible();

  const combo = item(page, "bundle", "Combo");
  await expect(combo).toContainText(
    t("hub.products.component_list", {
      items: `${t("hub.products.component_item", { qty: 2, name: "Cola" })}, ${t("hub.products.component_item", { qty: 1, name: "Noodles" })}`,
    }),
  );
  await expect(items(page, "product")).toHaveCount(3);
});

test("search finds an item by part of its name or by barcode", async ({ page }) => {
  await open(page);
  const search = page.getByLabel(t("owner.products.search"));
  await search.fill("noo");
  await expect(items(page, "product")).toHaveCount(1);
  await expect(item(page, "product", "Noodles")).toBeVisible();
  await search.fill("555000");
  await expect(items(page, "product")).toHaveCount(1);
  await expect(item(page, "product", "Chips")).toBeVisible();
  await expect(items(page, "bundle")).toHaveCount(0);
  await search.fill("");
  await expect(items(page, "product")).toHaveCount(3);
});

test("editing a product previews product.upsert with the row's base_rev, no MFA, and no picture or stock", async ({
  page,
}) => {
  const calls = await open(page);
  await item(page, "product", "Cola")
    .getByRole("button", { name: t("hub.products.edit", { name: "Cola" }) })
    .click();
  const form = page.getByRole("form", { name: t("hub.products.edit", { name: "Cola" }) });
  await expect(form.getByLabel(t("hub.products.price"))).toHaveValue("50.00");
  await expect(form.getByLabel(t("hub.products.tax"))).toHaveValue("12");
  await expect(form.getByLabel(t("hub.products.hsn"))).toHaveValue("2202");
  await expect(form.getByLabel(t("hub.products.barcode"))).toHaveValue("890123");
  await expect(form.getByRole("checkbox", { name: t("hub.products.track") })).toBeChecked();
  await expect(form.getByText(t("owner.products.image_kept"))).toBeVisible();
  // Stock quantities are never edited from the Owner app.
  await expect(form.getByLabel(/quantity|on hand|stock-in/i)).toHaveCount(0);

  await form.getByLabel(t("hub.products.price")).fill("60");
  await form.getByRole("checkbox", { name: t("hub.products.kitchen") }).check();
  await formSend(form).click();

  const p = preview(page);
  await expect(p).toBeVisible();
  await expect(p.getByText(`${t("owner.cmd.kind.product")} · Cola`)).toBeVisible();
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(page.getByText(/authenticator|verification code/i)).toHaveCount(0);
  expect(posts(calls)).toHaveLength(0);
  await sendIn(p).click();

  await expect(p).toHaveCount(0);
  const sent = posts(calls);
  expect(sent).toHaveLength(1);
  expect(sent[0]).toMatchObject({ branch_id: "b1", kind: "product.upsert", base_rev: 3 });
  expect(sent[0].idempotency_key).toMatch(ULID);
  expect(sent[0].payload).toEqual({
    id: "pr1",
    name: "Cola",
    category: "Drinks",
    price_minor: 6000,
    tax_rate_bp: 1200,
    hsn: "2202",
    track_stock: true,
    barcode: "890123",
    kitchen: true,
    favourite: false,
    active: true,
    bundle: false,
    components: [],
  });
});

test("invalid product fields show the Hub's copy and open no preview", async ({ page }) => {
  const calls = await open(page);
  await page.getByRole("button", { name: t("hub.products.add"), exact: true }).click();
  const form = page.getByRole("form", { name: t("hub.products.add") });
  await formSend(form).click();
  await expect(form.getByText(t("hub.products.invalid.name"))).toBeVisible();
  await expect(form.getByText(t("hub.products.invalid.price"))).toBeVisible();

  await form.getByLabel(t("hub.products.name")).fill("Tea");
  await form.getByLabel(t("hub.products.price")).fill("10.555");
  await form.getByLabel(t("hub.products.tax")).fill("120");
  await form.getByLabel(t("hub.products.hsn")).fill("12");
  await form.getByLabel(t("hub.products.barcode")).fill("a b");
  await formSend(form).click();
  for (const key of [
    "hub.products.invalid.price",
    "hub.products.invalid.tax",
    "hub.products.invalid.hsn",
    "hub.products.invalid.barcode",
  ] as const) {
    await expect(form.getByText(t(key))).toBeVisible();
  }
  await expect(form.getByLabel(t("hub.products.price"))).toHaveAttribute("aria-invalid", "true");
  await expect(preview(page)).toHaveCount(0);
  expect(posts(calls)).toHaveLength(0);
});

test("a new product for All branches is created where the name is free and left out where it exists", async ({
  page,
}) => {
  const calls = await open(page, TWO);
  await page.getByRole("button", { name: t("hub.products.add"), exact: true }).click();
  const form = page.getByRole("form", { name: t("hub.products.add") });
  await form.getByLabel(t("hub.products.name")).fill("Water");
  await form.getByLabel(t("hub.products.price")).fill("20");
  await formSend(form).click();
  let p = preview(page);
  await expect(p.getByText(t("owner.cmd.scope.all"))).toBeVisible();
  await sendIn(p).click();
  await expect(p).toHaveCount(0);
  let sent = posts(calls).sort((a, b) => a.branch_id.localeCompare(b.branch_id));
  expect(sent.map((s) => s.branch_id)).toEqual(["b1", "b3"]);
  for (const s of sent) {
    expect(s.payload).not.toHaveProperty("id");
    expect(s).not.toHaveProperty("base_rev");
    expect(s.payload).toMatchObject({ name: "Water", price_minor: 2000, active: true });
    expect(s.payload).not.toHaveProperty("image");
    expect(s.payload).not.toHaveProperty("tax_rate_bp");
  }

  // "cola" exists at b1 and b3 (any case): adding it again is refused per branch with the reason.
  await page.getByRole("button", { name: t("hub.products.add"), exact: true }).click();
  const again = page.getByRole("form", { name: t("hub.products.add") });
  await again.getByLabel(t("hub.products.name")).fill(" COLA ");
  await again.getByLabel(t("hub.products.price")).fill("20");
  await formSend(again).click();
  p = preview(page);
  await expect(p.locator("[data-excluded]")).toHaveText([
    t("owner.pricing.excluded.label", {
      reason: t("owner.pricing.excluded.exists", { name: "Cola" }),
    }),
    t("owner.pricing.excluded.label", {
      reason: t("owner.pricing.excluded.exists", { name: "cola" }),
    }),
  ]);
  sent = posts(calls);
  expect(sent).toHaveLength(2);
});

test("a barcode another active item uses keeps that branch out, with the reason inline", async ({
  page,
}) => {
  const calls = await open(page);
  await page.getByRole("button", { name: t("hub.products.add"), exact: true }).click();
  const form = page.getByRole("form", { name: t("hub.products.add") });
  await form.getByLabel(t("hub.products.name")).fill("Cola zero");
  await form.getByLabel(t("hub.products.price")).fill("50");
  await form.getByLabel(t("hub.products.barcode")).fill("890123");
  await formSend(form).click();
  const p = preview(page);
  await expect(
    p.getByText(
      t("owner.pricing.excluded.label", {
        reason: t("owner.products.excluded.barcode", { name: "Cola" }),
      }),
    ),
  ).toBeVisible();
  expect(posts(calls)).toHaveLength(0);
});

test("a bundle goes only to the branches that hold every component, the others say why", async ({
  page,
}) => {
  const calls = await open(page, TWO);
  await item(page, "bundle", "Combo")
    .getByRole("button", { name: t("hub.products.edit", { name: "Combo" }) })
    .click();
  const form = page.getByRole("form", { name: t("hub.products.edit", { name: "Combo" }) });
  await expect(form.getByRole("combobox").nth(0)).toHaveValue("Cola");
  await expect(form.getByLabel(t("hub.products.component_qty", { n: 1 }))).toHaveValue("2");
  await expect(form.getByRole("combobox").nth(1)).toHaveValue("Noodles");
  await form.getByLabel(t("hub.products.price")).fill("140");
  await formSend(form).click();

  const p = preview(page);
  const left = p.getByRole("listitem").filter({ hasText: B3 });
  await expect(
    left.getByText(
      t("owner.pricing.excluded.label", {
        reason: t("owner.pricing.excluded.missing", { name: "Noodles" }),
      }),
    ),
  ).toBeVisible();
  await expect(left.getByRole("radio")).toHaveCount(0);
  await sendIn(p).click();
  await expect(p).toHaveCount(0);

  const sent = posts(calls);
  expect(sent).toHaveLength(1);
  expect(sent[0]).toMatchObject({ branch_id: "b1", kind: "product.upsert", base_rev: 5 });
  expect(sent[0].payload).toMatchObject({
    id: "bu1",
    name: "Combo",
    price_minor: 14_000,
    bundle: true,
    track_stock: false,
    components: [
      { product_id: "pr1", qty: 2 },
      { product_id: "pr2", qty: 1 },
    ],
  });
});

test("a bundle checks its components with the Hub's copy", async ({ page }) => {
  const calls = await open(page);
  await page.getByRole("button", { name: t("owner.products.add_bundle") }).click();
  const form = page.getByRole("form", { name: t("owner.products.add_bundle") });
  await form.getByLabel(t("hub.products.name")).fill("Duo");
  await form.getByLabel(t("hub.products.price")).fill("90");
  await formSend(form).click();
  await expect(form.getByText(t("hub.products.invalid.components"))).toBeVisible();

  await form.getByRole("combobox").nth(0).selectOption("Cola");
  await form.getByLabel(t("hub.products.component_qty", { n: 1 })).fill("0");
  await formSend(form).click();
  await expect(form.getByText(t("hub.products.invalid.components"))).toBeVisible();

  await form.getByLabel(t("hub.products.component_qty", { n: 1 })).fill("2");
  await form.getByRole("button", { name: t("hub.products.component_add") }).click();
  await form.getByRole("combobox").nth(1).selectOption("Cola");
  await formSend(form).click();
  await expect(form.getByText(t("hub.products.invalid.components"))).toBeVisible();
  await expect(preview(page)).toHaveCount(0);

  await form.getByRole("combobox").nth(1).selectOption("Noodles");
  await formSend(form).click();
  await sendIn(preview(page)).click();
  const sent = posts(calls);
  expect(sent).toHaveLength(1);
  expect(sent[0].payload).toMatchObject({
    name: "Duo",
    bundle: true,
    track_stock: false,
    components: [
      { product_id: "pr1", qty: 2 },
      { product_id: "pr2", qty: 1 },
    ],
  });
});

test("an attached picture is made at most 512 px and 200 KB in the browser before it is sent", async ({
  page,
}) => {
  const calls = await open(page);
  const png = await page.evaluate(() => {
    const c = document.createElement("canvas");
    c.width = 1600;
    c.height = 1200;
    const ctx = c.getContext("2d")!;
    for (let i = 0; i < 400; i++) {
      ctx.fillStyle = `rgb(${(i * 37) % 256},${(i * 91) % 256},${(i * 53) % 256})`;
      ctx.fillRect((i * 13) % 1600, (i * 29) % 1200, 120, 90);
    }
    return c.toDataURL("image/png").split(",")[1];
  });
  await item(page, "product", "Noodles")
    .getByRole("button", { name: t("hub.products.edit", { name: "Noodles" }) })
    .click();
  const form = page.getByRole("form", { name: t("hub.products.edit", { name: "Noodles" }) });
  await form.getByLabel(t("hub.products.image"), { exact: true }).setInputFiles({
    name: "noodles.png",
    mimeType: "image/png",
    buffer: Buffer.from(png, "base64"),
  });
  await expect(form.locator("img.picture")).toBeVisible();
  await formSend(form).click();
  await sendIn(preview(page)).click();

  const sent = posts(calls);
  expect(sent).toHaveLength(1);
  const image = sent[0].payload.image as string;
  expect(image.startsWith("data:image/jpeg;base64,")).toBe(true);
  expect(Buffer.from(image.split(",")[1], "base64").length).toBeLessThanOrEqual(200 * 1024);
  const size = await page.evaluate(async (src) => {
    const bitmap = await createImageBitmap(await (await fetch(src)).blob());
    return [bitmap.width, bitmap.height];
  }, image);
  expect(Math.max(...size)).toBeLessThanOrEqual(512);
  expect(size[0] / size[1]).toBeCloseTo(4 / 3, 1);
});

test("removing the picture sends an empty image; a file that is not a picture is refused", async ({
  page,
}) => {
  const calls = await open(page);
  await item(page, "product", "Noodles")
    .getByRole("button", { name: t("hub.products.edit", { name: "Noodles" }) })
    .click();
  const form = page.getByRole("form", { name: t("hub.products.edit", { name: "Noodles" }) });
  await form.getByLabel(t("hub.products.image"), { exact: true }).setInputFiles({
    name: "notes.txt",
    mimeType: "text/plain",
    buffer: Buffer.from("not a picture"),
  });
  await expect(form.getByText(t("hub.products.invalid.image"))).toBeVisible();
  await formSend(form).click();
  await expect(preview(page)).toHaveCount(0);

  await form.getByRole("button", { name: t("hub.products.image_remove") }).click();
  await expect(form.getByText(t("owner.products.image_removed"))).toBeVisible();
  await formSend(form).click();
  await sendIn(preview(page)).click();
  expect(posts(calls)[0].payload.image).toBe("");
});

test("the low-stock level of a tracked product is sent as stock.threshold, to each branch with its own id", async ({
  page,
}) => {
  const calls = await open(page, TWO);
  await expect(
    item(page, "product", "Noodles").getByRole("button", {
      name: t("owner.products.threshold_edit", { name: "Noodles" }),
    }),
  ).toHaveCount(0);
  await expect(
    item(page, "bundle", "Combo").getByRole("button", {
      name: t("owner.products.threshold_edit", { name: "Combo" }),
    }),
  ).toHaveCount(0);

  await item(page, "product", "Cola")
    .getByRole("button", { name: t("owner.products.threshold_edit", { name: "Cola" }) })
    .click();
  const form = page.getByRole("form", {
    name: t("owner.products.threshold_title", { name: "Cola" }),
  });
  await expect(form.getByLabel(t("owner.products.threshold"))).toHaveValue("4");
  await form.getByLabel(t("owner.products.threshold")).fill("-1");
  await formSend(form).click();
  await expect(form.getByText(t("hub.lowstock.invalid"))).toBeVisible();
  await expect(preview(page)).toHaveCount(0);

  await form.getByLabel(t("owner.products.threshold")).fill("6");
  await formSend(form).click();
  await sendIn(preview(page)).click();
  await expect(preview(page)).toHaveCount(0);
  const sent = posts(calls).sort((a, b) => a.branch_id.localeCompare(b.branch_id));
  expect(sent.map((s) => [s.branch_id, s.kind, s.payload])).toEqual([
    ["b1", "stock.threshold", { product_id: "pr1", low_stock_qty: 6 }],
    ["b3", "stock.threshold", { product_id: "h1", low_stock_qty: 6 }],
  ]);
  for (const s of sent) expect(s).not.toHaveProperty("base_rev");
});

test("an empty low-stock level clears it", async ({ page }) => {
  const calls = await open(page);
  await item(page, "product", "Cola")
    .getByRole("button", { name: t("owner.products.threshold_edit", { name: "Cola" }) })
    .click();
  const form = page.getByRole("form", {
    name: t("owner.products.threshold_title", { name: "Cola" }),
  });
  await form.getByLabel(t("owner.products.threshold")).fill("");
  await formSend(form).click();
  await sendIn(preview(page)).click();
  expect(posts(calls)[0]).toMatchObject({
    kind: "stock.threshold",
    payload: { product_id: "pr1", low_stock_qty: null },
  });
});

test("an older summary lists the product by name and hides Edit and the low-stock level", async ({
  page,
}) => {
  await stubCloud(page, ONE, {
    live: { b1: liveWith("b1", { products: [row("pr9", "Old cola", undefined)], bundles: [] }) },
  });
  await page.goto("/manage?area=products");
  const old = item(page, "product", "Old cola");
  await expect(old).toBeVisible();
  await expect(old.getByRole("button")).toHaveCount(0);
});

test("with no signal the per-branch product changes are queued, one per branch", async ({
  page,
}) => {
  const calls = await open(page, TWO);
  await item(page, "product", "Cola")
    .getByRole("button", { name: t("hub.products.edit", { name: "Cola" }) })
    .click();
  const form = page.getByRole("form", { name: t("hub.products.edit", { name: "Cola" }) });
  await form.getByLabel(t("hub.products.price")).fill("70");
  await formSend(form).click();
  await expect(preview(page)).toBeVisible();
  await page.route("**/v1/commands", (route) =>
    route.request().method() === "POST" ? route.abort("internetdisconnected") : route.fallback(),
  );
  await sendIn(preview(page)).click();
  await expect(preview(page)).toHaveCount(0);
  await expect(page.getByText(t("owner.cmd.state.queued"))).toHaveCount(2);
  expect(posts(calls)).toHaveLength(0);
});

test("a branch that has sent no catalogue yet says so", async ({ page }) => {
  await stubCloud(page, ONE, {
    live: { b1: { ...liveWith("b1", {}), summary: null, as_of: null } },
  });
  await page.goto("/manage?area=products");
  await expect(page.getByText(t("owner.pricing.age_none", { branch: B1 }))).toBeVisible();
  await expect(page.getByText(t("hub.products.empty")).first()).toBeVisible();
});

for (const theme of ["light", "dark"] as const) {
  test(`the product form passes the accessibility checks (${theme})`, async ({ page }) => {
    await page.addInitScript((v) => localStorage.setItem("arena.theme", v), theme);
    await open(page, TWO);
    await item(page, "bundle", "Combo")
      .getByRole("button", { name: t("hub.products.edit", { name: "Combo" }) })
      .click();
    await expect(
      page.getByRole("form", { name: t("hub.products.edit", { name: "Combo" }) }),
    ).toBeVisible();
    await page.evaluate(() => document.fonts.ready);
    const small = await page.locator('button, a, input, select, [role="tab"]').evaluateAll((els) =>
      els
        .map((el) => [el.outerHTML.slice(0, 70), el.getBoundingClientRect()] as const)
        .filter(([, r]) => r.width > 0 && (r.width < 44 || r.height < 44))
        .map(([h, r]) => `${h} ${r.width}x${r.height}`),
    );
    expect(small, small.join("\n")).toEqual([]);
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth > document.documentElement.clientWidth,
      ),
    ).toBe(false);
    const blocking = (await new AxeBuilder({ page }).analyze()).violations.filter(
      (v) => v.impact === "serious" || v.impact === "critical",
    );
    const report = blocking.map(
      (v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`,
    );
    expect(report, report.join("\n")).toEqual([]);
  });
}
