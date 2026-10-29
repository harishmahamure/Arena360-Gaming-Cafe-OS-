import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type { GoLiveView } from "@arena/ui/generated";
import { READY_OWNER, stubCloud, t, type WizardStart } from "./stub";

type Key = Parameters<typeof t>[0];
const h1 = (page: Page, key: Key) => page.getByRole("heading", { level: 1, name: t(key) });
const BEFORE: NonNullable<WizardStart["done"]> = [
  "cafe",
  "tier",
  "install_hub",
  "devices",
  "rates",
  "products",
  "staff",
  "printer_payments",
  "test_session",
];
const button = (page: Page) =>
  page.getByRole("button", { name: t("onboarding.golive.button"), exact: true });

async function open(page: Page, goLive: Partial<GoLiveView> = {}) {
  const cloud = await stubCloud(page, {
    signedIn: true,
    owner: READY_OWNER,
    wizard: { branch: {}, done: BEFORE, hubPaired: true, goLive },
  });
  await page.goto("/setup/go_live");
  await expect(h1(page, "ui.wizard.step.go_live")).toBeVisible();
  return cloud;
}

test("every item shows its state and standby is advisory", async ({ page }) => {
  await open(page);
  const list = page.getByTestId("golive-list");
  await expect(list.getByRole("listitem")).toHaveCount(4);
  await expect(list.getByText(t("onboarding.golive.recommended"))).toBeVisible();
  await expect(list.getByText(t("onboarding.golive.paid_only"))).toBeVisible();
  await expect(button(page)).toBeEnabled();
  const axe = await new AxeBuilder({ page }).analyze();
  expect(axe.violations).toEqual([]);
});

test("a paid branch has no ad consent item and no paid-only note", async ({ page }) => {
  await open(page, {
    free: false,
    items: [
      { key: "opening_hours", done: true },
      { key: "backup_location", done: true },
      { key: "standby", done: false },
    ],
  });
  await expect(page.getByTestId("golive-list").getByRole("listitem")).toHaveCount(3);
  await expect(page.getByText(t("onboarding.golive.paid_only"))).toHaveCount(0);
});

test("a missing item disables Go live and names the first one", async ({ page }) => {
  const cloud = await open(page, {
    blocker: { item: "backup_location" },
    items: [
      { key: "opening_hours", done: true },
      { key: "backup_location", done: false },
      { key: "standby", done: false },
      { key: "ad_consent", done: true },
    ],
  });
  await expect(button(page)).toHaveAttribute("aria-disabled", "true");
  await expect(page.getByText(t("onboarding.golive.blocked.backup_location"))).toBeVisible();
  await button(page).click({ force: true });
  expect(cloud.goLivePosts).toBe(0);
});

test("Go live ends on the done screen that opens the dashboard", async ({ page }) => {
  const cloud = await open(page);
  await button(page).click();
  await expect(h1(page, "onboarding.golive.live.title")).toBeVisible();
  expect(cloud.goLivePosts).toBe(1);
  await expect(page.getByTestId("golive-dashboard")).toHaveAttribute(
    "href",
    "https://app.arena360.cloud/",
  );
  const axe = await new AxeBuilder({ page }).analyze();
  expect(axe.violations).toEqual([]);
});

test("a live branch's wizard URL goes to the Owner app dashboard (Today)", async ({ page }) => {
  await stubCloud(page, {
    signedIn: true,
    owner: READY_OWNER,
    wizard: { branch: {}, done: [...BEFORE, "go_live"], hubPaired: true, goLive: { live: true } },
  });
  await page.route("https://app.arena360.cloud/", (route) =>
    route.fulfill({ body: "<h1>today</h1>", contentType: "text/html" }),
  );
  await page.goto("/setup");
  await expect(page).toHaveURL("https://app.arena360.cloud/");
});
