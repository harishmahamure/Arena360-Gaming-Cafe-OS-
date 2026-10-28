import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type {
  HubSnapshot,
  PlanImportPreview,
  PlanImportResult,
  PlayerImportPreview,
  PlayerImportResult,
  ProductImportPreview,
  ProductImportResult,
  SessionStaff,
  StarterPreview,
  StarterResult,
  StarterTemplateInfo,
} from "@arena/ui/generated";
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
    permissions: ["catalog.edit", "settings.edit", "player.manage"],
  },
  "counter-token": {
    staff_id: "01JCOUNTER0000000000000000",
    display_name: "Counter",
    role_name: "Counter",
    permissions: ["session.start", "pos.sell"],
  },
};
const SNAPSHOT: HubSnapshot = {
  branch: { name: "Dev Branch", branch_code: "DEV01" },
  groups: [],
  zones: [],
  devices: [],
  locale: null,
  ui: { idle_lock_minutes: 5, hc: false },
  connectivity: { state: "online", checked_at: null },
  lan: null,
  paired: true,
  setup: null,
};

const json = (route: Route, status: number, body: unknown) =>
  route.fulfill({
    status,
    contentType: "application/json",
    headers: CORS,
    body: JSON.stringify(body),
  });

const CLEAN: ProductImportPreview = {
  rows: 3,
  new: 2,
  updated: 1,
  failed: 0,
  with_stock: 2,
  failures: [],
  truncated: false,
  unusable: false,
  can_commit: true,
  can_commit_valid: true,
};
const BROKEN: ProductImportPreview = {
  rows: 4,
  new: 2,
  updated: 0,
  failed: 2,
  with_stock: 0,
  failures: [
    { row: 3, field: "price", reason: "bad_number", message: "price must be a number" },
    {
      row: 5,
      field: "barcode",
      reason: "duplicate_barcode",
      message: "the barcode is already used by row 2",
    },
  ],
  truncated: false,
  unusable: false,
  can_commit: false,
  can_commit_valid: true,
};
const DONE: ProductImportResult = { created: 2, updated: 1, skipped: 0, chunks: 1 };

const PLAYERS: PlayerImportPreview = {
  rows: 4,
  new: 2,
  matched: 1,
  failed: 1,
  with_balance: 1,
  with_credit: 1,
  failures: [{ row: 4, field: "phone", reason: "bad_phone", message: "phone must start with +" }],
  truncated: false,
  unusable: false,
  can_commit: false,
  can_commit_valid: true,
};
const PLAYERS_DONE: PlayerImportResult = { created: 2, matched: 1, skipped: 1, chunks: 1 };
const PLANS: PlanImportPreview = {
  rows: 3,
  new: 2,
  updated: 1,
  failed: 0,
  failures: [],
  truncated: false,
  unusable: false,
  can_commit: true,
  can_commit_valid: true,
};
const PLANS_DONE: PlanImportResult = { created: 2, updated: 1, skipped: 0, chunks: 1 };

const STARTERS: StarterTemplateInfo[] = [
  {
    id: "indian-cafe",
    name: "Indian café",
    description: "PC gaming hours and a chai-and-snacks menu.",
    plans: [
      { name: "PC 1 hour pack", plan_kind: "pack", group_kind: "pc", price_minor: 6000 },
      { name: "PC night pass", plan_kind: "pass", group_kind: "pc", price_minor: 50000 },
    ],
    products: [{ name: "Masala chai", category: "Drinks", price_minor: 2000 }],
  },
  {
    id: "vr-arcade",
    name: "VR arcade",
    description: "VR sessions.",
    plans: [{ name: "VR 1 hour pack", plan_kind: "pack", group_kind: "vr", price_minor: 35000 }],
    products: [],
  },
];
const STARTER_PREVIEW: StarterPreview = {
  template_id: "indian-cafe",
  plans: [
    { name: "PC 1 hour pack", add: true },
    { name: "PC night pass", add: false, reason: "no_group" },
  ],
  products: [{ name: "Masala chai", add: false, reason: "exists" }],
  new_plans: 1,
  new_products: 0,
  skipped_plans: 1,
  skipped_products: 1,
  can_commit: true,
};
const STARTER_DONE: StarterResult = {
  plans_created: 1,
  products_created: 0,
  skipped_plans: 1,
  skipped_products: 1,
  chunks: 1,
};

