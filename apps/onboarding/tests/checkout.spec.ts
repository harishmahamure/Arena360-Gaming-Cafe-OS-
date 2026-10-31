import { expect, test, type Page } from "@playwright/test";
import { RENEWS_AT, READY_OWNER, stubCloud, t, type Cloud } from "./stub";

type Key = Parameters<typeof t>[0];
const billing = { signedIn: true, owner: READY_OWNER, wizard: { branch: {}, billing: {} } };
const h1 = (page: Page, key: Key) => page.getByRole("heading", { level: 1, name: t(key) });
const radio = (page: Page, key: Key) => page.getByRole("radio", { name: t(key), exact: true });
const proceed = (page: Page) =>
  page.getByRole("button", { name: t("onboarding.tier.continue"), exact: true });
const pay = (page: Page) =>
  page.getByRole("button", { name: t("onboarding.pay.button"), exact: true });
const confirming = (page: Page) => page.getByTestId("confirming");
const announce = t("onboarding.setup.announce", {
  n: 3,
  total: 10,
  name: t("ui.wizard.step.install_hub"),
});

type Rzp = {
  opened: { key: string; subscription_id: string; config: unknown }[];
  pay(): void;
  fail(): void;
  dismiss(): void;
};
declare global {
  interface Window {
    __rzp: Rzp;
    __announcements: number;
  }
}

/** A `window.Razorpay` that records what Checkout was opened with and lets the test act as the payer. */
async function stubRazorpay(page: Page) {
  await page.addInitScript(() => {
    type Opts = {
      key: string;
      subscription_id: string;
      config: unknown;
      handler: (r: unknown) => void;
      modal: { ondismiss: () => void };
    };
    const instances: { options: Opts; handlers: Record<string, (r: unknown) => void> }[] = [];
    const last = () => instances[instances.length - 1]!;
    window.__rzp = {
      opened: [],
      pay: () => last().options.handler({ razorpay_payment_id: "pay_1" }),
      fail: () => last().handlers["payment.failed"]!({ error: { code: "BAD_REQUEST_ERROR" } }),
      dismiss: () => last().options.modal.ondismiss(),
    };
    // @ts-expect-error a stand-in for checkout.js
    window.Razorpay = class {
      options: Opts;
      handlers: Record<string, (r: unknown) => void> = {};
      constructor(options: Opts) {
        this.options = options;
        instances.push(this);
      }
      on(event: string, fn: (r: unknown) => void) {
        this.handlers[event] = fn;
      }
      open() {
        window.__rzp.opened.push({
          key: this.options.key,
          subscription_id: this.options.subscription_id,
          config: this.options.config,
        });
      }
      // The real Checkout reports its closing too.
      close() {
        this.options.modal.ondismiss();
      }
    };
  });
}

/** Counts how many times the wizard's live region was given a non-empty announcement. */
async function countAnnouncements(page: Page) {
  await page.evaluate(() => {
    window.__announcements = 0;
    const el = document.querySelector("[data-testid=wizard-announcement]")!;
    new MutationObserver(() => {
      if (el.textContent) window.__announcements += 1;
    }).observe(el, { childList: true, characterData: true, subtree: true });
  });
}

type Choice = { plan?: "basic" | "premium"; yearly?: boolean; packs?: number; tunnel?: boolean };

async function choose(page: Page, c: Choice = {}) {
  await radio(page, `pricing.${c.plan ?? "basic"}.name`).check();
  if (c.yearly) {
    await page.getByRole("tab", { name: t("onboarding.tier.yearly") }).click();
  }
  if (c.packs) await page.getByLabel(t("onboarding.addons.devices")).fill(String(c.packs));
  if (c.tunnel) {
    await page.getByRole("checkbox", { name: t("onboarding.addons.tunnel") }).check();
  }
}

