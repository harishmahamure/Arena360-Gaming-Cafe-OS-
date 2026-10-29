import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type { OwnerRange, OwnerRangeStock, RollupShift } from "@arena/ui/generated";
import { PAID, inSetup, liveOf, rangeOfBranch, stubCloud, t } from "./stub";

const ONE = [PAID[0]];
const DUBAI = { ...PAID[0], id: "b3", name: "Pixel Arena Dubai", branch_code: "DXB" };
const TWO = [PAID[0], DUBAI];

/** The date it is in the branch's zone now, as the app computes it. */
const today = () =>
  new Intl.DateTimeFormat("en-CA", {
    timeZone: "Asia/Kolkata",
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  }).format(Date.now());

const totals = (total: number): OwnerRange["totals"] => ({
  days: 3,
  revenue: {
    total,
    by_type: { play: total - 3_000_000, products: 2_000_000, plans: 700_000, other: 300_000 },
    by_method: {
      cash: 4_000_000,
      upi: 3_000_000,
      card: 1_000_000,
      wallet: 500_000,
      credit: 500_000,
    },
  },
  refunds: { count: 2, amount: 120_000 },
  discounts: { count: 4, amount: 80_000 },
  voids: { count: 1, amount: 45_000 },
  sessions: { count: 40, minutes: 3_000 },
});

const shift = (id: string, over: Partial<RollupShift> = {}): RollupShift => ({
  shift_id: id,
  staff_name: "Meera",
  opened_at: Date.parse("2026-10-27T03:30:00Z"),
  closed_at: Date.parse("2026-10-27T11:30:00Z"),
  float: 100_000,
  expected: 520_000,
  counted: 515_000,
  variance: -5_000,
  ...over,
});

const REVENUE = (query: URLSearchParams) =>
  rangeOfBranch("b1", {
    from: query.get("from") ?? "",
    to: query.get("to") ?? "",
    totals: totals(9_000_000),
    rows: [
      { period: "2026-10-27", totals: { ...totals(4_000_000), days: 1 } },
      { period: "2026-10-28", totals: { ...totals(5_000_000), days: 1 } },
    ],
  });

const summaryCalls = (calls: string[]) => calls.filter((c) => /summary$/.test(c));

async function pick(page: Page, label: Parameters<typeof t>[0], option: string) {
  await page.getByLabel(t(label)).selectOption(option);
}

test("Revenue shows totals by type and method with refunds negative and discounts listed", async ({
  page,
}) => {
  await stubCloud(page, ONE, { range: { b1: REVENUE } });
  await page.goto("/reports");
  const main = page.getByRole("main");
  await expect(main.getByRole("heading", { name: t("owner.reports.tab.revenue") })).toBeVisible();
  await expect(main.getByText("₹90,000.00").first()).toBeVisible();
  await expect(main.getByText(t("owner.reports.revenue.days", { n: 3 }))).toBeVisible();
  const line = (label: string) => main.locator(".line").filter({ hasText: label });
  await expect(line(t("owner.reports.type.play"))).toContainText("₹60,000.00");
  await expect(line(t("owner.reports.type.products"))).toContainText("₹20,000.00");
  await expect(line(t("owner.reports.method.upi"))).toContainText("₹30,000.00");
  await expect(line(t("owner.reports.method.credit"))).toContainText("₹5,000.00");
  await expect(line(t("owner.reports.refunds", { n: 2 }))).toContainText("−₹1,200.00");
  await expect(line(t("owner.reports.discounts", { n: 4 }))).toContainText("−₹800.00");
  await expect(line(t("owner.reports.voids", { n: 1 }))).toContainText("−₹450.00");
  await expect(line("2026-10-27")).toContainText("₹40,000.00");
  await expect(main.getByText(t("owner.reports.partial"))).toHaveCount(0);
});

