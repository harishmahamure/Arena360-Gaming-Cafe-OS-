import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import { PAID, planOf, stubCloud, t } from "./stub";

const B1 = PAID[0];
const B2 = PAID[1];
const GOOD = "123456";
const UNTIL = Date.UTC(2026, 10, 20, 12);
const OFFER = { amount_minor: 176_882, currency: "INR", until: UNTIL };
const AMOUNT = "₹1,768.82";
const card = (page: Page, id: string) => page.getByTestId(`subscription-${id}`);

async function open(page: Page) {
  await page.goto("/account/subscription");
  await card(page, B1.id)
    .getByRole("button", { name: t("owner.cancel.open") })
    .click();
}

test("inside 7 days the cancel panel offers a full refund and says the branch moves to Free at once", async ({
  page,
}) => {
  await stubCloud(page, PAID, { plans: [planOf(B1, { refund_offer: OFFER }), planOf(B2)] });
  await open(page);
  const panel = page.getByTestId(`cancel-plan-${B1.id}`);
  const offer = panel.getByTestId("refund-offer");
  await expect(offer).toContainText(t("owner.refund.policy", { amount: AMOUNT }));
  await expect(offer).toContainText("Free at once");
  await expect(
    offer.getByRole("button", { name: t("owner.refund.confirm", { amount: AMOUNT }) }),
  ).toBeVisible();
  // The Story 10.14 cancel stays beside it.
  await expect(panel.getByRole("button", { name: t("owner.cancel.confirm") })).toBeVisible();
  const results = await new AxeBuilder({ page })
    .include(`[data-testid="cancel-plan-${B1.id}"]`)
    .analyze();
  expect(results.violations).toEqual([]);
});

test("after 7 days the panel shows only the cancel flow", async ({ page }) => {
  await stubCloud(page, PAID, { plans: [planOf(B1), planOf(B2)] });
  await open(page);
  const panel = page.getByTestId(`cancel-plan-${B1.id}`);
  await expect(panel.getByTestId("refund-offer")).toHaveCount(0);
  await expect(panel.getByTestId("cancel-policy")).toBeVisible();
});

test("asking for a refund goes through a code and leaves a requested note", async ({ page }) => {
  let proof = false;
  let plan = planOf(B1, { refund_offer: OFFER });
  const calls = await stubCloud(page, PAID, {
    plans: () => [plan, planOf(B2)],
    refund: () => {
      if (!proof)
        return { status: 403, body: { code: "MFA_REQUIRED", message: "Confirm it's you." } };
      plan = planOf(B1, {
        rev: 8,
        refund: { status: "requested", amount_minor: 176_882, currency: "INR", requested_at: 1 },
      });
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
    .getByRole("button", { name: t("owner.refund.confirm", { amount: AMOUNT }) })
    .click();
  const d = page.getByRole("dialog", { name: t("owner.stepup.title") });
  await d.getByLabel(t("owner.stepup.code")).fill(GOOD);
  await d.getByRole("button", { name: t("owner.stepup.confirm") }).click();
  await expect(d).toBeHidden();

  await expect
    .poll(() => calls.bodies["refund"])
    .toEqual([
      { branch: B1.id, rev: 7, amount_minor: 176_882 },
      { branch: B1.id, rev: 7, amount_minor: 176_882 },
    ]);
  const c = card(page, B1.id);
  await expect(c.getByTestId("refund-note")).toHaveText(t("owner.refund.requested"));
  // Nothing else can change while the refund is on its way.
  await expect(c.getByRole("button", { name: t("owner.cancel.open") })).toHaveCount(0);
  await expect(c.getByRole("button", { name: t("owner.subscription.change.open") })).toHaveCount(0);
});

test("declining the code changes nothing", async ({ page }) => {
  const calls = await stubCloud(page, PAID, {
    plans: [planOf(B1, { refund_offer: OFFER }), planOf(B2)],
    refund: () => ({ status: 403, body: { code: "MFA_REQUIRED", message: "no" } }),
  });
  await open(page);
  await page
    .getByTestId(`cancel-plan-${B1.id}`)
    .getByRole("button", { name: t("owner.refund.confirm", { amount: AMOUNT }) })
    .click();
  const d = page.getByRole("dialog", { name: t("owner.stepup.title") });
  await expect(d).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(d).toBeHidden();
  await expect(
    page.getByRole("alert").filter({ hasText: t("owner.cancel.declined") }),
  ).toBeVisible();
  expect(calls.bodies["refund"]).toHaveLength(1);
});

test("a refund the cloud refuses is explained and the cancel flow remains", async ({ page }) => {
  await stubCloud(page, PAID, {
    plans: [planOf(B1, { refund_offer: OFFER }), planOf(B2)],
    refund: () => ({ status: 409, body: { code: "REFUND_NOT_ALLOWED", message: "late" } }),
  });
  await open(page);
  const panel = page.getByTestId(`cancel-plan-${B1.id}`);
  await panel.getByRole("button", { name: t("owner.refund.confirm", { amount: AMOUNT }) }).click();
  await expect(
    page.getByRole("alert").filter({ hasText: t("owner.refund.not_allowed") }),
  ).toBeVisible();
  await expect(panel.getByRole("button", { name: t("owner.cancel.confirm") })).toBeVisible();
});

test("the card reads issued, processed and rejected refunds", async ({ page }) => {
  const refund = (status: "issued" | "processed" | "rejected", reason?: string) => ({
    status,
    amount_minor: 176_882,
    currency: "INR",
    requested_at: 1,
    reason,
  });
  await stubCloud(page, PAID, {
    plans: [
      planOf(B1, { refund: refund("issued") }),
      // A refunded plan leaves the branch on Free; the card still says so.
      planOf(B2, { refund: refund("processed") }),
    ],
  });
  await page.goto("/account/subscription");
  await expect(card(page, B1.id).getByTestId("refund-note")).toHaveText(t("owner.refund.issued"));
  await expect(card(page, B2.id).getByTestId("refund-note")).toHaveText(
    t("owner.refund.processed", { amount: AMOUNT }),
  );
});

test("a rejected refund shows the operator's reason and the plan stays active", async ({
  page,
}) => {
  await stubCloud(page, PAID, {
    plans: [
      planOf(B1, {
        refund: {
          status: "rejected",
          amount_minor: 176_882,
          currency: "INR",
          requested_at: 1,
          reason: "Not a first payment",
        },
      }),
      planOf(B2),
    ],
  });
  await page.goto("/account/subscription");
  const c = card(page, B1.id);
  await expect(c.getByTestId("refund-note")).toHaveText(
    t("owner.refund.rejected", { reason: "Not a first payment" }),
  );
  await expect(c.locator('[data-component="status-badge"]')).toHaveText(
    t("owner.subscription.state.active"),
  );
  await expect(c.getByRole("button", { name: t("owner.cancel.open") })).toBeVisible();
});