async function toSummary(page: Page, c: Choice = {}) {
  await page.goto("/setup/tier");
  await expect(h1(page, "ui.wizard.step.tier")).toBeVisible();
  await choose(page, c);
  await proceed(page).click();
  await expect(page.getByTestId("summary")).toBeVisible();
}

/** The 10.3 webhook: the entitlement is written and the plan step is done. */
function activate(cloud: Cloud, tier: "basic" | "premium" = "basic") {
  cloud.tier = tier;
  cloud.done.add("tier");
}

async function toConfirming(page: Page, cloud: Cloud, c: Choice = {}) {
  await toSummary(page, c);
  await pay(page).click();
  await expect.poll(() => page.evaluate(() => window.__rzp.opened.length)).toBe(1);
  await countAnnouncements(page);
  await page.evaluate(() => window.__rzp.pay());
  await expect(confirming(page)).toHaveText(t("onboarding.pay.confirming"));
  expect(cloud.tierPicks).toEqual([]);
}

async function expectStepThree(page: Page) {
  await expect(page).toHaveURL(/\/setup\/install_hub$/);
  await expect(h1(page, "ui.wizard.step.install_hub")).toBeFocused();
  await expect(page.getByTestId("wizard-announcement")).toHaveText(announce);
  expect(await page.evaluate(() => window.__announcements)).toBe(1);
}

test("the summary is the cloud's quote: subtotal, GST, total, renewal date and refund line", async ({
  page,
}) => {
  const cloud = await stubCloud(page, billing);
  await stubRazorpay(page);
  await toSummary(page, { yearly: true, packs: 2, tunnel: true });
  expect(cloud.quoteBodies).toEqual([
    { tier: "basic", cycle: "yearly", add_ons: { devices_20: 2, tunnel: 1, owner_login: 0 } },
  ]);
  await expect(page.getByTestId("summary-plan")).toHaveText(t("pricing.basic.name"));
  await expect(page.getByTestId("summary-cycle")).toHaveText(t("onboarding.tier.yearly"));
  await expect(page.getByTestId("summary-addons")).toHaveText(
    `${t("onboarding.summary.devices", { n: 2 })}, ${t("onboarding.summary.tunnel")}`,
  );
  // 2,000 + 2 x 500 + 2,000 = 5,000; GST 900; total 5,900.
  await expect(page.getByTestId("summary-subtotal")).toHaveText("₹5,000");
  await expect(page.getByTestId("summary-gst")).toHaveText("₹900");
  await expect(page.getByTestId("summary-total")).toHaveText("₹5,900");
  await expect(page.getByText(t("onboarding.summary.gst", { percent: 18 }))).toBeVisible();
  // The browser's medium date, in the branch's zone.
  const date = await page.evaluate(
    (at) =>
      new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeZone: "Asia/Kolkata" }).format(
        at,
      ),
    RENEWS_AT,
  );
  await expect(page.getByTestId("summary-renews")).toHaveText(
    t("onboarding.summary.renews", { date }),
  );
  await expect(page.getByTestId("summary-refund")).toHaveText(t("onboarding.summary.refund"));
  await expect(pay(page)).toBeVisible();
  await expect(page.getByTestId("summary-notice")).toHaveCount(0);
  await expect(
    page.getByRole("heading", { level: 2, name: t("onboarding.summary.title") }),
  ).toBeFocused();
  expect(cloud.subscribeBodies).toEqual([]);
});

test("a renewal over Rs 15,000 says why UPI is left out", async ({ page }) => {
  await stubCloud(page, billing);
  await stubRazorpay(page);
  await toSummary(page, { plan: "premium", yearly: true });
  await expect(page.getByTestId("summary-total")).toHaveText("₹23,588.20");
  await expect(page.getByTestId("summary-notice")).toHaveText(
    "Renewals over Rs 15,000 need a card or bank mandate",
  );
  await pay(page).click();
  await expect.poll(() => page.evaluate(() => window.__rzp.opened.length)).toBe(1);
  const config = await page.evaluate(() => JSON.stringify(window.__rzp.opened[0]!.config));
  expect(config).not.toContain("upi");
});