test("today's row is the live figure, labelled and aged, and today is not a missing day", async ({
  page,
}) => {
  const day = today();
  await stubCloud(page, ONE, {
    range: {
      b1: (q) =>
        rangeOfBranch("b1", {
          from: q.get("from") ?? "",
          to: q.get("to") ?? "",
          totals: { ...totals(4_000_000), days: 1 },
          rows: [{ period: "2026-10-01", totals: { ...totals(4_000_000), days: 1 } }],
          missing_days: ["2026-10-02", day],
        }),
    },
    live: { b1: liveOf("b1", {}, { business_day: day, revenue_so_far_minor: 1_234_500 }) },
  });
  await page.goto("/reports");
  await pick(page, "owner.reports.range", "month");
  const main = page.getByRole("main");
  const row = main.locator(".line.live");
  await expect(row).toContainText(t("owner.reports.so_far"));
  await expect(row).toContainText("₹12,345.00");
  await expect(row).toContainText(/as of [12] min ago/);
  await expect(main.getByText(t("owner.reports.partial"))).toBeVisible();
  await expect(
    main.getByText(t("owner.reports.missing.one", { dates: "2026-10-02" })),
  ).toBeVisible();
  await expect(main.getByText(day)).toHaveCount(1);
});

test("days without a rollup are listed and the totals are marked partial", async ({ page }) => {
  await stubCloud(page, ONE, {
    range: {
      b1: (q) =>
        rangeOfBranch("b1", {
          from: q.get("from") ?? "",
          to: q.get("to") ?? "",
          missing_days: ["2026-10-28", "2026-10-29"],
        }),
    },
  });
  await page.goto("/reports");
  await expect(
    page.getByText(t("owner.reports.missing.many", { n: 2, dates: "2026-10-28, 2026-10-29" })),
  ).toBeVisible();
  await expect(page.getByText(t("owner.reports.partial"))).toBeVisible();
});

test("the range picker asks for Today, this week, this month and a custom range", async ({
  page,
}) => {
  const calls = await stubCloud(page, ONE);
  const queries: string[] = [];
  await page.route("**/v1/branches/b1/summary?**", (route) => {
    queries.push(new URL(route.request().url()).search);
    return route.fallback();
  });
  await page.goto("/reports");
  const day = today();
  await expect.poll(() => queries.length).toBeGreaterThan(0);
  const week = new URLSearchParams(queries.at(-1));
  expect(week.get("to")).toBe(day);
  expect(new Date(week.get("from") + "T00:00:00Z").getUTCDay()).toBe(1);

  await pick(page, "owner.reports.range", "today");
  await expect.poll(() => queries.at(-1)).toBe(`?from=${day}&to=${day}&group_by=day`);
  await pick(page, "owner.reports.range", "month");
  await expect.poll(() => queries.at(-1)).toBe(`?from=${day.slice(0, 8)}01&to=${day}&group_by=day`);

  await pick(page, "owner.reports.range", "custom");
  await page.getByLabel(t("owner.reports.from")).fill("2026-09-01");
  await page.getByLabel(t("owner.reports.to")).fill("2026-09-30");
  await expect.poll(() => queries.at(-1)).toBe("?from=2026-09-01&to=2026-09-30&group_by=day");

  const before = summaryCalls(calls).length;
  await page.getByLabel(t("owner.reports.from")).fill("2026-10-05");
  await expect(page.getByRole("alert")).toHaveText(t("owner.reports.error.order"));
  await page.getByLabel(t("owner.reports.from")).fill("2024-01-01");
  await expect(page.getByRole("alert")).toHaveText(t("owner.reports.error.long"));
  expect(summaryCalls(calls).length).toBe(before);
});

