import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type { BranchPlan, PlanChangePreview } from "@arena/ui/generated";
import { PAID, planOf, stubCloud, t } from "./stub";

const B1 = PAID[0];
const card = (page: Page, id: string) => page.getByTestId(`subscription-${id}`);
const day = (ms: number) =>
  new Intl.DateTimeFormat("en-US", { dateStyle: "medium", timeZone: "Asia/Kolkata" }).format(ms);
const RENEWS = Date.UTC(2026, 10, 30, 12);

const preview = (over: Partial<PlanChangePreview> = {}): PlanChangePreview => ({
  rev: 7,
  kind: "now",
  currency: "INR",
  tier: "premium",
  add_ons: { devices_20: 2, tunnel: 0, owner_login: 0 },
  gst_percent: 18,
  charge_amount_minor: 174_097,
  charge_tax_minor: 31_337,
  charge_total_minor: 205_434,
  renewal_amount_minor: 209_900,
  renewal_tax_minor: 37_782,
  renewal_total_minor: 247_682,
  effective_at: Date.UTC(2026, 10, 1),
  days_left: 30,
  needs_mandate: false,
  methods: ["upi", "card", "emandate"],
  ...over,
});

const open = async (page: Page) => {
  await page.goto("/account/subscription");
  await card(page, "b1")
    .getByRole("button", { name: t("owner.subscription.change.open") })
    .click();
  return page.getByTestId("change-plan-b1");
};

test("an upgrade shows the prorated charge with GST and the new renewal, then confirms against the rev it saw", async ({
  page,
}) => {
  let pending: BranchPlan["pending_change"];
  const calls = await stubCloud(page, PAID, {
    plans: () => [planOf(B1, { pending_change: pending }), planOf(PAID[1])],
    change: (step) =>
      step === "preview"
        ? { status: 200, body: preview() }
        : {
            status: 200,
            body: {
              kind: "now",
              effective_at: 1,
              charge_total_minor: 205_434,
              renewal_total_minor: 247_682,
            },
          },
  });
  const form = await open(page);
  await expect(
    form.getByRole("button", { name: t("owner.subscription.change.confirm") }),
  ).toBeDisabled();
  await form.getByRole("radio", { name: t("owner.tier.premium") }).check();
  await expect(form.getByTestId("change-charge")).toHaveText(
    t("owner.subscription.change.charge_gst", { amount: "₹1,740.97", gst: "₹313.37" }),
  );
  await expect(form.getByTestId("change-renewal")).toContainText("₹2,099.00 + ₹377.82 GST");
  // Premium includes Tunnel.
  await expect(form.getByTestId("tunnel-included")).toHaveText(
    t("owner.subscription.change.tunnel_included"),
  );
  expect(calls.bodies["change"]).toEqual([
    {
      step: "preview",
      branch: "b1",
      tier: "premium",
      add_ons: { devices_20: 2, tunnel: 0, owner_login: 0 },
      rev: 7,
    },
  ]);

  pending = {
    kind: "now",
    tier: "premium",
    add_ons: { devices_20: 2, tunnel: 0, owner_login: 0 },
    effective_at: Date.now(),
    renewal_total_minor: 247_682,
    needs_mandate: false,
  };
  await form.getByRole("button", { name: t("owner.subscription.change.confirm") }).click();
  await expect(page.getByTestId("change-plan-b1")).toHaveCount(0);
  await expect(card(page, "b1").getByTestId("pending-change")).toContainText(
    "Starts as soon as your payment is confirmed.",
  );
  // A change being paid cannot be taken back.
  await expect(
    card(page, "b1").getByRole("button", { name: t("owner.subscription.pending.keep") }),
  ).toHaveCount(0);
  await expect(
    card(page, "b1").getByRole("button", { name: t("owner.subscription.change.open") }),
  ).toHaveCount(0);
  expect(calls.bodies["change"]?.at(-1)).toMatchObject({
    step: "confirm",
    tier: "premium",
    rev: 7,
  });
  await expect(page.getByRole("status").filter({ hasText: /./ }).last()).toHaveText(
    t("owner.subscription.change.sent"),
  );
});

