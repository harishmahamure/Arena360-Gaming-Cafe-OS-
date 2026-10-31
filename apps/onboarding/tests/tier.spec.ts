import { expect, test, type Page } from "@playwright/test";
import { PRICING } from "../../../packages/ui/src/pricing";
import { READY_OWNER, stubCloud, t } from "./stub";

type Key = Parameters<typeof t>[0];
const h1 = (page: Page, key: Key) => page.getByRole("heading", { level: 1, name: t(key) });
const isWide = (page: Page) => (page.viewportSize()?.width ?? 0) >= 840;
const created = { signedIn: true, owner: READY_OWNER, wizard: { branch: {} } };
const radio = (page: Page, key: Key) => page.getByRole("radio", { name: t(key), exact: true });
const proceed = (page: Page) =>
  page.getByRole("button", { name: t("onboarding.tier.continue"), exact: true });

const PRICES = {
  monthly: {
    free: t("pricing.price.month", { price: "₹0" }),
    basic: t("pricing.price.branch_month", { price: "₹200" }),
    premium: t("pricing.price.month", { price: "₹1,999" }),
  },
  yearly: {
    free: t("pricing.price.year", { price: "₹0" }),
    basic: t("pricing.price.branch_year", { price: "₹2,000" }),
    premium: t("pricing.price.year", { price: "₹19,990" }),
  },
} as const;

test("cards come from the pricing table and follow the billing period", async ({ page }) => {
  await stubCloud(page, created);
  await page.goto("/setup/tier");
  await expect(h1(page, "ui.wizard.step.tier")).toBeVisible();
  const tabs = page.getByRole("tablist", { name: t("onboarding.tier.period") });
  await expect(tabs.getByRole("tab", { name: t("onboarding.tier.monthly") })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  for (const plan of PRICING) {
    const card = page.getByTestId(`plan-${plan.key}`);
    await expect(card).toContainText(t(plan.name));
    await expect(page.getByTestId(`price-${plan.key}`)).toHaveText(PRICES.monthly[plan.key]);
    for (const f of plan.features) await expect(card).toContainText(t(f));
    if (plan.gstPercent > 0) {
      await expect(page.getByTestId(`gst-${plan.key}`)).toHaveText(
        t("pricing.gst", { percent: 18 }),
      );
    } else {
      await expect(page.getByTestId(`gst-${plan.key}`)).toHaveCount(0);
    }
  }
  await expect(page.getByTestId("plan-free")).toContainText(
    t("onboarding.tier.devices", { n: 20 }),
  );
  await expect(page.getByTestId("plan-premium")).toContainText(
    t("onboarding.tier.devices", { n: 200 }),
  );
  await tabs.getByRole("tab", { name: t("onboarding.tier.yearly") }).click();
  for (const plan of PRICING) {
    await expect(page.getByTestId(`price-${plan.key}`)).toHaveText(PRICES.yearly[plan.key]);
  }

  const boxes = await Promise.all(
    PRICING.map((p) => page.getByTestId(`plan-${p.key}`).boundingBox()),
  );
  const heights = boxes.map((b) => Math.round(b!.height));
  expect(new Set(heights).size, heights.join(",")).toBe(1);
  if (isWide(page)) {
    expect(new Set(boxes.map((b) => Math.round(b!.y))).size).toBe(1);
  } else {
    expect(new Set(boxes.map((b) => Math.round(b!.x))).size).toBe(1);
    expect(boxes[1]!.y).toBeGreaterThan(boxes[0]!.y);
  }
});

test("paid plans are disabled with their reason inline", async ({ page }) => {
  await stubCloud(page, created);
  await page.goto("/setup/tier");
  await expect(radio(page, "pricing.free.name")).toBeChecked();
  await expect(radio(page, "pricing.free.name")).toBeEnabled();
  for (const key of ["basic", "premium"] as const) {
    await expect(radio(page, `pricing.${key}.name`)).toBeDisabled();
    await expect(page.getByTestId(`plan-${key}`)).toContainText(
      t("onboarding.tier.reason.billing_disabled"),
    );
  }
  await expect(proceed(page)).not.toHaveAttribute("aria-disabled", "true");
});

test("one Free branch per account: free is disabled and continue says why", async ({ page }) => {
  const cloud = await stubCloud(page, { ...created, wizard: { branch: {}, otherFree: true } });
  await page.goto("/setup/tier");
  await expect(radio(page, "pricing.free.name")).toBeDisabled();
  await expect(page.getByTestId("plan-free")).toContainText(
    t("onboarding.tier.reason.one_free_branch"),
  );
  await expect(proceed(page)).toHaveAttribute("aria-disabled", "true");
  await expect(page.getByText(t("onboarding.tier.choose_first"))).toBeVisible();
  await proceed(page).focus();
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\/setup\/tier$/);
  expect(cloud.tierPicks).toEqual([]);
});