test("Cash reconciliation lists each shift and flags only those over the threshold", async ({
  page,
}) => {
  await stubCloud(page, ONE, {
    range: {
      b1: (q) =>
        rangeOfBranch("b1", {
          from: q.get("from") ?? "",
          to: q.get("to") ?? "",
          shifts: [
            { business_day: "2026-10-27", shift: shift("s1") },
            {
              business_day: "2026-10-27",
              shift: shift("s2", {
                staff_name: "Ravi",
                counted: 480_000,
                variance: -40_000,
                reason: "counting_error",
                approver_name: "Asha",
                over_threshold: true,
              }),
            },
          ],
        }),
    },
  });
  await page.goto("/reports");
  await page.getByRole("tab", { name: t("owner.reports.tab.cash") }).click();
  const cards = page.getByRole("tabpanel").getByRole("listitem");
  await expect(cards).toHaveCount(2);
  const first = cards.nth(0);
  await expect(first).toContainText("Meera");
  await expect(first).toContainText("₹1,000.00");
  await expect(first).toContainText("₹5,200.00");
  await expect(first).toContainText("₹5,150.00");
  await expect(first).toContainText(t("owner.today.variance.short", { amount: "−₹50.00" }));
  await expect(first).toContainText("09:00");
  await expect(first).toContainText("17:00");
  await expect(first.getByText(t("owner.today.variance.threshold"))).toHaveCount(0);
  const second = cards.nth(1);
  await expect(second).toContainText("Ravi");
  await expect(second).toContainText(t("hub.shift.close.reason.counting_error"));
  await expect(second).toContainText("Asha");
  await expect(second.getByText(t("owner.today.variance.threshold"))).toBeVisible();
});

test("a range with no closed shift says so", async ({ page }) => {
  await stubCloud(page, ONE);
  await page.goto("/reports");
  await page.getByRole("tab", { name: t("owner.reports.tab.cash") }).click();
  await expect(page.getByText(t("owner.reports.cash.empty"))).toBeVisible();
});

test("All branches shows one total per currency and never mixes them", async ({ page }) => {
  const calls = await stubCloud(page, TWO, {
    range: {
      b1: REVENUE,
      b3: (q) =>
        rangeOfBranch("b3", {
          currency: "AED",
          from: q.get("from") ?? "",
          to: q.get("to") ?? "",
          totals: { ...totals(1_000_000), days: 2 },
        }),
    },
    live: {
      b3: liveOf("b3", { currency: "AED" }, { currency: "AED", business_day: "2000-01-01" }),
    },
  });
  await page.goto("/reports");
  const main = page.getByRole("main");
  const inr = main.getByRole("region", { name: t("owner.reports.currency", { currency: "INR" }) });
  const aed = main.getByRole("region", { name: t("owner.reports.currency", { currency: "AED" }) });
  await expect(inr).toContainText("₹90,000.00");
  await expect(inr).not.toContainText("AED");
  await expect(aed).toContainText("10,000.00");
  await expect(aed).not.toContainText("₹");

  // One branch picked: only its figures are asked for.
  const before = calls.length;
  await pick(page, "owner.reports.branch", DUBAI.id);
  await expect(main.getByRole("heading", { name: t("owner.reports.tab.revenue") })).toBeVisible();
  expect(calls.slice(before).filter((c) => c.includes("/b1/"))).toEqual([]);
});