type Posted = { path: string; body: Record<string, unknown> };

async function open(
  page: Page,
  opts: { token?: string; preview?: ProductImportPreview; folderRefused?: boolean } = {},
) {
  const posts: Posted[] = [];
  let refuseFolder = opts.folderRefused ?? false;
  let starterApplied = false;
  await page.addInitScript(
    (t) => sessionStorage.setItem("arena.hub.token", t),
    opts.token ?? "manager-token",
  );
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    const { pathname } = new URL(req.url());
    const staff = STAFF[(req.headers()["authorization"] ?? "").replace(/^Bearer /, "")];
    switch (pathname) {
      case "/ui/snapshot":
        return json(route, 200, SNAPSHOT);
      case "/ui/me":
        return json(route, 200, staff);
      case "/ui/settings":
        return json(route, 200, []);
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: "retry: 400\n\n",
        });
      case "/ui/import/products/preview":
        posts.push({ path: "preview", body: req.postDataJSON() });
        return json(route, 200, opts.preview ?? CLEAN);
      case "/ui/import/products/commit":
        posts.push({ path: "commit", body: req.postDataJSON() });
        return json(route, 200, DONE);
      case "/ui/import/players/preview":
        posts.push({ path: "players/preview", body: req.postDataJSON() });
        return json(route, 200, PLAYERS);
      case "/ui/import/players/commit":
        posts.push({ path: "players/commit", body: req.postDataJSON() });
        return json(route, 200, PLAYERS_DONE);
      case "/ui/import/players/template":
        posts.push({ path: "players/template", body: req.postDataJSON() });
        return json(route, 200, {
          file_name: "players-template.csv",
          path: "/usb/players-template.csv",
        });
      case "/ui/import/plans/preview":
        posts.push({ path: "plans/preview", body: req.postDataJSON() });
        return json(route, 200, PLANS);
      case "/ui/import/plans/commit":
        posts.push({ path: "plans/commit", body: req.postDataJSON() });
        return json(route, 200, PLANS_DONE);
      case "/ui/import/templates":
        return json(route, 200, STARTERS);
      case "/ui/import/templates/preview":
        posts.push({ path: "templates/preview", body: req.postDataJSON() });
        return json(
          route,
          200,
          starterApplied ? { ...STARTER_PREVIEW, can_commit: false } : STARTER_PREVIEW,
        );
      case "/ui/import/templates/commit":
        posts.push({ path: "templates/commit", body: req.postDataJSON() });
        starterApplied = true;
        return json(route, 200, STARTER_DONE);
      case "/ui/import/products/template":
        posts.push({ path: "template", body: req.postDataJSON() });
        if (refuseFolder) {
          refuseFolder = false;
          return json(route, 422, {
            code: "VALIDATION_FAILED",
            field: "folder",
            message: "not_found",
          });
        }
        return json(route, 200, {
          file_name: "products-template.csv",
          path: "/usb/products-template.csv",
        });
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/settings/import");
  return posts;
}

const Buf = (globalThis as unknown as { Buffer: { from: (v: string) => never } }).Buffer;
const FILE = "name,price\nCola,40\nTea,abc\n";
const choose = (page: Page) =>
  page.getByTestId("import-file").setInputFiles({
    name: "products.csv",
    mimeType: "text/csv",
    buffer: Buf.from(FILE),
  });

test("Settings has an Import section; without catalogue access it only says so", async ({
  page,
}) => {
  await open(page, { token: "counter-token" });
  await expect(page.getByTestId("import-no-access")).toHaveText(en["hub.import.no_access"]);
  await expect(page.getByTestId("import-file")).toHaveCount(0);
});