test("Pay opens Checkout with the key, the subscription and only the allowed methods", async ({
  page,
}) => {
  const cloud = await stubCloud(page, billing);
  await stubRazorpay(page);
  await toSummary(page, { packs: 1 });
  await pay(page).click();
  await expect.poll(() => page.evaluate(() => window.__rzp.opened.length)).toBe(1);
  const opened = await page.evaluate(() => window.__rzp.opened[0]!);
  expect(opened.key).toBe("rzp_test_stub");
  expect(opened.subscription_id).toBe("sub_stub_001");
  expect(opened.config).toEqual({
    display: {
      blocks: {
        allowed: {
          name: t("onboarding.pay.methods"),
          instruments: [{ method: "upi" }, { method: "card" }, { method: "emandate" }],
        },
      },
      sequence: ["block.allowed"],
      preferences: { show_default_blocks: false },
    },
  });
  const wire = JSON.stringify(opened.config);
  for (const banned of ["wallet", "netbanking", "paylater", "emi"]) {
    expect(wire).not.toContain(banned);
  }
  expect(cloud.subscribeBodies).toEqual([
    {
      tier: "basic",
      cycle: "monthly",
      add_ons: { devices_20: 1, tunnel: 0, owner_login: 0 },
      billing_country: "IN",
    },
  ]);
  expect(cloud.tierPicks).toEqual([]);
});

test("closing Checkout returns to the summary and a second try uses the same subscription", async ({
  page,
}) => {
  const cloud = await stubCloud(page, billing);
  await stubRazorpay(page);
  await toSummary(page);
  await pay(page).click();
  await expect.poll(() => page.evaluate(() => window.__rzp.opened.length)).toBe(1);
  await page.evaluate(() => window.__rzp.dismiss());
  await expect(page.getByTestId("summary")).toBeVisible();
  await expect(page.getByTestId("pay-failed")).toHaveCount(0);
  await expect(confirming(page)).toHaveCount(0);
  await expect(page).toHaveURL(/\/setup\/tier$/);
  expect(cloud.done.has("tier")).toBe(false);

  await pay(page).click();
  await expect.poll(() => page.evaluate(() => window.__rzp.opened.length)).toBe(2);
  expect(cloud.subscriptionIds).toEqual(["sub_stub_001"]);
  expect(cloud.cancelled).toEqual([]);
  const ids = await page.evaluate(() => window.__rzp.opened.map((o) => o.subscription_id));
  expect(ids).toEqual(["sub_stub_001", "sub_stub_001"]);
});

test("a declined payment says nothing was charged and Pay again reuses the subscription", async ({
  page,
}) => {
  const cloud = await stubCloud(page, billing);
  await stubRazorpay(page);
  await toSummary(page);
  await pay(page).click();
  await expect.poll(() => page.evaluate(() => window.__rzp.opened.length)).toBe(1);
  await page.evaluate(() => window.__rzp.fail());
  await expect(page.getByTestId("pay-failed")).toContainText(t("onboarding.pay.failed"));
  await expect(page.getByTestId("summary")).toBeVisible();
  await expect(confirming(page)).toHaveCount(0);

  await pay(page).click();
  await expect.poll(() => page.evaluate(() => window.__rzp.opened.length)).toBe(2);
  await expect(page.getByTestId("pay-failed")).toHaveCount(0);
  expect(cloud.subscriptionIds).toEqual(["sub_stub_001"]);
  expect(cloud.cancelled).toEqual([]);
});

