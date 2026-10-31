import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type { BranchPlan } from "@arena/ui/generated";
import { PAID, invoiceRowOf, planOf, stubCloud, t } from "./stub";

const B1 = PAID[0];
const B2 = PAID[1];
const card = (page: Page, id: string) => page.getByTestId(`subscription-${id}`);
const day = (ms: number) =>
  new Intl.DateTimeFormat("en-US", { dateStyle: "medium", timeZone: "Asia/Kolkata" }).format(ms);
const RENEWS = Date.UTC(2026, 10, 30, 12);

test("a paid branch's card shows tier, cycle, state, add-ons, renewal with GST and the payment method", async ({
  page,
}) => {
  await stubCloud(page);
  await page.goto("/account/subscription");
  const c = card(page, "b1");
  await expect(c.getByRole("heading", { level: 2, name: B1.name })).toBeVisible();
  await expect(c.locator('[data-component="status-badge"]')).toHaveText(
    t("owner.subscription.state.active"),
  );
  await expect(c).toContainText(
    `${t("owner.tier.basic")} · ${t("owner.subscription.cycle.monthly")}`,
  );
  await expect(c).toContainText(
    `${t("owner.subscription.addon.devices_20", { qty: 2 })}, ${t("owner.subscription.addon.tunnel")}`,
  );
  // The price keeps GST beside the amount, never inside it.
  const monthly = t("owner.subscription.price.monthly", {
    amount: t("owner.subscription.price.gst", { amount: "₹1,499.00", gst: "₹269.82" }),
  });
  await expect(c.getByText(monthly, { exact: true })).toBeVisible();
  await expect(c).toContainText(
    t("owner.subscription.renews_value", { date: day(RENEWS), amount: monthly }),
  );
  await expect(c).toContainText(t("owner.subscription.payment.card", { last4: "4242" }));
});

test("a Free branch says what Free is and Upgrade opens the checkout for that branch", async ({
  page,
}) => {
  await stubCloud(page);
  await page.route("https://start.arena360.cloud/**", (route) =>
    route.fulfill({ contentType: "text/html", body: "<title>Upgrade</title>" }),
  );
  await page.goto("/account/subscription");
  const c = card(page, "b2");
  await expect(c.locator('[data-component="status-badge"]')).toHaveText(
    t("owner.subscription.state.free"),
  );
  await expect(c).toContainText(t("owner.subscription.free_line"));
  await expect(
    card(page, "b1").getByRole("button", { name: t("owner.subscription.upgrade") }),
  ).toHaveCount(0);
  await c.getByRole("button", { name: t("owner.subscription.upgrade") }).click();
  await expect(page).toHaveURL(
    /^https:\/\/start\.arena360\.cloud\/upgrade\?tier=basic&branch=b2&return=/,
  );
});

test("Monthly | Yearly swaps every price in place, shows in the URL and leaves the renewal alone", async ({
  page,
}) => {
  await stubCloud(page);
  await page.goto("/account/subscription");
  const tabs = page.getByRole("tablist", { name: t("owner.subscription.cycle.label") });
  await expect(
    tabs.getByRole("tab", { name: t("owner.subscription.cycle.monthly") }),
  ).toHaveAttribute("aria-selected", "true");
  await tabs.getByRole("tab", { name: t("owner.subscription.cycle.yearly") }).click();
  await expect(page).toHaveURL(/cycle=yearly/);
  const yearly = t("owner.subscription.price.yearly", {
    amount: t("owner.subscription.price.gst", { amount: "₹14,990.00", gst: "₹2,698.20" }),
  });
  await expect(card(page, "b1").getByText(yearly, { exact: true })).toBeVisible();
  // The next renewal is still what the branch is billed.
  await expect(card(page, "b1")).toContainText("₹1,499.00");
  // A link that names the cycle opens on it.
  await page.goto("/account/subscription?cycle=yearly");
  await expect(
    page
      .getByRole("tablist", { name: t("owner.subscription.cycle.label") })
      .getByRole("tab", { name: t("owner.subscription.cycle.yearly") }),
  ).toHaveAttribute("aria-selected", "true");
  await expect(card(page, "b1").getByText(yearly, { exact: true })).toBeVisible();
});