test("a dry-run shows counts and each failure with its row, and commit stays closed", async ({
  page,
}) => {
  const posts = await open(page, { preview: BROKEN });
  await expect(
    page
      .getByRole("navigation", { name: en["hub.settings.sections"] })
      .getByRole("link", { name: en["hub.settings.nav.import"] }),
  ).toBeVisible();
  await choose(page);
  await expect(page.getByTestId("import-summary")).toContainText(
    "4 rows: 2 new, 0 updated, 2 with problems",
  );
  const rows = page.getByTestId("import-failure");
  await expect(rows).toHaveCount(2);
  await expect(rows.nth(0)).toContainText("3");
  await expect(rows.nth(0)).toContainText("price");
  await expect(rows.nth(0)).toContainText("price must be a number");
  await expect(
    page.getByRole("button", { name: en["hub.import.commit"], exact: true }),
  ).toHaveAttribute("aria-disabled", "true");
  expect(posts.map((p) => p.path)).toEqual(["preview"]);
  expect(posts[0].body).toEqual({ csv: FILE });
  const results = await new AxeBuilder({ page }).analyze();
  expect(
    results.violations.filter((v) => v.impact === "serious" || v.impact === "critical"),
  ).toEqual([]);
});

test("Import valid rows only sends the file with valid_only and one id for this attempt", async ({
  page,
}) => {
  const posts = await open(page, { preview: BROKEN });
  await choose(page);
  await page.getByRole("button", { name: en["hub.import.commit_valid"] }).click();
  await expect(page.getByTestId("import-done")).toContainText("Imported: 2 new, 1 updated.");
  const commit = posts.find((p) => p.path === "commit")!;
  expect(commit.body.valid_only).toBe(true);
  expect(commit.body.csv).toBe(FILE);
  expect(String(commit.body.import_id)).toMatch(/^[0-9A-HJKMNP-TV-Z]{26}$/);
  await expect(page.getByTestId("import-preview")).toHaveCount(0);
});

test("a clean file commits whole", async ({ page }) => {
  const posts = await open(page);
  await choose(page);
  await expect(page.getByTestId("import-summary")).toContainText("3 rows: 2 new, 1 updated");
  await expect(page.getByTestId("import-failures")).toHaveCount(0);
  await expect(page.getByRole("button", { name: en["hub.import.commit_valid"] })).toHaveCount(0);
  await page.getByRole("button", { name: en["hub.import.commit"], exact: true }).click();
  await expect(page.getByTestId("import-done")).toBeVisible();
  expect(posts.find((p) => p.path === "commit")!.body.valid_only).toBe(false);
});

test("Download template names a bad folder inline and then saves", async ({ page }) => {
  const posts = await open(page, { folderRefused: true });
  await page.getByRole("button", { name: en["hub.import.template_button"] }).click();
  await expect(page.getByTestId("template-folder-error")).toBeVisible();
  expect(posts).toEqual([]);
  await page.getByTestId("template-folder").fill("/nope");
  await page.getByRole("button", { name: en["hub.import.template_button"] }).click();
  await expect(page.getByTestId("template-folder-error")).toBeVisible();
  await page.getByTestId("template-folder").fill("/usb");
  await page.getByRole("button", { name: en["hub.import.template_button"] }).click();
  await expect(page.getByTestId("template-status-text")).toContainText(
    "/usb/products-template.csv",
  );
  expect(posts.map((p) => p.body.folder)).toEqual(["/nope", "/usb"]);
});