test("free continues to step 3, announcing it and focusing its heading", async ({ page }) => {
  const cloud = await stubCloud(page, created);
  await page.goto("/setup/tier");
  await expect(radio(page, "pricing.free.name")).toBeChecked();
  const live = page.getByTestId("wizard-announcement");
  await expect(live).toHaveText("");
  await proceed(page).click();
  await expect(page).toHaveURL(/\/setup\/install_hub$/);
  await expect(h1(page, "ui.wizard.step.install_hub")).toBeFocused();
  await expect(live).toHaveText(
    t("onboarding.setup.announce", { n: 3, total: 10, name: t("ui.wizard.step.install_hub") }),
  );
  expect(cloud.tierPicks).toEqual(["free"]);
  expect(cloud.tierBodies).toEqual([{ tier: "free", branch_id: cloud.branch!.id }]);
  expect(cloud.done.has("tier")).toBe(true);
});

test("a done plan step shows the current plan and continuing changes nothing", async ({ page }) => {
  const cloud = await stubCloud(page, {
    ...created,
    wizard: { branch: {}, done: ["cafe", "tier"] },
  });
  await page.goto("/setup/tier");
  await expect(page.getByTestId("plan-free")).toContainText(t("onboarding.tier.current"));
  await proceed(page).click();
  await expect(page).toHaveURL(/\/setup\/install_hub$/);
  expect(cloud.tierPicks).toEqual(["free"]);
  await expect(page.getByTestId("wizard-announcement")).toHaveText("");
});

test("a refused branch sends the owner to the current step", async ({ page }) => {
  const cloud = await stubCloud(page, created);
  await page.route("**/v1/wizard/tier", async (route) => {
    cloud.done.delete("cafe");
    cloud.branch = null;
    await route.fulfill({
      status: 403,
      contentType: "application/json",
      body: JSON.stringify({ code: "FORBIDDEN", message: "stub" }),
    });
  });
  await page.goto("/setup/tier");
  await expect(page).toHaveURL(/\/setup\/cafe$/);
  await expect(h1(page, "ui.wizard.step.cafe")).toBeVisible();
});

const billingOn = { ...created, wizard: { branch: {}, billing: {} } };

test("billing on: a paid plan can be picked, shows its cycle price and GST, and enables Continue", async ({
  page,
}) => {
  const cloud = await stubCloud(page, billingOn);
  await page.goto("/setup/tier");
  await expect(h1(page, "ui.wizard.step.tier")).toBeVisible();
  for (const key of ["basic", "premium"] as const) {
    await expect(radio(page, `pricing.${key}.name`)).toBeEnabled();
  }
  await expect(page.getByTestId("addons")).toHaveCount(0);

  await radio(page, "pricing.basic.name").check();
  await expect(proceed(page)).not.toHaveAttribute("aria-disabled", "true");
  await expect(page.getByTestId("price-basic")).toHaveText(PRICES.monthly.basic);
  await expect(page.getByTestId("gst-basic")).toHaveText(t("pricing.gst", { percent: 18 }));
  await expect(page.getByTestId("saving-basic")).toHaveCount(0);

  await page.getByRole("tab", { name: t("onboarding.tier.yearly") }).click();
  await expect(page.getByTestId("price-basic")).toHaveText(PRICES.yearly.basic);
  await expect(page.getByTestId("saving-basic")).toHaveText(t("onboarding.tier.yearly_saving"));
  await expect(page.getByTestId("saving-premium")).toHaveText(t("onboarding.tier.yearly_saving"));
  await expect(page.getByTestId("saving-free")).toHaveCount(0);

  // Nothing is chosen through the plan endpoint: a paid plan is bought through checkout.
  expect(cloud.tierPicks).toEqual([]);
});