test("another choice after a closed Checkout cancels the old subscription", async ({ page }) => {
  const cloud = await stubCloud(page, billing);
  await stubRazorpay(page);
  await toSummary(page);
  await pay(page).click();
  await expect.poll(() => page.evaluate(() => window.__rzp.opened.length)).toBe(1);
  await page.evaluate(() => window.__rzp.dismiss());
  await page.getByRole("button", { name: t("onboarding.summary.change") }).click();
  await choose(page, { plan: "premium" });
  await proceed(page).click();
  await expect(page.getByTestId("summary-plan")).toHaveText(t("pricing.premium.name"));
  await pay(page).click();
  await expect.poll(() => page.evaluate(() => window.__rzp.opened.length)).toBe(2);
  expect(cloud.cancelled).toEqual(["sub_stub_001"]);
  expect(cloud.subscriptionIds).toEqual(["sub_stub_001", "sub_stub_002"]);
});

test("a provider failure shows a generic error and opens nothing", async ({ page }) => {
  const cloud = await stubCloud(page, billing);
  await stubRazorpay(page);
  await toSummary(page);
  cloud.nextSubscribe.push({ status: 503, body: { code: "INTERNAL", message: "stub" } });
  await pay(page).click();
  await expect(page.getByRole("alert")).toHaveText(new RegExp(t("onboarding.error.generic")));
  expect(await page.evaluate(() => window.__rzp.opened.length)).toBe(0);
  await expect(pay(page)).not.toHaveAttribute("aria-disabled", "true");
  expect(cloud.done.has("tier")).toBe(false);
});

test("a band that went off sale is said so on Continue", async ({ page }) => {
  const cloud = await stubCloud(page, billing);
  await stubRazorpay(page);
  await page.goto("/setup/tier");
  await choose(page);
  cloud.billing!.onSale = false;
  await proceed(page).click();
  await expect(page.getByRole("alert")).toContainText(t("pricing.band_unavailable"));
  await expect(radio(page, "pricing.basic.name")).toBeDisabled();
  await expect(radio(page, "pricing.free.name")).toBeEnabled();
});

test("after payment the owner waits, and the poll moves them to step 3 once", async ({ page }) => {
  await page.clock.install();
  const cloud = await stubCloud(page, billing);
  await stubRazorpay(page);
  await toConfirming(page, cloud);
  await expect(page).toHaveURL(/\/setup\/tier$/);

  // Nothing yet: the poll finds the plan step still pending.
  await page.clock.fastForward(5_000);
  await expect(confirming(page)).toBeVisible();

  activate(cloud);
  await page.clock.fastForward(5_000);
  await expectStepThree(page);
  // A paid branch downloads the Windows Hub first and keeps the Android link.
  await expect(
    page.getByRole("link", { name: t("onboarding.install.download.link") }),
  ).toHaveAttribute("href", "/download/hub");
  await expect(
    page.getByRole("link", { name: t("onboarding.install.download.android") }),
  ).toHaveAttribute("href", "/help/android-hub");
  expect(cloud.tierPicks).toEqual([]);
});

test("a server-sent event moves the wizard on without waiting for the poll", async ({ page }) => {
  const cloud = await stubCloud(page, billing);
  cloud.sse = true;
  await stubRazorpay(page);
  await toConfirming(page, cloud);
  activate(cloud);
  // The stream wakes the wizard on its next (re)connect; the 5 s poll is far off.
  await expect(page).toHaveURL(/\/setup\/install_hub$/, { timeout: 4_000 });
  await expectStepThree(page);
});

test("after two minutes the owner is told it is still being confirmed, and still moves on", async ({
  page,
}) => {
  await page.clock.install();
  const cloud = await stubCloud(page, billing);
  await stubRazorpay(page);
  await toConfirming(page, cloud, { plan: "premium", yearly: true });

  await page.clock.fastForward(119_000);
  await expect(confirming(page)).toHaveText(t("onboarding.pay.confirming"));
  await page.clock.fastForward(2_000);
  await expect(confirming(page)).toHaveText(t("onboarding.pay.slow"));
  await expect(page).toHaveURL(/\/setup\/tier$/);

  activate(cloud, "premium");
  await page.clock.fastForward(5_000);
  await expectStepThree(page);
});
