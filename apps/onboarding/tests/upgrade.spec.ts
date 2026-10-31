import { expect, test, type Page } from "@playwright/test";
import type { OwnerBranch } from "@arena/ui/generated";
import { BRANCH, READY_OWNER, stubCloud, t } from "./stub";

// Story 10.5: an "Upgrade" link opens the 10.4 picker and Checkout for one branch and comes back.

const OWNER = "https://app.arena360.cloud";
const FROM = `${OWNER}/reports?x=1`;
const live = {
  signedIn: true,
  owner: READY_OWNER,
  wizard: { branch: {}, billing: {}, tier: "free" as const },
};
const pay = (page: Page) =>
  page.getByRole("button", { name: t("onboarding.pay.button"), exact: true });
const proceed = (page: Page) =>
  page.getByRole("button", { name: t("onboarding.tier.continue"), exact: true });
const radio = (
  page: Page,
  key: "pricing.basic.name" | "pricing.premium.name" | "pricing.free.name",
) => page.getByRole("radio", { name: t(key), exact: true });
const link = (query: string) => `/upgrade?${query}`;

declare global {
  interface Window {
    __rzp: { opened: number; pay(): void; dismiss(): void };
  }
}

async function stubRazorpay(page: Page) {
  await page.addInitScript(() => {
    type Opts = { handler: (r: unknown) => void; modal: { ondismiss: () => void } };
    let last: Opts | undefined;
    window.__rzp = {
      opened: 0,
      pay: () => last?.handler({ razorpay_payment_id: "pay_1" }),
      dismiss: () => last?.modal.ondismiss(),
    };
    // @ts-expect-error a stand-in for checkout.js
    window.Razorpay = class {
      constructor(options: Opts) {
        last = options;
      }
      on() {}
      open() {
        window.__rzp.opened += 1;
      }
      close() {}
    };
  });
}

/** The Owner app is another origin: the test answers for it and records where the page went. */
async function stubOwner(page: Page): Promise<string[]> {
  const visited: string[] = [];
  await page.route(`${OWNER}/**`, (route) => {
    visited.push(route.request().url());
    return route.fulfill({ body: "<h1>owner</h1>", contentType: "text/html" });
  });
  return visited;
}

test("the lock's tier is preselected, free is not offered and the picker needs no wizard", async ({
  page,
}) => {
  await stubCloud(page, live);
  await page.goto(link(`branch=${BRANCH.id}&tier=premium&return=${encodeURIComponent(FROM)}`));
  await expect(
    page.getByRole("heading", { level: 1, name: t("onboarding.upgrade.title") }),
  ).toBeVisible();
  await expect(
    page.getByText(t("onboarding.upgrade.intro", { branch: BRANCH.name })),
  ).toBeVisible();
  await expect(radio(page, "pricing.premium.name")).toBeChecked();
  await expect(radio(page, "pricing.basic.name")).not.toBeChecked();
  await expect(radio(page, "pricing.free.name")).toHaveCount(0);
  await expect(page.getByTestId("wizard-announcement")).toHaveCount(0);
});

test("paying returns to the page the link came from once the entitlement is written", async ({
  page,
}) => {
  const cloud = await stubCloud(page, live);
  await stubRazorpay(page);
  const visited = await stubOwner(page);
  await page.goto(link(`branch=${BRANCH.id}&tier=basic&return=${encodeURIComponent(FROM)}`));
  await proceed(page).click();
  await expect(page.getByTestId("summary")).toBeVisible();
  await pay(page).click();
  await expect.poll(() => page.evaluate(() => window.__rzp.opened)).toBe(1);
  expect(cloud.subscribeBodies).toHaveLength(1);
  await page.evaluate(() => window.__rzp.pay());
  await expect(page.getByTestId("confirming")).toHaveText(t("onboarding.pay.confirming"));
  // The 10.3 webhook writes the entitlement; the page notices on its next poll.
  cloud.tier = "basic";
  await expect(page).toHaveURL(FROM, { timeout: 15_000 });
  expect(visited).toContain(FROM);
});

test("a branch whose wizard is past the plan step still returns, not into the wizard", async ({
  page,
}) => {
  const cloud = await stubCloud(page, {
    ...live,
    wizard: { ...live.wizard, done: ["cafe", "tier"] },
  });
  await stubRazorpay(page);
  const visited = await stubOwner(page);
  await page.goto(link(`branch=${BRANCH.id}&tier=basic&return=${encodeURIComponent(FROM)}`));
  await proceed(page).click();
  await pay(page).click();
  await expect.poll(() => page.evaluate(() => window.__rzp.opened)).toBe(1);
  await page.evaluate(() => window.__rzp.pay());
  await expect(page.getByTestId("confirming")).toBeVisible();
  await expect(page).toHaveURL(/\/upgrade\?/);
  cloud.tier = "basic";
  await expect(page).toHaveURL(FROM, { timeout: 15_000 });
  expect(visited).toContain(FROM);
});

