import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import { PAID, planOf, stubCloud, t } from "./stub";

const B1 = PAID[0];
const B2 = PAID[1];
const END = Date.UTC(2026, 10, 30, 12);
const GOOD = "123456";
const card = (page: Page, id: string) => page.getByTestId(`subscription-${id}`);
const day = (ms: number) =>
  new Intl.DateTimeFormat("en-US", { dateStyle: "medium", timeZone: "Asia/Kolkata" }).format(ms);

async function open(page: Page) {
  await page.goto("/account/subscription");
  await card(page, B1.id)
    .getByRole("button", { name: t("owner.cancel.open") })
    .click();
}

test("the cancel panel states the policy and what changes on Free", async ({ page }) => {
  await stubCloud(page, PAID, { plans: [planOf(B1), planOf(B2)] });
  await open(page);
  const panel = page.getByTestId(`cancel-plan-${B1.id}`);
  await expect(panel.getByTestId("cancel-policy")).toHaveText(
    t("owner.cancel.policy", { date: day(END) }),
  );
  for (const key of [
    "owner.cancel.free_users",
    "owner.cancel.free_devices",
    "owner.cancel.free_dashboards",
    "owner.cancel.free_backup",
    "owner.cancel.free_ads",
  ] as const)
    await expect(panel.getByText(t(key))).toBeVisible();
  const results = await new AxeBuilder({ page })
    .include(`[data-testid="cancel-plan-${B1.id}"]`)
    .analyze();
  expect(results.violations).toEqual([]);
  await panel.getByRole("button", { name: t("owner.cancel.back") }).click();
  await expect(panel).toBeHidden();
});

test("cancelling asks for a code, then goes again with the same revision", async ({ page }) => {
  let proof = false;
  let plan = planOf(B1);
  const calls = await stubCloud(page, PAID, {
    plans: () => [plan, planOf(B2)],
    cancel: (step) => {
      if (step === "cancel" && !proof)
        return { status: 403, body: { code: "MFA_REQUIRED", message: "Confirm it's you." } };
      plan = planOf(B1, { state: "cancels", rev: 8 });
      return { status: 204, body: null };
    },
  });
  await page.route("**/v1/session/stepup", (route) => {
    proof = route.request().postDataJSON().code === GOOD;
    return route.fulfill({
      status: proof ? 204 : 400,
      body: proof ? "" : '{"code":"TOTP_INVALID"}',
    });
  });
  await open(page);
  await page
    .getByTestId(`cancel-plan-${B1.id}`)
    .getByRole("button", { name: t("owner.cancel.confirm") })
    .click();
  const d = page.getByRole("dialog", { name: t("owner.stepup.title") });
  await d.getByLabel(t("owner.stepup.code")).fill(GOOD);
  await d.getByRole("button", { name: t("owner.stepup.confirm") }).click();
  await expect(d).toBeHidden();

  await expect
    .poll(() => calls.bodies["cancel"])
    .toEqual([
      { step: "cancel", branch: B1.id, rev: 7 },
      { step: "cancel", branch: B1.id, rev: 7 },
    ]);
  // The card reloads: it says when the plan ends and offers to keep it.
  const c = card(page, B1.id);
  await expect(c.locator('[data-component="status-badge"]')).toHaveText(
    t("owner.subscription.state.cancels", { date: day(END) }),
  );
  await expect(c.getByRole("button", { name: t("owner.cancel.keep") })).toBeVisible();
  await expect(c.getByRole("button", { name: t("owner.cancel.open") })).toHaveCount(0);
});

test("declining the code changes nothing", async ({ page }) => {
  const calls = await stubCloud(page, PAID, {
    plans: [planOf(B1), planOf(B2)],
    cancel: () => ({ status: 403, body: { code: "MFA_REQUIRED", message: "no" } }),
  });
  await open(page);
  await page
    .getByTestId(`cancel-plan-${B1.id}`)
    .getByRole("button", { name: t("owner.cancel.confirm") })
    .click();
  const d = page.getByRole("dialog", { name: t("owner.stepup.title") });
  await expect(d).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(d).toBeHidden();
  await expect(
    page.getByRole("alert").filter({ hasText: t("owner.cancel.declined") }),
  ).toBeVisible();
  expect(calls.bodies["cancel"]).toHaveLength(1);
});

test("Keep my plan withdraws the cancellation", async ({ page }) => {
  let plan = planOf(B1, { state: "cancels" });
  const calls = await stubCloud(page, PAID, {
    plans: () => [plan, planOf(B2)],
    cancel: () => {
      plan = planOf(B1, { rev: 9 });
      return { status: 204, body: null };
    },
  });
  await page.goto("/account/subscription");
  const c = card(page, B1.id);
  await c.getByRole("button", { name: t("owner.cancel.keep") }).click();
  expect(calls.bodies["cancel"]).toEqual([{ step: "keep", branch: B1.id, rev: 7 }]);
  await expect(c.locator('[data-component="status-badge"]')).toHaveText(
    t("owner.subscription.state.active"),
  );
  await expect(c.getByRole("button", { name: t("owner.cancel.open") })).toBeVisible();
});

test("a branch whose cancelled plan ended reads Lapsed — Free limits and offers Resubscribe", async ({
  page,
}) => {
  await stubCloud(page, PAID, { plans: [planOf(B1, { state: "lapsed" }), planOf(B2)] });
  await page.goto("/account/subscription");
  const c = card(page, B1.id);
  await expect(c.getByTestId("lapsed-line")).toHaveText(t("owner.subscription.lapsed_line"));
  await expect(c.getByRole("button", { name: t("owner.subscription.resubscribe") })).toBeVisible();
  await expect(c.getByRole("button", { name: t("owner.cancel.open") })).toHaveCount(0);
});

test("a stale plan reloads the card instead of cancelling", async ({ page }) => {
  await stubCloud(page, PAID, {
    plans: [planOf(B1), planOf(B2)],
    cancel: () => ({ status: 409, body: { code: "STALE", message: "moved" } }),
  });
  await open(page);
  await page
    .getByTestId(`cancel-plan-${B1.id}`)
    .getByRole("button", { name: t("owner.cancel.confirm") })
    .click();
  await expect(page.getByText(t("owner.subscription.change.stale"))).toBeVisible();
});
