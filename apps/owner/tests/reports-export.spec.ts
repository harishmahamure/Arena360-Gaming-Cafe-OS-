import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type { OwnerRange } from "@arena/ui/generated";
import { PAID, inSetup, rangeOfBranch, stubCloud, t } from "./stub";

const ONE = [PAID[0]];
const DUBAI = { ...PAID[0], id: "b3", name: "Pixel Arena Dubai", branch_code: "DXB" };
const TWO = [PAID[0], DUBAI];

const REVENUE = (query: URLSearchParams): OwnerRange =>
  rangeOfBranch("b1", { from: query.get("from") ?? "", to: query.get("to") ?? "" });

/** `window.print` is recorded instead of opening the browser's print dialog. */
const recordPrint = (page: Page) =>
  page.addInitScript(() => {
    const w = window as unknown as { printed: number };
    w.printed = 0;
    window.print = () => {
      w.printed++;
    };
  });
const printed = (page: Page) =>
  page.evaluate(() => (window as unknown as { printed: number }).printed);

async function customRange(page: Page, from: string, to: string) {
  await page.getByLabel(t("owner.reports.range")).selectOption("custom");
  await page.getByLabel(t("owner.reports.from")).fill(from);
  await page.getByLabel(t("owner.reports.to")).fill(to);
}

const csvButton = (page: Page) => page.getByRole("button", { name: t("owner.reports.export.csv") });

test("Export → CSV downloads each report of the picked range and branch from the cloud", async ({
  page,
}) => {
  const calls = await stubCloud(page, ONE, { range: { b1: REVENUE } });
  await page.goto("/reports");
  await customRange(page, "2026-10-20", "2026-10-27");
  for (const tab of ["revenue", "cash", "stock"] as const) {
    await page.getByRole("tab", { name: t(`owner.reports.tab.${tab}`) }).click();
    const download = page.waitForEvent("download");
    await csvButton(page).click();
    const file = await download;
    expect(file.suggestedFilename()).toBe(`arena360-${tab}-2026-10-20-to-2026-10-27.csv`);
  }
  expect(calls.exports).toEqual(
    ["revenue", "cash", "stock"].map(
      (r) => `/v1/branches/b1/export?from=2026-10-20&to=2026-10-27&report=${r}`,
    ),
  );
});

test("All branches exports through all, narrowed to the branches shown", async ({ page }) => {
  const calls = await stubCloud(page, TWO);
  await page.goto("/reports");
  const download = page.waitForEvent("download");
  await csvButton(page).click();
  await download;
  expect(calls.exports).toHaveLength(1);
  expect(calls.exports[0]).toMatch(
    /^\/v1\/branches\/all\/export\?from=.*&report=revenue&branches=b1,b3$/,
  );
});

test("a branch in setup is left out of the export", async ({ page }) => {
  const calls = await stubCloud(page, [...ONE, inSetup(DUBAI)]);
  await page.goto("/reports");
  const download = page.waitForEvent("download");
  await csvButton(page).click();
  await download;
  expect(calls.exports[0]).toContain("/v1/branches/b1/export?");
});

test("a failed export says so and downloads nothing", async ({ page }) => {
  await stubCloud(page, ONE, { exportStatus: 503 });
  await page.goto("/reports");
  let downloads = 0;
  page.on("download", () => downloads++);
  await csvButton(page).click();
  await expect(page.getByRole("alert")).toHaveText(t("owner.reports.export.failed"));
  expect(downloads).toBe(0);
});

test("Export → PDF prints the report with branch, range, generated time and data age", async ({
  page,
}) => {
  await stubCloud(page, ONE, { range: { b1: REVENUE } });
  await recordPrint(page);
  await page.goto("/reports");
  await page.getByRole("tab", { name: t("owner.reports.tab.cash") }).click();
  await expect(page.getByText(t("owner.loading"))).toHaveCount(0);
  await page.getByRole("button", { name: t("owner.reports.export.pdf") }).click();
  await expect.poll(() => printed(page)).toBe(1);

  const header = page.locator(".print-header");
  await expect(header).toBeHidden();
  await page.emulateMedia({ media: "print" });
  await expect(header).toBeVisible();
  await expect(header).toContainText(t("owner.reports.tab.cash"));
  await expect(header).toContainText(t("owner.reports.print.branches", { names: ONE[0].name }));
  await expect(header).toContainText("Range:");
  await expect(header).toContainText("Generated:");
  await expect(header).toContainText("Data age:");
  // The chrome is left off the page.
  await expect(page.getByRole("navigation")).toBeHidden();
  await expect(csvButton(page)).toBeHidden();
});

test("Reports → Exports lists the exports of the current range in one place", async ({ page }) => {
  const calls = await stubCloud(page, ONE, { range: { b1: REVENUE } });
  await recordPrint(page);
  await page.goto("/reports");
  await customRange(page, "2026-10-20", "2026-10-27");
  await page.getByRole("tab", { name: t("owner.reports.tab.exports") }).click();
  const panel = page.getByRole("tabpanel");
  await expect(
    panel.getByText(
      t("owner.reports.exports.scope", {
        from: "2026-10-20",
        to: "2026-10-27",
        branches: ONE[0].name,
      }),
    ),
  ).toBeVisible();
  const items = panel.getByRole("listitem");
  await expect(items).toHaveCount(3);
  await expect(items.nth(0)).toContainText(t("owner.reports.tab.revenue"));
  await expect(items.nth(1)).toContainText(t("owner.reports.tab.cash"));
  await expect(items.nth(2)).toContainText(t("owner.reports.tab.stock"));

  const download = page.waitForEvent("download");
  await items
    .nth(1)
    .getByRole("button", { name: t("owner.reports.export.csv") })
    .click();
  expect((await download).suggestedFilename()).toBe("arena360-cash-2026-10-20-to-2026-10-27.csv");
  expect(calls.exports.at(-1)).toContain("report=cash");

  // PDF opens that report for printing.
  await items
    .nth(0)
    .getByRole("button", { name: t("owner.reports.export.pdf") })
    .click();
  await expect.poll(() => printed(page)).toBe(1);
  await expect(page.getByRole("tab", { name: t("owner.reports.tab.revenue") })).toHaveAttribute(
    "aria-selected",
    "true",
  );
});

for (const theme of ["light", "dark", "hc"] as const) {
  test(`Reports exports (${theme}): axe finds nothing serious and nothing overflows`, async ({
    page,
  }) => {
    await stubCloud(page, TWO);
    await page.addInitScript((v) => localStorage.setItem("arena.theme", v), theme);
    await page.goto("/reports");
    await page.getByRole("tab", { name: t("owner.reports.tab.exports") }).click();
    await expect(page.getByRole("tabpanel").getByRole("listitem")).toHaveCount(3);
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
