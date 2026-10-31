import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type {
  BranchPlan,
  ChangeConfirmed,
  CycleSwitchPreview,
  SubscriptionCheckout,
} from "@arena/ui/generated";
import { PAID, planOf, stubCloud, t } from "./stub";

const B1 = PAID[0];
const card = (page: Page, id: string) => page.getByTestId(`subscription-${id}`);
const day = (ms: number) =>
  new Intl.DateTimeFormat("en-US", { dateStyle: "medium", timeZone: "Asia/Kolkata" }).format(ms);
const RENEWS = Date.UTC(2026, 10, 30, 12);
const YEAR_OUT = Date.UTC(2027, 10, 15, 12);

const toYearly = (over: Partial<CycleSwitchPreview> = {}): CycleSwitchPreview => ({
  rev: 7,
  kind: "now",
  currency: "INR",
  cycle: "yearly",
  gst_percent: 18,
  cycle_amount_minor: 1_499_000,
  cycle_tax_minor: 269_820,
  cycle_total_minor: 1_768_820,
  credit_total_minor: 90_000,
  days_left: 12,
  charge_amount_minor: 1_422_000,
  charge_tax_minor: 255_960,
  charge_total_minor: 1_677_960,
  effective_at: Date.UTC(2026, 10, 18),
  renews_at: YEAR_OUT,
  needs_mandate: true,
  methods: ["card", "emandate"],
  ...over,
});

const checkout = (cycle: "monthly" | "yearly"): SubscriptionCheckout => ({
  subscription_id: "sb1",
  state: "active",
  key_id: "rzp_test",
  razorpay_subscription_id: "sub_new",
  band: "IN",
  currency: "INR",
  tier: "basic",
  cycle,
  methods: ["card", "emandate"],
  amount_minor: 1_499_000,
  tax_minor: 269_820,
  total_minor: 1_768_820,
});

const confirmed = (over: Partial<ChangeConfirmed> = {}): ChangeConfirmed => ({
  kind: "now",
  effective_at: 1,
  charge_total_minor: 1_677_960,
  renewal_total_minor: 1_768_820,
  checkout: checkout("yearly"),
  ...over,
});

/** A Razorpay that opens at once and then either pays or is closed. */
const fakeCheckout = (page: Page, outcome: "paid" | "closed") =>
  page.addInitScript((how) => {
    class Fake {
      constructor(
        private options: { handler: (r: unknown) => void; modal: { ondismiss: () => void } },
      ) {}
      on() {}
      open() {
        (window as unknown as { __opened: unknown }).__opened = this.options;
        if (how === "paid") this.options.handler({});
        else this.options.modal.ondismiss();
      }
      close() {}
    }
    (window as unknown as { Razorpay: unknown }).Razorpay = Fake;
  }, outcome);

const lastStatus = (page: Page) => page.getByRole("status").filter({ hasText: /./ }).last();

test("switching to yearly shows the price, the credit for unused days, the net today and the renewal a year out", async ({
  page,
}) => {
  await fakeCheckout(page, "paid");
  const calls = await stubCloud(page, PAID, {
    plans: [planOf(B1), planOf(PAID[1])],
    cycle: (step) =>
      step === "preview" ? { status: 200, body: toYearly() } : { status: 200, body: confirmed() },
  });
  await page.goto("/account/subscription");
  await card(page, "b1")
    .getByRole("button", { name: t("owner.subscription.cycle.open_yearly") })
    .click();
  const form = page.getByTestId("switch-cycle-b1");
  await expect(form.getByTestId("cycle-price")).toHaveText(
    t("owner.subscription.change.charge_gst", { amount: "₹14,990.00", gst: "₹2,698.20" }),
  );
  await expect(form.getByTestId("cycle-credit")).toHaveText("−₹900.00");
  await expect(form.getByTestId("cycle-net")).toHaveText(
    t("owner.subscription.change.charge_gst", { amount: "₹14,220.00", gst: "₹2,559.60" }),
  );
  await expect(form.getByTestId("cycle-renews")).toHaveText(day(YEAR_OUT));
  await expect(form.getByTestId("cycle-mandate")).toHaveText(t("owner.subscription.cycle.mandate"));
  expect(calls.bodies["cycle"]).toEqual([
    { step: "preview", branch: "b1", cycle: "yearly", rev: 7 },
  ]);

  await form.getByRole("button", { name: t("owner.subscription.cycle.confirm") }).click();
  // Checkout opened for the new subscription, with only the methods the cloud allows.
  await page.waitForFunction(() => (window as unknown as { __opened?: unknown }).__opened);
  const options = (await page.evaluate(
    () => (window as unknown as { __opened: unknown }).__opened,
  )) as {
    subscription_id: string;
    config: { display: { blocks: { allowed: { instruments: { method: string }[] } } } };
  };
  expect(options.subscription_id).toBe("sub_new");
  expect(options.config.display.blocks.allowed.instruments.map((i) => i.method)).toEqual([
    "card",
    "emandate",
  ]);
  expect(calls.bodies["cycle"]?.at(-1)).toEqual({
    step: "confirm",
    branch: "b1",
    cycle: "yearly",
    rev: 7,
  });
  await expect(page.getByTestId("switch-cycle-b1")).toHaveCount(0);
  await expect(lastStatus(page)).toHaveText(t("owner.subscription.cycle.sent_yearly"));
});