test("a lower plan is scheduled for the period end with no partial refund, and Keep current plan withdraws it", async ({
  page,
}) => {
  let pending: BranchPlan["pending_change"];
  const calls = await stubCloud(page, PAID, {
    plans: () => [planOf(B1, { pending_change: pending }), planOf(PAID[1])],
    change: (step) => {
      if (step === "preview")
        return {
          status: 200,
          body: preview({
            kind: "period_end",
            tier: "basic",
            add_ons: { devices_20: 1, tunnel: 1, owner_login: 0 },
            charge_amount_minor: 0,
            charge_tax_minor: 0,
            charge_total_minor: 0,
            days_left: 0,
            effective_at: RENEWS,
          }),
        };
      if (step === "withdraw") {
        pending = undefined;
        return { status: 204, body: null };
      }
      pending = {
        kind: "period_end",
        tier: "basic",
        add_ons: { devices_20: 1, tunnel: 1, owner_login: 0 },
        effective_at: RENEWS,
        renewal_total_minor: 100_000,
        needs_mandate: false,
      };
      return {
        status: 200,
        body: {
          kind: "period_end",
          effective_at: RENEWS,
          charge_total_minor: 0,
          renewal_total_minor: 100_000,
        },
      };
    },
  });
  const form = await open(page);
  await form.getByLabel(t("owner.subscription.change.devices")).fill("1");
  await form.getByLabel(t("owner.subscription.change.devices")).blur();
  await expect(form.getByTestId("change-later")).toHaveText(
    t("owner.subscription.change.scheduled", { date: day(RENEWS) }),
  );
  await expect(form.getByTestId("change-charge")).toHaveCount(0);
  await form.getByRole("button", { name: t("owner.subscription.change.confirm") }).click();

  const c = card(page, "b1");
  await expect(c.getByTestId("pending-change")).toContainText(
    `Changes on ${day(RENEWS)}. No partial refunds.`,
  );
  // The entitlement shown is unchanged until then.
  await expect(c).toContainText(t("owner.subscription.addon.devices_20", { qty: 2 }));
  await c.getByRole("button", { name: t("owner.subscription.pending.keep") }).click();
  await expect(c.getByTestId("pending-change")).toHaveCount(0);
  await expect(c.getByRole("button", { name: t("owner.subscription.change.open") })).toBeVisible();
  expect(calls.bodies["change"]?.at(-1)).toEqual({ step: "withdraw", branch: "b1", rev: 7 });
  await expect(page.getByRole("status").filter({ hasText: /./ }).last()).toHaveText(
    t("owner.subscription.pending.kept"),
  );
});

for (const state of ["needs_approval", "grace", "pending_review"] as const) {
  test(`Change plan is disabled with its reason while the state is ${state}`, async ({ page }) => {
    await stubCloud(page, PAID, { plans: [planOf(B1, { state }), planOf(PAID[1])] });
    await page.goto("/account/subscription");
    const button = card(page, "b1").getByRole("button", {
      name: t("owner.subscription.change.open"),
    });
    await expect(button).toBeDisabled();
    await expect(card(page, "b1")).toContainText(t("owner.subscription.change.blocked"));
  });
}

test("Tunnel cannot be added on an Android Hub and says why", async ({ page }) => {
  await stubCloud(page, PAID, {
    plans: [
      planOf(B1, {
        hub_platform: "android",
        add_ons: { devices_20: 0, tunnel: 0, owner_login: 0 },
      }),
      planOf(PAID[1]),
    ],
  });
  const form = await open(page);
  await expect(
    form.getByRole("checkbox", { name: t("owner.subscription.change.tunnel") }),
  ).toBeDisabled();
  await expect(form.getByTestId("tunnel-windows")).toHaveText(
    t("owner.subscription.change.tunnel_windows"),
  );
  await expect(form.getByText(t("owner.subscription.change.owner_login_soon"))).toBeVisible();
});