test("billing on: the Add-ons panel offers packs of 20 devices, Tunnel and a locked Owner login", async ({
  page,
}) => {
  await stubCloud(page, billingOn);
  await page.goto("/setup/tier");
  await radio(page, "pricing.basic.name").check();
  const panel = page.getByTestId("addons");
  await expect(panel).toBeVisible();
  await expect(panel).toContainText(t("onboarding.addons.title"));
  const packs = page.getByLabel(t("onboarding.addons.devices"));
  await expect(packs).toHaveValue("0");
  await expect(packs).toHaveAttribute("min", "0");
  await expect(packs).toHaveAttribute("max", "10");
  await packs.fill("25");
  await packs.blur();
  await expect(packs).toHaveValue("10");
  await packs.fill("3");
  await packs.blur();
  await expect(packs).toHaveValue("3");

  const tunnel = page.getByRole("checkbox", { name: t("onboarding.addons.tunnel") });
  await expect(tunnel).toBeEnabled();
  await tunnel.check();
  const login = page.getByRole("checkbox", { name: t("onboarding.addons.owner_login") });
  await expect(login).toBeDisabled();
  await expect(page.getByTestId("owner-login-soon")).toHaveText(
    t("onboarding.addons.owner_login_soon"),
  );

  // Premium includes Tunnel: no checkbox, and a Tunnel picked on Basic is dropped.
  await radio(page, "pricing.premium.name").check();
  await expect(page.getByTestId("tunnel-included")).toHaveText(
    t("onboarding.addons.tunnel_included"),
  );
  await expect(page.getByRole("checkbox", { name: t("onboarding.addons.tunnel") })).toHaveCount(0);
  await radio(page, "pricing.basic.name").check();
  await expect(tunnel).not.toBeChecked();
  await expect(page.getByLabel(t("onboarding.addons.devices"))).toHaveValue("3");
});

test("billing on, band off sale or without one: paid plans say so and Free still continues", async ({
  page,
}) => {
  for (const billing of [{ onSale: false, band: "MY" as const }, { band: null }]) {
    const cloud = await stubCloud(page, { ...created, wizard: { branch: {}, billing } });
    await page.goto("/setup/tier");
    await expect(radio(page, "pricing.free.name")).toBeEnabled();
    for (const key of ["basic", "premium"] as const) {
      await expect(radio(page, `pricing.${key}.name`)).toBeDisabled();
      await expect(page.getByTestId(`plan-${key}`)).toContainText(t("pricing.band_unavailable"));
    }
    await expect(page.getByTestId("addons")).toHaveCount(0);
    await proceed(page).click();
    await expect(page).toHaveURL(/\/setup\/install_hub$/);
    expect(cloud.tierPicks).toEqual(["free"]);
    await page.unrouteAll({ behavior: "ignoreErrors" });
  }
});

test("billing on: Free keeps its one-click Continue", async ({ page }) => {
  const cloud = await stubCloud(page, billingOn);
  await page.goto("/setup/tier");
  await expect(radio(page, "pricing.free.name")).toBeChecked();
  await proceed(page).click();
  await expect(page).toHaveURL(/\/setup\/install_hub$/);
  await expect(h1(page, "ui.wizard.step.install_hub")).toBeFocused();
  expect(cloud.tierPicks).toEqual(["free"]);
  expect(cloud.quoteBodies).toEqual([]);
});