test("switching to monthly is scheduled for the end of the paid year with no partial refund", async ({
  page,
}) => {
  await fakeCheckout(page, "paid");
  const yearly = planOf(B1, { cycle: "yearly" });
  const calls = await stubCloud(page, PAID, {
    plans: [yearly, planOf(PAID[1])],
    cycle: (step) =>
      step === "preview"
        ? {
            status: 200,
            body: toYearly({
              kind: "period_end",
              cycle: "monthly",
              cycle_amount_minor: 149_900,
              cycle_tax_minor: 26_982,
              cycle_total_minor: 176_882,
              credit_total_minor: 0,
              days_left: 0,
              charge_amount_minor: 0,
              charge_tax_minor: 0,
              charge_total_minor: 0,
              effective_at: RENEWS,
              renews_at: Date.UTC(2026, 11, 30, 12),
              notice: undefined,
            }),
          }
        : {
            status: 200,
            body: confirmed({
              kind: "period_end",
              charge_total_minor: 0,
              renewal_total_minor: 176_882,
              checkout: checkout("monthly"),
            }),
          },
  });
  await page.goto("/account/subscription");
  await card(page, "b1")
    .getByRole("button", { name: t("owner.subscription.cycle.open_monthly") })
    .click();
  const form = page.getByTestId("switch-cycle-b1");
  await expect(form.getByTestId("cycle-later")).toHaveText(
    t("owner.subscription.cycle.scheduled", { date: day(RENEWS) }),
  );
  // Nothing is charged or credited today.
  await expect(form.getByTestId("cycle-net")).toHaveCount(0);
  await expect(form.getByTestId("cycle-credit")).toHaveCount(0);
  await form.getByRole("button", { name: t("owner.subscription.cycle.confirm") }).click();
  expect(calls.bodies["cycle"]?.at(-1)).toMatchObject({ step: "confirm", cycle: "monthly" });
  await expect(lastStatus(page)).toHaveText(
    t("owner.subscription.cycle.sent_monthly", { date: day(RENEWS) }),
  );
});

test("closing Checkout takes the switch back and the old cycle stays", async ({ page }) => {
  await fakeCheckout(page, "closed");
  const calls = await stubCloud(page, PAID, {
    plans: [planOf(B1), planOf(PAID[1])],
    cycle: (step) =>
      step === "preview" ? { status: 200, body: toYearly() } : { status: 200, body: confirmed() },
    change: (step) =>
      step === "withdraw" ? { status: 204, body: null } : { status: 404, body: { code: "x" } },
  });
  await page.goto("/account/subscription");
  await card(page, "b1")
    .getByRole("button", { name: t("owner.subscription.cycle.open_yearly") })
    .click();
  await page
    .getByTestId("switch-cycle-b1")
    .getByRole("button", { name: t("owner.subscription.cycle.confirm") })
    .click();
  await expect(lastStatus(page)).toHaveText(t("owner.subscription.cycle.dismissed"));
  // Storing the switch moved the revision on by one, so the withdrawal carries the next one.
  expect(calls.bodies["change"]).toEqual([{ step: "withdraw", branch: "b1", rev: 8 }]);
  await expect(card(page, "b1").getByTestId("pending-change")).toHaveCount(0);
});