test("the Players tab checks phones, shows matched rows and imports the valid ones", async ({
  page,
}) => {
  const posts = await open(page);
  await page.getByRole("tab", { name: en["hub.import.tab_players"] }).click();
  await expect(page.getByRole("tab", { name: en["hub.import.tab_players"] })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  await page.getByTestId("import-file").setInputFiles({
    name: "players.csv",
    mimeType: "text/csv",
    buffer: Buf.from("phone,name,balance,credit_limit\n+919800000001,A,10,0\n"),
  });
  await expect(page.getByTestId("import-summary")).toContainText(
    "4 rows: 2 new, 1 already have a profile, 1 with problems",
  );
  await expect(page.getByTestId("import-note")).toContainText(
    "1 start with a balance and 1 get a credit limit",
  );
  await expect(page.getByTestId("import-failure")).toContainText("phone must start with +");
  await expect(
    page.getByRole("button", { name: en["hub.import.commit"], exact: true }),
  ).toHaveAttribute("aria-disabled", "true");
  await page.getByRole("button", { name: en["hub.import.commit_valid"] }).click();
  await expect(page.getByTestId("import-done")).toContainText(
    "Imported: 2 new players, 1 already had a profile.",
  );
  const commit = posts.find((p) => p.path === "players/commit")!;
  expect(commit.body.valid_only).toBe(true);
  expect(String(commit.body.import_id)).toMatch(/^[0-9A-HJKMNP-TV-Z]{26}$/);
  await page.getByTestId("template-folder").fill("/usb");
  await page.getByRole("button", { name: en["hub.import.template_button"] }).click();
  await expect(page.getByTestId("template-status-text")).toContainText("/usb/players-template.csv");
  const results = await new AxeBuilder({ page }).analyze();
  expect(
    results.violations.filter((v) => v.impact === "serious" || v.impact === "critical"),
  ).toEqual([]);
});

test("the Players tab needs permission to manage players", async ({ page }) => {
  await open(page, { token: "counter-token" });
  await page.getByRole("tab", { name: en["hub.import.tab_players"] }).click();
  await expect(page.getByTestId("import-no-access")).toHaveText(en["hub.import.players_no_access"]);
  await expect(page.getByTestId("import-file")).toHaveCount(0);
});

test("the Plans tab previews and commits a plan file whole", async ({ page }) => {
  const posts = await open(page);
  await page.getByRole("tab", { name: en["hub.import.tab_plans"] }).click();
  await page.getByTestId("import-file").setInputFiles({
    name: "plans.csv",
    mimeType: "text/csv",
    buffer: Buf.from("name,type,price,groups\n2 h,pack,100,PC\n"),
  });
  await expect(page.getByTestId("import-summary")).toContainText(
    "3 rows: 2 new, 1 updated, 0 with problems",
  );
  await page.getByRole("button", { name: en["hub.import.commit"], exact: true }).click();
  await expect(page.getByTestId("import-done")).toContainText("Imported: 2 new, 1 updated.");
  expect(posts.map((p) => p.path)).toEqual(["plans/preview", "plans/commit"]);
  expect(posts[1].body.valid_only).toBe(false);
});

test("the Templates tab lists each template, shows a dry-run and adds it once", async ({
  page,
}) => {
  const posts = await open(page);
  await page.getByRole("tab", { name: en["hub.import.tab_templates"] }).click();
  await expect(page.getByTestId("starter-indian-cafe")).toContainText("Indian café");
  await expect(page.getByTestId("starter-vr-arcade")).toBeVisible();
  await page.getByTestId("starter-indian-cafe").getByRole("radio").check();
  await expect(page.getByTestId("starter-plans").locator("li")).toHaveCount(2);
  await expect(page.getByTestId("starter-plans")).toContainText("PC 1 hour pack · Time pack");
  await expect(page.getByTestId("starter-products")).toContainText("Masala chai · Drinks");
  await expect(page.getByTestId("import-summary")).toContainText(
    "1 plans and 0 products will be added; 1 plans and 1 products are skipped.",
  );
  const items = page.getByTestId("starter-item");
  await expect(items).toHaveCount(3);
  await expect(items.nth(1)).toContainText(en["hub.import.starter_skip_no_group"]);
  await expect(items.nth(2)).toContainText(en["hub.import.starter_skip_exists"]);
  const results = await new AxeBuilder({ page }).analyze();
  expect(
    results.violations.filter((v) => v.impact === "serious" || v.impact === "critical"),
  ).toEqual([]);
  await page.getByRole("button", { name: en["hub.import.starter_add"] }).click();
  await expect(page.getByTestId("import-done")).toContainText("Added 1 plans and 0 products.");
  await expect(page.getByTestId("starter-nothing")).toBeVisible();
  await expect(page.getByRole("button", { name: en["hub.import.starter_add"] })).toHaveAttribute(
    "aria-disabled",
    "true",
  );
  expect(posts.map((p) => p.path)).toEqual([
    "templates/preview",
    "templates/commit",
    "templates/preview",
  ]);
  expect(posts[1].body.template_id).toBe("indian-cafe");
  expect(String(posts[1].body.import_id)).toMatch(/^[0-9A-HJKMNP-TV-Z]{26}$/);
});

test("the Templates tab needs permission to edit the catalogue", async ({ page }) => {
  await open(page, { token: "counter-token" });
  await page.getByRole("tab", { name: en["hub.import.tab_templates"] }).click();
  await expect(page.getByTestId("import-no-access")).toHaveText(en["hub.import.starter_no_access"]);
});
