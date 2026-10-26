import { expect, test, type Page } from "@playwright/test";
import type { BranchSetup, SetupGroup, SetupStatus } from "@arena/ui/generated";
import { READY_OWNER, stubCloud, t, type WizardStart } from "./stub";

type Key = Parameters<typeof t>[0];
const h1 = (page: Page, key: Key) => page.getByRole("heading", { level: 1, name: t(key) });
const skip = (page: Page) => page.getByRole("button", { name: t("onboarding.skip"), exact: true });
const group = (id: string, name: string, has_rate: boolean): SetupGroup => ({
  id,
  name,
  kind: "pc",
  colour: "teal",
  sort: 0,
  has_rate,
});
const status = (printer: SetupStatus["printer"]): SetupStatus => ({
  opening_hours_set: false,
  backup_location_set: false,
  ad_consent: "missing",
  printer,
  products_count: 0,
  groups_without_rate: 0,
  manager_exists: false,
  test_session: "unset",
});
const block = (over: Partial<BranchSetup> = {}): BranchSetup => ({
  devices: [],
  groups: [group("G1", "Gaming", false), group("G2", "Console room", true)],
  status: status("unset"),
  ...over,
});

async function open(
  page: Page,
  path: string,
  done: WizardStart["done"],
  setup = block(),
  more = {},
) {
  const cloud = await stubCloud(page, {
    signedIn: true,
    owner: READY_OWNER,
    wizard: {
      branch: {},
      done,
      hubPaired: true,
      devices: { setup, reported_at: Date.now(), now: Date.now(), ...more },
    },
  });
  await page.goto(path);
  return cloud;
}

const BEFORE_RATES: WizardStart["done"] = ["cafe", "tier", "install_hub", "devices"];
const BEFORE_PRODUCTS: WizardStart["done"] = [...BEFORE_RATES, "rates"];
const BEFORE_PRINTER: WizardStart["done"] = [...BEFORE_PRODUCTS, "products", "staff"];

test("step 5 lists the groups without a rate and the import line", async ({ page }) => {
  await open(page, "/setup/rates", BEFORE_RATES);
  await expect(h1(page, "ui.wizard.step.rates")).toBeVisible();
  await expect(page.getByTestId("rates-missing")).toHaveText("Gaming");
  await expect(page.getByText(t("onboarding.import_hint"))).toBeVisible();
});

test("step 5 completes by itself and focus moves to step 6", async ({ page }) => {
  await page.clock.install();
  const cloud = await open(page, "/setup/rates", BEFORE_RATES);
  await expect(h1(page, "ui.wizard.step.rates")).toBeVisible();
  cloud.deviceView.setup = block({ groups: [group("G1", "Gaming", true)] });
  cloud.done.add("rates");
  await page.clock.runFor(5000);
  await expect(page).toHaveURL(/\/setup\/products$/);
  await expect(h1(page, "ui.wizard.step.products")).toBeFocused();
});

test("step 5 with no report yet says it is waiting for the Hub", async ({ page }) => {
  await open(page, "/setup/rates", BEFORE_RATES, null as unknown as BranchSetup);
  await expect(page.getByTestId("hub-waiting")).toHaveText(t("onboarding.hub.waiting"));
});

test("step 5 with the Hub offline shows the last report with its age and a badge", async ({
  page,
}) => {
  const now = Date.now();
  await open(page, "/setup/rates", BEFORE_RATES, block(), {
    hub_online: false,
    reported_at: now - 3 * 60_000 - 5000,
    now,
  });
  const offline = page.getByTestId("hub-offline");
  await expect(offline).toContainText(t("onboarding.devices.hub_offline"));
  await expect(offline).toContainText(t("onboarding.devices.as_of", { n: 3 }));
  await expect(page.getByTestId("rates-missing")).toBeVisible();
});

test("Skip for now on step 6 skips it and opens step 7", async ({ page }) => {
  const cloud = await open(page, "/setup/products", BEFORE_PRODUCTS);
  await expect(page.getByText(t("onboarding.products.intro"))).toBeVisible();
  await expect(page.getByText(t("onboarding.import_hint"))).toBeVisible();
  await skip(page).click();
  await expect(h1(page, "ui.wizard.step.staff")).toBeVisible();
  expect([...cloud.skipped]).toEqual(["products"]);
});

test("step 8 shows the Hub's printer state and Skip for now skips it", async ({ page }) => {
  const cloud = await open(
    page,
    "/setup/printer_payments",
    BEFORE_PRINTER,
    block({ status: status("failed") }),
  );
  await expect(page.getByTestId("printer-state")).toHaveText(t("onboarding.printer.state.failed"));
  await skip(page).click();
  await expect(h1(page, "ui.wizard.step.test_session")).toBeVisible();
  expect([...cloud.skipped]).toEqual(["printer_payments"]);
});

test("step 8 completes by itself when the Hub reports the printer ok", async ({ page }) => {
  await page.clock.install();
  const cloud = await open(page, "/setup/printer_payments", BEFORE_PRINTER);
  await expect(page.getByTestId("printer-state")).toHaveText(t("onboarding.printer.state.unset"));
  cloud.deviceView.setup = block({ status: status("ok") });
  cloud.done.add("printer_payments");
  await page.clock.runFor(5000);
  await expect(h1(page, "ui.wizard.step.test_session")).toBeFocused();
});

test("a branch in the address is carried on every wizard call", async ({ page }) => {
  const urls: string[] = [];
  page.on("request", (r) => urls.push(r.url()));
  await open(page, "/setup/rates?branch=B2", BEFORE_RATES);
  await expect(h1(page, "ui.wizard.step.rates")).toBeVisible();
  expect(urls.some((u) => u.includes("/v1/wizard?branch_id=B2"))).toBe(true);
  expect(urls.some((u) => u.includes("/v1/wizard/events?branch_id=B2"))).toBe(true);
  await expect(page).toHaveURL(/\/setup\/rates\?branch=B2$/);
});