test("Change payment method opens Checkout for a new mandate, and closing it says the method is unchanged", async ({
  page,
}) => {
  await fakeCheckout(page, "closed");
  const calls = await stubCloud(page, PAID, {
    plans: [planOf(B1), planOf(PAID[1])],
    method: () => ({
      status: 200,
      body: confirmed({ kind: "period_end", charge_total_minor: 0, checkout: checkout("monthly") }),
    }),
    change: (step) =>
      step === "withdraw" ? { status: 204, body: null } : { status: 404, body: { code: "x" } },
  });
  await page.goto("/account/subscription");
  await card(page, "b1")
    .getByRole("button", { name: t("owner.subscription.method.open") })
    .click();
  await expect(lastStatus(page)).toHaveText(t("owner.subscription.method.unchanged"));
  expect(calls.bodies["method"]).toEqual([{ branch: "b1", rev: 7 }]);
  expect(calls.bodies["change"]).toEqual([{ step: "withdraw", branch: "b1", rev: 8 }]);
  // The card still shows the previous method.
  await expect(card(page, "b1")).toContainText("Card ending 4242");
});

test("a paid method change asks the owner to wait while it is confirmed", async ({ page }) => {
  await fakeCheckout(page, "paid");
  await stubCloud(page, PAID, {
    plans: [planOf(B1), planOf(PAID[1])],
    method: () => ({
      status: 200,
      body: confirmed({ kind: "period_end", charge_total_minor: 0, checkout: checkout("monthly") }),
    }),
  });
  await page.goto("/account/subscription");
  await card(page, "b1")
    .getByRole("button", { name: t("owner.subscription.method.open") })
    .click();
  await expect(lastStatus(page)).toHaveText(t("owner.subscription.method.sent"));
});

test("a pending cycle switch or method change reads in words, and only an unauthorised one can be kept out", async ({
  page,
}) => {
  const pending = (over: Partial<NonNullable<BranchPlan["pending_change"]>>) => ({
    kind: "period_end" as const,
    subject: "cycle" as const,
    cycle: "monthly" as const,
    tier: "basic" as const,
    add_ons: { devices_20: 2, tunnel: 1, owner_login: 0 },
    effective_at: RENEWS,
    renewal_total_minor: 176_882,
    needs_mandate: false,
    ...over,
  });
  const keep = (p: Page) =>
    card(p, "b1").getByRole("button", { name: t("owner.subscription.pending.keep") });

  // Waiting for the owner to authorise the new cycle's mandate: it can still be kept out.
  await stubCloud(page, PAID, {
    plans: [
      planOf(B1, { cycle: "yearly", pending_change: pending({ needs_mandate: true }) }),
      planOf(PAID[1]),
    ],
  });
  await page.goto("/account/subscription");
  await expect(card(page, "b1").getByTestId("pending-change")).toContainText(
    "Waiting for you to authorise a card or bank mandate.",
  );
  await expect(keep(page)).toBeVisible();

  // Authorised: it switches on the day the paid year ends and can no longer be taken back.
  const authorised = await page.context().newPage();
  await stubCloud(authorised, PAID, {
    plans: [planOf(B1, { cycle: "yearly", pending_change: pending({}) }), planOf(PAID[1])],
  });
  await authorised.goto("/account/subscription");
  await expect(card(authorised, "b1").getByTestId("pending-change")).toContainText(
    `Switches to monthly on ${day(RENEWS)}. No partial refunds.`,
  );
  await expect(keep(authorised)).toHaveCount(0);

  // A new payment method waits for the owner, who can still keep the current one.
  const method = await page.context().newPage();
  await stubCloud(method, PAID, {
    plans: [
      planOf(B1, {
        pending_change: pending({ subject: "method", cycle: undefined, needs_mandate: true }),
      }),
      planOf(PAID[1]),
    ],
  });
  await method.goto("/account/subscription");
  await expect(card(method, "b1").getByTestId("pending-change")).toHaveText(
    t("owner.subscription.pending.method"),
  );
  await expect(keep(method)).toBeVisible();
});

test("the open switch panel passes axe", async ({ page }) => {
  await stubCloud(page, PAID, {
    cycle: (step) =>
      step === "preview" ? { status: 200, body: toYearly() } : { status: 200, body: confirmed() },
  });
  await page.goto("/account/subscription");
  await card(page, "b1")
    .getByRole("button", { name: t("owner.subscription.cycle.open_yearly") })
    .click();
  await expect(page.getByTestId("cycle-net")).toBeVisible();
  await page.evaluate(() => document.fonts.ready);
  const found = await new AxeBuilder({ page }).analyze();
  expect(found.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" "))}`)).toEqual(
    [],
  );
});
