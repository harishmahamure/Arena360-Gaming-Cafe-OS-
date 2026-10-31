import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";
import { PAID, planOf, stubCloud, t } from "./stub";

const B1 = PAID[0];
const B2 = PAID[1];
const RENEWAL = Date.UTC(2026, 10, 30, 12);
const GRACE_END = Date.UTC(2026, 11, 7, 12);
const day = (ms: number) =>
  new Intl.DateTimeFormat("en-US", { dateStyle: "medium", timeZone: "Asia/Kolkata" }).format(ms);

const needsApproval = () => planOf(B1, { state: "needs_approval", current_period_end: RENEWAL });
const inGrace = () =>
  planOf(B1, { state: "grace", current_period_end: RENEWAL, grace_until: GRACE_END });

test("a renewal that needs approval shows a banner on every screen that names the branch and the date", async ({
  page,
}) => {
  await stubCloud(page, PAID, { plans: [needsApproval(), planOf(B2)] });
  for (const path of ["/", "/alerts", "/account/subscription"]) {
    await page.goto(path);
    const banner = page.getByTestId("renewal-banner");
    await expect(banner).toContainText(
      t("owner.renewal.banner.needs_approval", { branch: B1.name, date: day(RENEWAL) }),
    );
    // Only the branch that needs it is named.
    await expect(banner).not.toContainText(B2.name);
    const link = banner.getByRole("link", {
      name: t("owner.renewal.banner.approve_for", { branch: B1.name }),
    });
    await expect(link).toHaveText(t("owner.renewal.banner.approve"));
    await expect(link).toHaveAttribute("href", `/account/subscription?branch=${B1.id}`);
  }
});

test("a branch in grace names the day grace ends and offers to update the payment method", async ({
  page,
}) => {
  await stubCloud(page, PAID, { plans: [inGrace(), planOf(B2)] });
  await page.goto("/reports");
  const banner = page.getByTestId("renewal-banner");
  await expect(banner).toContainText(
    t("owner.renewal.banner.grace", { branch: B1.name, date: day(GRACE_END) }),
  );
  await banner
    .getByRole("link", { name: t("owner.renewal.banner.update_for", { branch: B1.name }) })
    .click();
  await expect(page).toHaveURL(
    (u) => u.pathname + u.search === `/account/subscription?branch=${B1.id}`,
  );
  const card = page.getByTestId(`subscription-${B1.id}`);
  await expect(card.getByTestId("unpaid-note")).toHaveText(
    t("owner.subscription.unpaid.grace", { date: day(GRACE_END) }),
  );
  await expect(card.locator('[data-component="status-badge"]')).toHaveText(
    t("owner.subscription.state.grace"),
  );
});

test("while a renewal is unpaid Change plan stays shut but Change payment method is open", async ({
  page,
}) => {
  await stubCloud(page, PAID, { plans: [needsApproval(), planOf(B2)] });
  await page.goto("/account/subscription");
  const card = page.getByTestId(`subscription-${B1.id}`);
  await expect(card.getByTestId("unpaid-note")).toHaveText(
    t("owner.subscription.unpaid.needs_approval", { date: day(RENEWAL) }),
  );
  await expect(
    card.getByRole("button", { name: t("owner.subscription.change.open") }),
  ).toBeDisabled();
  await expect(
    card.getByRole("button", { name: t("owner.subscription.method.open") }),
  ).toBeEnabled();
  // The cycle switch is not offered either.
  await expect(card.getByRole("button", { name: /Switch to/ })).toHaveCount(0);
});

test("no banner when every renewal is in order, and the banner passes the accessibility check", async ({
  page,
}) => {
  await stubCloud(page);
  await page.goto("/");
  await expect(page.getByRole("heading", { level: 1 })).toBeVisible();
  await expect(page.getByTestId("renewal-banner")).toHaveCount(0);

  await stubCloud(page, PAID, { plans: [inGrace(), planOf(B2)] });
  await page.goto("/account/subscription");
  await expect(page.getByTestId("renewal-banner")).toBeVisible();
  const results = await new AxeBuilder({ page }).analyze();
  expect(results.violations).toEqual([]);
});