test("branches in setup and Free branches are gated and never asked for figures", async ({
  page,
}) => {
  const calls = await stubCloud(page, [
    PAID[0],
    inSetup({ ...DUBAI }),
    { ...PAID[1], id: "b4", name: "Pixel Arena Free" },
  ]);
  await page.goto("/reports");
  await expect(
    page
      .getByRole("region", { name: DUBAI.name })
      .getByText(t("owner.setup.after_live", { branch: DUBAI.name })),
  ).toBeVisible();
  await expect(
    page
      .getByRole("region", { name: "Pixel Arena Free" })
      .getByText(t("staff.tier.available_on", { tier: "Basic" })),
  ).toBeVisible();
  await expect(page.getByLabel(t("owner.reports.branch"))).not.toContainText(DUBAI.name);
  expect(calls.filter((c) => /\/(b3|b4)\//.test(c))).toEqual([]);
  expect(calls.filter((c) => c.includes("/b1/")).length).toBeGreaterThan(0);
});

test("offline, a report already opened shows from its saved copy", async ({ page }) => {
  await stubCloud(page, ONE, { range: { b1: REVENUE } });
  await page.goto("/reports");
  await expect(page.getByRole("main").getByText("₹90,000.00").first()).toBeVisible();
  await page.route("**/v1/branches/b1/**", (route) => route.abort());
  await pick(page, "owner.reports.range", "today");
  await pick(page, "owner.reports.range", "week");
  await expect(page.getByRole("main").getByText("₹90,000.00").first()).toBeVisible();
  await expect(page.getByText(/^Updated /)).toBeVisible();
});

/** The date `n` days from today at the branch (negative: before). */
const dayOffset = (n: number) =>
  new Date(Date.parse(today() + "T00:00:00Z") + n * 86_400_000).toISOString().slice(0, 10);

const lvl = (id: string, name: string, category: string, qty: number, low?: number) => ({
  product_id: id,
  name,
  category,
  qty,
  ...(low === undefined ? {} : { low_qty: low }),
});

const wasteOf = (
  id: string,
  name: string,
  category: string,
  reason: string,
  qty: number,
  value: number,
) => ({ product_id: id, name, category, reason, qty, value });

const stockOf = (over: Partial<OwnerRangeStock> = {}): OwnerRangeStock => ({
  levels_as_of: dayOffset(-1),
  levels: [],
  waste: [],
  expiring: [],
  ...over,
});

const withStock = (id: string, stock: OwnerRangeStock) => (q: URLSearchParams) =>
  rangeOfBranch(id, { from: q.get("from") ?? "", to: q.get("to") ?? "", stock });

const openStock = async (page: Page) => {
  await page.goto("/reports");
  await page.getByRole("tab", { name: t("owner.reports.tab.stock") }).click();
};

test("Stock shows levels with their date, low stock, waste with value and expiring lots", async ({
  page,
}) => {
  await stubCloud(page, ONE, {
    range: {
      b1: withStock(
        "b1",
        stockOf({
          levels: [lvl("p1", "Cola", "Drinks", 3, 5), lvl("p2", "Chips", "Snacks", 20, 5)],
          waste: [
            wasteOf("p1", "Cola", "Drinks", "spoiled", 3, 6_000),
            wasteOf("p2", "Chips", "Snacks", "expired", 1, 1_500),
          ],
          expiring: [
            { product_id: "p1", name: "Cola", lot: "L-NEW", qty: 4, expires_on: dayOffset(5) },
            { product_id: "p1", name: "Cola", lot: "L-OLD", qty: 2, expires_on: dayOffset(-1) },
            { product_id: "p2", name: "Chips", lot: "L-FAR", qty: 9, expires_on: dayOffset(9) },
          ],
        }),
      ),
    },
    live: {
      b1: liveOf(
        "b1",
        {},
        {
          attention: [
            {
              kind: "low_stock",
              ref: "p1",
              text_params: { product: "Cola", qty: "3", threshold: "5" },
            },
          ],
        },
      ),
    },
  });
  await openStock(page);
  const panel = page.getByRole("tabpanel");
  await expect(
    panel.getByText(t("owner.reports.stock.as_of", { date: dayOffset(-1) })),
  ).toBeVisible();

  const levels = panel.getByRole("region", { name: t("owner.reports.stock.levels") });
  await expect(levels.getByRole("row", { name: /Cola/ })).toContainText("Drinks");
  await expect(levels.getByRole("row", { name: /Chips/ })).toContainText("20");

  const branch = "Pixel Arena MG Road";
  await expect(
    panel.getByText(t("owner.today.attention.low_stock", { branch, product: "Cola", qty: "3" })),
  ).toBeVisible();
  await expect(
    panel.getByText(t("owner.reports.stock.low.line", { branch, product: "Cola", qty: 3, low: 5 })),
  ).toBeVisible();
  await expect(panel.getByText(/Chips has/)).toHaveCount(0);

  const waste = panel.getByRole("region", { name: t("owner.reports.stock.waste") });
  await expect(waste.getByRole("row", { name: /Cola/ })).toContainText(t("reason.spoiled"));
  await expect(waste.getByRole("row", { name: /Cola/ })).toContainText("₹60.00");
  await expect(waste.getByRole("row", { name: /Chips/ })).toContainText(
    t("owner.reports.stock.waste.expired"),
  );

  const expiring = panel.getByRole("region", { name: t("owner.reports.stock.expiring") });
  await expect(expiring.getByRole("row", { name: /L-NEW/ })).toContainText(dayOffset(5));
  await expect(expiring.getByRole("row", { name: /L-OLD/ })).toContainText(
    t("owner.reports.stock.expired"),
  );
  await expect(expiring.getByText("L-FAR")).toHaveCount(0);
});

test("Stock reads the last week's rollups for levels and the picked range for waste", async ({
  page,
}) => {
  await stubCloud(page, ONE, { range: { b1: withStock("b1", stockOf()) } });
  const queries: string[] = [];
  await page.route("**/v1/branches/b1/summary?**", (route) => {
    queries.push(new URL(route.request().url()).search);
    return route.fallback();
  });
  await openStock(page);
  await expect(page.getByText(t("owner.reports.stock.none"))).toBeVisible();
  expect(queries).toContain(`?from=${dayOffset(-6)}&to=${today()}&group_by=day`);
  await pick(page, "owner.reports.range", "month");
  await expect
    .poll(() => queries.at(-1))
    .toBe(`?from=${today().slice(0, 8)}01&to=${today()}&group_by=day`);
});

test("a branch with no tracked products says so, and one with no rollup says no figures yet", async ({
  page,
}) => {
  await stubCloud(page, TWO, {
    range: {
      b1: withStock("b1", stockOf()),
      b3: withStock("b3", stockOf({ levels_as_of: null })),
    },
  });
  await openStock(page);
  const panel = page.getByRole("tabpanel");
  await expect(panel).toContainText(t("owner.reports.stock.none"));
  await expect(panel).toContainText(t("owner.reports.stock.no_data"));
  await expect(panel.getByRole("region", { name: t("owner.reports.stock.levels") })).toHaveCount(0);
});

test("All branches lists levels and waste per branch and in total, matched by name and category", async ({
  page,
}) => {
  await stubCloud(page, TWO, {
    range: {
      b1: withStock(
        "b1",
        stockOf({
          levels: [lvl("p1", "Cola", "Drinks", 3, 5), lvl("p2", "Chips", "Snacks", 20)],
          waste: [wasteOf("p1", "Cola", "Drinks", "spoiled", 1, 2_000)],
        }),
      ),
      b3: withStock(
        "b3",
        stockOf({
          levels: [lvl("q1", " cola ", "drinks", 8, 5), lvl("q2", "Cola", "Mixers", 6)],
          waste: [wasteOf("q1", "cola", "drinks", "spoiled", 2, 3_000)],
        }),
      ),
    },
  });
  await openStock(page);
  const panel = page.getByRole("tabpanel");
  const levels = panel.getByRole("region", { name: t("owner.reports.stock.levels") });
  await expect(levels.getByRole("columnheader", { name: DUBAI.name })).toBeVisible();
  const dash = t("owner.reports.stock.missing");
  // Same name and category in both branches: one row with both quantities and their total.
  await expect(levels.getByRole("row", { name: /Drinks/ }).getByRole("cell")).toHaveText([
    "Drinks",
    "3",
    "8",
    "11",
  ]);
  // Chips only at the first branch; the same name in another category is another product.
  await expect(levels.getByRole("row", { name: /Chips/ }).getByRole("cell")).toHaveText([
    "Snacks",
    "20",
    dash,
    "20",
  ]);
  await expect(levels.getByRole("row", { name: /Mixers/ }).getByRole("cell")).toHaveText([
    "Mixers",
    dash,
    "6",
    "6",
  ]);

  const waste = panel.getByRole("region", { name: t("owner.reports.stock.waste") });
  await expect(waste.getByRole("row", { name: /Cola/ }).getByRole("cell")).toHaveText([
    t("reason.spoiled"),
    "1 · ₹20.00",
    "2 · ₹30.00",
    "3 · ₹50.00",
  ]);
});

test("All branches keeps waste value in one section per currency", async ({ page }) => {
  await stubCloud(page, TWO, {
    range: {
      b1: withStock(
        "b1",
        stockOf({
          levels: [lvl("p1", "Cola", "Drinks", 3)],
          waste: [wasteOf("p1", "Cola", "Drinks", "broken", 1, 2_000)],
        }),
      ),
      b3: (q) =>
        rangeOfBranch("b3", {
          currency: "AED",
          from: q.get("from") ?? "",
          to: q.get("to") ?? "",
          stock: stockOf({
            levels: [lvl("q1", "Cola", "Drinks", 8)],
            waste: [wasteOf("q1", "Cola", "Drinks", "broken", 2, 3_000)],
          }),
        }),
    },
    live: { b3: liveOf("b3", { currency: "AED" }, { currency: "AED" }) },
  });
  await openStock(page);
  const main = page.getByRole("main");
  const inr = main.getByRole("region", { name: t("owner.reports.currency", { currency: "INR" }) });
  const aed = main.getByRole("region", { name: t("owner.reports.currency", { currency: "AED" }) });
  await expect(inr).toContainText("₹20.00");
  await expect(inr).not.toContainText("30.00");
  await expect(aed).toContainText("30.00");
  await expect(aed).not.toContainText("₹");
});

test("Stock honours the gates: a branch in setup is never asked for stock", async ({ page }) => {
  const calls = await stubCloud(page, [PAID[0], inSetup({ ...DUBAI })]);
  await openStock(page);
  await expect(page.getByText(t("owner.setup.after_live", { branch: DUBAI.name }))).toBeVisible();
  expect(calls.filter((c) => c.includes("/b3/"))).toEqual([]);
});

for (const theme of ["light", "dark", "hc"] as const) {
  for (const tab of ["revenue", "cash", "stock"] as const) {
    test(`Reports ${tab} (${theme}): axe finds nothing serious and nothing overflows`, async ({
      page,
    }) => {
      await stubCloud(page, TWO, {
        range: {
          b1: (q) => ({
            ...REVENUE(q),
            missing_days: ["2026-10-28"],
            shifts: [
              {
                business_day: "2026-10-27",
                shift: shift("s1", {
                  over_threshold: true,
                  reason: "other",
                  approver_name: "Asha",
                }),
              },
            ],
          }),
          b3: (q) =>
            rangeOfBranch("b3", {
              currency: "AED",
              from: q.get("from") ?? "",
              to: q.get("to") ?? "",
              stock: stockOf({
                levels: [lvl("q1", "Cola", "Drinks", 2, 5)],
                waste: [wasteOf("q1", "Cola", "Drinks", "free", 1, 500)],
                expiring: [
                  { product_id: "q1", name: "Cola", lot: "L1", qty: 1, expires_on: dayOffset(-1) },
                ],
              }),
            }),
        },
      });
      await page.addInitScript((v) => localStorage.setItem("arena.theme", v), theme);
      await page.goto("/reports");
      if (tab !== "revenue")
        await page.getByRole("tab", { name: t(`owner.reports.tab.${tab}`) }).click();
      await expect(page.getByText(t("owner.loading"))).toHaveCount(0);
      await page.evaluate(() => document.fonts.ready);
      expect(
        await page.evaluate(
          () => document.documentElement.scrollWidth > document.documentElement.clientWidth,
        ),
      ).toBe(false);
      const blocking = (await new AxeBuilder({ page }).analyze()).violations.filter(
        (v) => v.impact === "serious" || v.impact === "critical",
      );
      const report = blocking.map(
        (v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`,
      );
      expect(report, report.join("\n")).toEqual([]);
    });
  }
}