test("a change from another device fails STALE: the owner sees the current plan", async ({
  page,
}) => {
  let reads = 0;
  await stubCloud(page, PAID, {
    plans: () => {
      reads++;
      return [planOf(B1, { rev: reads > 1 ? 9 : 7 }), planOf(PAID[1])];
    },
    change: (step) =>
      step === "preview"
        ? { status: 200, body: preview() }
        : { status: 409, body: { code: "STALE", message: "This plan changed." } },
  });
  const form = await open(page);
  await form.getByRole("radio", { name: t("owner.tier.premium") }).check();
  await form.getByRole("button", { name: t("owner.subscription.change.confirm") }).click();
  await expect.poll(() => reads).toBeGreaterThan(1);
  await expect(page.getByRole("status").filter({ hasText: /./ }).last()).toHaveText(
    t("owner.subscription.change.stale"),
  );
  await expect(
    card(page, "b1").getByRole("button", { name: t("owner.subscription.change.open") }),
  ).toBeVisible();
});

test("a change over the UPI limit opens Checkout for a new mandate and closing it leaves everything as it was", async ({
  page,
}) => {
  await page.addInitScript(() => {
    class Fake {
      constructor(private options: { modal: { ondismiss: () => void } }) {}
      on() {}
      open() {
        (window as unknown as { __opened: unknown }).__opened = this.options;
        this.options.modal.ondismiss();
      }
      close() {}
    }
    (window as unknown as { Razorpay: unknown }).Razorpay = Fake;
  });
  const calls = await stubCloud(page, PAID, {
    plans: [planOf(B1), planOf(PAID[1])],
    change: (step) =>
      step === "preview"
        ? {
            status: 200,
            body: preview({
              needs_mandate: true,
              methods: ["card", "emandate"],
              renewal_total_minor: 2_358_820,
            }),
          }
        : {
            status: 200,
            body: {
              kind: "now",
              effective_at: 1,
              charge_total_minor: 205_434,
              renewal_total_minor: 2_358_820,
              checkout: {
                subscription_id: "sb1",
                state: "active",
                key_id: "rzp_test",
                razorpay_subscription_id: "sub_new",
                band: "IN",
                currency: "INR",
                tier: "premium",
                cycle: "monthly",
                methods: ["card", "emandate"],
                amount_minor: 1_999_000,
                tax_minor: 359_820,
                total_minor: 2_358_820,
              },
            },
          },
  });
  const form = await open(page);
  await form.getByRole("radio", { name: t("owner.tier.premium") }).check();
  await expect(form.getByTestId("change-mandate")).toHaveText(
    t("owner.subscription.change.mandate"),
  );
  await form.getByRole("button", { name: t("owner.subscription.change.confirm_mandate") }).click();
  await expect(form.getByRole("alert")).toHaveText(t("owner.subscription.change.dismissed"));
  const options = (await page.evaluate(
    () => (window as unknown as { __opened: unknown }).__opened,
  )) as {
    subscription_id: string;
    key: string;
    config: { display: { blocks: { allowed: { instruments: { method: string }[] } } } };
  };
  expect(options.subscription_id).toBe("sub_new");
  expect(options.config.display.blocks.allowed.instruments.map((i) => i.method)).toEqual([
    "card",
    "emandate",
  ]);
  // Closing Checkout changed nothing: the form is still open and the card has no pending change.
  expect(calls.bodies["change"]?.map((b) => (b as { step: string }).step)).toEqual([
    "preview",
    "confirm",
  ]);
  await expect(card(page, "b1").getByTestId("pending-change")).toHaveCount(0);
});

test("the open Change plan panel passes axe", async ({ page }) => {
  await stubCloud(page, PAID, { change: () => ({ status: 200, body: preview() }) });
  const form = await open(page);
  await form.getByRole("radio", { name: t("owner.tier.premium") }).check();
  await expect(form.getByTestId("change-charge")).toBeVisible();
  await page.evaluate(() => document.fonts.ready);
  const found = await new AxeBuilder({ page }).analyze();
  expect(found.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" "))}`)).toEqual(
    [],
  );
});