const STATES: [Partial<BranchPlan>, string][] = [
  [{ state: "needs_approval" }, t("owner.subscription.state.needs_approval")],
  [{ state: "grace" }, t("owner.subscription.state.grace")],
  [{ state: "lapsed" }, t("owner.subscription.state.lapsed")],
  [{ state: "pending" }, t("owner.subscription.state.pending")],
  [
    { state: "pending_review", message: "We're checking your payment" },
    t("owner.subscription.state.pending_review"),
  ],
  [{ state: "cancels" }, t("owner.subscription.state.cancels", { date: day(RENEWS) })],
];
for (const [over, words] of STATES) {
  test(`state ${over.state} reads as "${words}"`, async ({ page }) => {
    await stubCloud(page, PAID, { plans: [planOf(B1, over), planOf(B2)] });
    await page.goto("/account/subscription");
    await expect(card(page, "b1").locator('[data-component="status-badge"]')).toHaveText(words);
    if (over.message) await expect(card(page, "b1")).toContainText(over.message);
  });
}

test("a cancelled plan shows the day it ends and no renewal", async ({ page }) => {
  await stubCloud(page, PAID, { plans: [planOf(B1, { state: "cancels" }), planOf(B2)] });
  await page.goto("/account/subscription");
  const c = card(page, "b1");
  await expect(c.getByText(t("owner.subscription.ends"))).toBeVisible();
  await expect(c.getByText(t("owner.subscription.renews"))).toHaveCount(0);
});

test("a state that changes is said once in the live region", async ({ page }) => {
  let state: BranchPlan["state"] = "active";
  await stubCloud(page, PAID, { plans: () => [planOf(B1, { state }), planOf(B2)] });
  await page.goto("/account/subscription");
  const live = page.getByRole("status").filter({ hasText: /./ });
  await expect(card(page, "b1")).toContainText(t("owner.subscription.state.active"));
  state = "grace";
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await expect(card(page, "b1").locator('[data-component="status-badge"]')).toHaveText(
    t("owner.subscription.state.grace"),
  );
  await expect(live.last()).toHaveText(
    t("owner.subscription.announce", {
      branch: B1.name,
      state: t("owner.subscription.state.grace"),
    }),
  );
});

test("one h1, each card a labelled region, one column on a phone and two on a desktop", async ({
  page,
}, info) => {
  await stubCloud(page);
  await page.goto("/account/subscription");
  await expect(page.locator("h1")).toHaveCount(1);
  for (const b of PAID)
    await expect(page.getByRole("region", { name: b.name, exact: true })).toBeVisible();
  const [first, second] = await Promise.all(["b1", "b2"].map((id) => card(page, id).boundingBox()));
  if (info.project.name === "phone") {
    expect(second!.y).toBeGreaterThan(first!.y + first!.height - 1);
    expect(Math.abs(second!.x - first!.x)).toBeLessThan(2);
  } else {
    expect(Math.abs(second!.y - first!.y)).toBeLessThan(2);
    expect(second!.x).toBeGreaterThan(first!.x + first!.width - 1);
  }
});

const INVOICES = [
  invoiceRowOf({
    id: "cn1",
    number: "A360/2026-27/CN000001",
    kind: "credit_note",
    total_minor: -176_882,
    original_number: "A360/2026-27/000002",
    issued_at: Date.UTC(2026, 9, 31, 8),
  }),
  invoiceRowOf({ id: "inv2", number: "A360/2026-27/000002", issued_at: Date.UTC(2026, 9, 30, 8) }),
  invoiceRowOf({
    id: "inv1",
    number: "A360/2025-26/000009",
    fy: "2025-26",
    branch_id: "b2",
    branch_name: B2.name,
    issued_at: Date.UTC(2026, 2, 5, 8),
    total_minor: 88_000,
    pdf_ready: false,
  }),
];

test("invoices list newest first with a credit note as a negative amount against its invoice", async ({
  page,
}) => {
  await stubCloud(page, PAID, { invoices: INVOICES });
  await page.goto("/account/subscription");
  const rows = page.getByTestId("invoices").getByRole("listitem");
  await expect(rows).toHaveCount(3);
  await expect(rows.nth(0)).toContainText(
    t("owner.invoices.credit_note", { number: "A360/2026-27/CN000001" }),
  );
  await expect(rows.nth(0)).toContainText(
    t("owner.invoices.against", { number: "A360/2026-27/000002" }),
  );
  await expect(rows.nth(0)).toContainText("−₹1,768.82");
  await expect(rows.nth(1)).toContainText("A360/2026-27/000002");
  await expect(rows.nth(1)).toContainText(B1.name);
  await expect(rows.nth(1)).toContainText(day(Date.UTC(2026, 9, 30, 8)));
  await expect(rows.nth(1)).toContainText("₹1,768.82");
  await expect(rows.nth(2)).toContainText("A360/2025-26/000009");
});