test("closing Checkout keeps the owner on the summary with nothing changed, and Back leaves", async ({
  page,
}) => {
  const cloud = await stubCloud(page, live);
  await stubRazorpay(page);
  const visited = await stubOwner(page);
  await page.goto(link(`branch=${BRANCH.id}&tier=basic&return=${encodeURIComponent(FROM)}`));
  await proceed(page).click();
  await pay(page).click();
  await expect.poll(() => page.evaluate(() => window.__rzp.opened)).toBe(1);
  await page.evaluate(() => window.__rzp.dismiss());
  await expect(page.getByTestId("summary")).toBeVisible();
  expect(cloud.tier).toBe("free");
  await page.getByRole("button", { name: t("onboarding.upgrade.back") }).click();
  await expect(page).toHaveURL(FROM);
  expect(visited).toEqual([FROM]);
});

test("a branch that already has a subscription opens Account -> Subscription, not a checkout", async ({
  page,
}) => {
  const cloud = await stubCloud(page, live);
  cloud.existing = {
    subscription_id: "sb_1",
    state: "active",
    tier: "basic",
    cycle: "monthly",
    band: "IN",
    current_period_end: null,
  } as typeof cloud.existing;
  const visited = await stubOwner(page);
  await page.goto(link(`branch=${BRANCH.id}&tier=premium`));
  const to = `${OWNER}/account/subscription?branch=${BRANCH.id}`;
  await expect(page).toHaveURL(to);
  expect(visited).toEqual([to]);
  expect(cloud.subscribeBodies).toEqual([]);
});

test("an unpaid pending subscription still opens the picker", async ({ page }) => {
  const cloud = await stubCloud(page, live);
  cloud.existing = {
    subscription_id: "sb_1",
    state: "pending",
    tier: "basic",
    cycle: "monthly",
    band: "IN",
    current_period_end: null,
  } as typeof cloud.existing;
  await page.goto(link(`branch=${BRANCH.id}&tier=basic`));
  await expect(radio(page, "pricing.basic.name")).toBeChecked();
});

test("a branch the owner doesn't own is refused and no checkout opens", async ({ page }) => {
  const cloud = await stubCloud(page, live);
  cloud.forbiddenBranch = "01JSTRANGER000000000000000";
  await stubRazorpay(page);
  await page.goto(link("branch=01JSTRANGER000000000000000&tier=basic"));
  await expect(page.getByRole("alert")).toHaveText(t("onboarding.upgrade.forbidden"));
  await expect(proceed(page)).toHaveCount(0);
  expect(await page.evaluate(() => window.__rzp.opened)).toBe(0);
  expect(cloud.subscribeBodies).toEqual([]);
});

test("a return link to another site is ignored: the owner lands on the Owner app", async ({
  page,
}) => {
  await stubCloud(page, live);
  await stubRazorpay(page);
  const visited = await stubOwner(page);
  await page.goto(
    link(`branch=${BRANCH.id}&tier=basic&return=${encodeURIComponent("https://evil.example/x")}`),
  );
  await page
    .getByRole("button", { name: t("onboarding.upgrade.back") })
    .first()
    .click();
  await expect(page).toHaveURL(`${OWNER}/`);
  expect(visited).toEqual([`${OWNER}/`]);
});

test("without a branch the only Free branch is used", async ({ page }) => {
  const cloud = await stubCloud(page, live);
  cloud.branchList = [{ id: BRANCH.id, name: BRANCH.name, tier: "free" } as OwnerBranch];
  await page.goto(link("tier=basic"));
  await expect(radio(page, "pricing.basic.name")).toBeChecked();
});

test("without a branch and with several Free ones the owner picks which", async ({ page }) => {
  const cloud = await stubCloud(page, live);
  cloud.branchList = [
    { id: BRANCH.id, name: "North", tier: "free" } as OwnerBranch,
    { id: "01JOTHER00000000000000000", name: "South", tier: "free" } as OwnerBranch,
    { id: "01JPAID000000000000000000", name: "East", tier: "basic" } as OwnerBranch,
  ];
  await page.goto(link("tier=basic"));
  await expect(page.getByText(t("onboarding.upgrade.pick_branch"))).toBeVisible();
  await expect(page.getByRole("button")).toHaveCount(2);
  await page
    .getByRole("button", { name: t("onboarding.upgrade.pick_button", { branch: "North" }) })
    .click();
  await expect(radio(page, "pricing.basic.name")).toBeChecked();
});