test("the invoice list filters by branch and financial year", async ({ page }) => {
  const calls = await stubCloud(page, PAID, { invoices: INVOICES });
  await page.goto("/account/subscription");
  const list = page.getByTestId("invoices");
  await expect(list.getByRole("listitem")).toHaveCount(3);
  await list.getByLabel(t("owner.invoices.year")).selectOption("2025-26");
  await expect(list.getByRole("listitem")).toHaveCount(1);
  await list.getByLabel(t("owner.invoices.year")).selectOption("");
  await list.getByLabel(t("owner.invoices.branch")).selectOption("b1");
  await expect(list.getByRole("listitem")).toHaveCount(2);
  await list.getByLabel(t("owner.invoices.year")).selectOption("2025-26");
  await expect(list.getByText(t("owner.invoices.empty"))).toBeVisible();
  expect(calls.bodies["invoices"]).toEqual([
    "",
    "fy=2025-26",
    "",
    "branch=b1",
    "branch=b1&fy=2025-26",
  ]);
  // Every financial year stays on offer while filtered.
  await expect(list.getByLabel(t("owner.invoices.year")).locator("option")).toHaveText([
    t("owner.invoices.year.all"),
    "2026-27",
    "2025-26",
  ]);
});

test("Download PDF opens the signed link; an invoice without a PDF says it is being prepared", async ({
  page,
}) => {
  const calls = await stubCloud(page, PAID, { invoices: INVOICES });
  await page.route("https://files.example.test/**", (route) =>
    route.fulfill({ contentType: "text/html", body: "<title>pdf</title>" }),
  );
  await page.goto("/account/subscription");
  const list = page.getByTestId("invoices");
  await expect(list.getByRole("button", { name: /Download PDF/ })).toHaveCount(2);
  const last = list.getByRole("listitem").nth(2);
  await expect(last.getByRole("button")).toHaveCount(0);
  await expect(last).toContainText(t("owner.invoices.preparing"));
  await list
    .getByRole("button", {
      name: t("owner.invoices.download_for", { number: "A360/2026-27/000002" }),
    })
    .click();
  await expect(page).toHaveURL(/^https:\/\/files\.example\.test\/billing\/invoices\/x\.pdf/);
  expect(calls.bodies["pdf"]).toEqual(["inv2"]);
});

test("a PDF that cannot be opened says so", async ({ page }) => {
  await stubCloud(page, PAID, {
    invoices: INVOICES,
    pdf: () => ({ status: 503, body: { code: "INTERNAL", message: "" } }),
  });
  await page.goto("/account/subscription");
  await page
    .getByTestId("invoices")
    .getByRole("button", { name: /A360\/2026-27\/000002/ })
    .click();
  await expect(page.getByRole("alert")).toHaveText(t("owner.invoices.failed"));
  await expect(page).toHaveURL(/\/account\/subscription/);
});

test("with no invoices the list says so", async ({ page }) => {
  await stubCloud(page);
  await page.goto("/account/subscription");
  await expect(page.getByText(t("owner.invoices.empty"))).toBeVisible();
});

for (const theme of ["light", "dark", "hc"] as const) {
  test(`every state's badge passes axe in the ${theme} theme`, async ({ page }) => {
    const states: BranchPlan["state"][] = [
      "free",
      "pending",
      "pending_review",
      "active",
      "needs_approval",
      "grace",
      "cancels",
      "lapsed",
    ];
    const plans = states.map((state, i) =>
      planOf(
        { ...B1, id: `s${i}`, name: `Branch ${state}` },
        { state, tier: state === "free" ? "free" : "basic" },
      ),
    );
    await stubCloud(page, PAID, { plans });
    await page.addInitScript((v) => localStorage.setItem("arena.theme", v), theme);
    await page.goto("/account/subscription");
    await expect(page.getByTestId("subscription-s7")).toBeVisible();
    await page.evaluate(() => document.fonts.ready);
    const found = await new AxeBuilder({ page }).analyze();
    expect(
      found.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" "))}`),
    ).toEqual([]);
  });
}
