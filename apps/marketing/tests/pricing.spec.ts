import { expect, test, type Page } from "@playwright/test";
import { DESKTOP, PHONE, open, t } from "./helpers";

// Pricing: tabs and billing cycle in the URL, three equal cards, yearly prices, the comparison
// table, add-ons, the FAQ and the phone layout. Amounts come from the price-book snapshot.

const cards = (page: Page) => page.locator(".tiers > li");
/** The price a card shows now: the other cycle's price is in the page but hidden. */
const shown = (page: Page, i: number) => cards(page).nth(i).locator(".tier-price:visible");

test("Cafés is selected, in the URL, with a Partners tab that says it is coming soon", async ({
  page,
}) => {
  await open(page, "/pricing");
  const tabs = page.getByRole("tablist", { name: t("site.pricing.audience") });
  const cafes = tabs.getByRole("tab", { name: t("site.pricing.tab.cafes") });
  await expect(cafes).toHaveAttribute("aria-selected", "true");
  await expect(page).toHaveURL(/\/pricing\?tab=cafes/);

  await cafes.focus();
  await page.keyboard.press("ArrowRight");
  await expect(tabs.getByRole("tab", { name: t("site.pricing.tab.partners") })).toBeFocused();
  await expect(page).toHaveURL(/tab=partners/);
  await expect(page.getByText(t("site.pricing.partners.soon"))).toBeVisible();
  await expect(cards(page).first()).toBeHidden();
  await page.keyboard.press("ArrowLeft");
  await expect(cards(page).first()).toBeVisible();
});

test("a URL naming Partners or Yearly opens on it, and nonsense falls back", async ({ page }) => {
  await open(page, "/pricing?tab=partners");
  await expect(page.getByText(t("site.pricing.partners.soon"))).toBeVisible();
  await expect(page.getByRole("tab", { name: t("site.pricing.tab.partners") })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  await open(page, "/pricing?tab=cafes&billing=yearly");
  await expect(page.getByRole("tab", { name: t("site.pricing.billing.yearly") })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  await expect(shown(page, 1)).toContainText("₹2,000");
  await open(page, "/pricing?tab=nonsense&billing=weekly");
  await expect(page.getByRole("tab", { name: t("site.pricing.tab.cafes") })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  await expect(page.getByRole("tab", { name: t("site.pricing.billing.monthly") })).toHaveAttribute(
    "aria-selected",
    "true",
  );
});

test("one Get started sits above Monthly · Yearly, then three equal plan cards", async ({
  page,
}) => {
  await page.setViewportSize(DESKTOP);
  await open(page, "/pricing");
  const start = page.locator(".pricing-cta").getByRole("link", { name: t("site.cta.get_started") });
  await expect(start).toHaveAttribute("href", "https://start.arena360.cloud");
  const cycle = (await page
    .getByRole("tablist", { name: t("site.pricing.billing") })
    .boundingBox())!;
  const first = (await cards(page).first().boundingBox())!;
  expect((await start.boundingBox())!.y).toBeLessThan(cycle.y);
  expect(cycle.y).toBeLessThan(first.y);

  await expect(cards(page)).toHaveCount(3);
  const boxes = await cards(page).evaluateAll((els) =>
    els.map((el) => {
      const r = el.getBoundingClientRect();
      return { w: Math.round(r.width), h: Math.round(r.height), y: Math.round(r.top) };
    }),
  );
  expect(new Set(boxes.map((b) => b.w)).size).toBe(1);
  expect(new Set(boxes.map((b) => b.y)).size).toBe(1);
  expect(new Set(boxes.map((b) => b.h)).size).toBe(1);

  await expect(cards(page).nth(0)).toContainText("₹0");
  await expect(cards(page).nth(0)).toContainText("1 branch");
  await expect(cards(page).nth(0)).toContainText("Ads on the Android Hub");
  await expect(shown(page, 1)).toHaveText("₹200/branch/month + 18% GST");
  await expect(shown(page, 2)).toHaveText("₹1,999/branch/month + 18% GST");
  await expect(page.getByText(t("site.pricing.gst_note", { gst: 18 }))).toBeVisible();
});

test("each Choose <tier> leads to Get started with the tier pre-selected", async ({ page }) => {
  await open(page, "/pricing");
  const names = { free: "Free", basic: "Basic", premium: "Premium" };
  for (const [i, tier] of (["free", "basic", "premium"] as const).entries()) {
    const link = cards(page).nth(i).getByRole("link");
    await expect(link).toHaveText(t("site.pricing.choose", { tier: names[tier] }));
    await expect(link).toHaveAttribute("href", `https://start.arena360.cloud?tier=${tier}`);
  }
});

test("Yearly swaps the prices, shows 2 months free and announces the change", async ({ page }) => {
  await page.setViewportSize(DESKTOP);
  await open(page, "/pricing");
  const status = page.getByRole("status");
  await page.getByRole("tab", { name: t("site.pricing.billing.yearly") }).click();
  await expect(page).toHaveURL(/billing=yearly/);
  await expect(shown(page, 0)).toHaveText("₹0· 1 branch");
  await expect(shown(page, 1)).toHaveText("₹2,000/branch/year + 18% GST2 months free");
  await expect(shown(page, 2)).toHaveText("₹19,990/branch/year + 18% GST2 months free");
  await expect(status).toHaveText(
    t("site.pricing.announce.yearly", { basic: "₹2,000", premium: "₹19,990", gst: 18, months: 2 }),
  );

  await page.getByRole("tab", { name: t("site.pricing.billing.monthly") }).click();
  await expect(shown(page, 1)).toHaveText("₹200/branch/month + 18% GST");
  await expect(status).toHaveText(
    t("site.pricing.announce.monthly", { basic: "₹200", premium: "₹1,999", gst: 18 }),
  );
});

test("the comparison table lists each tier's limits from the tier table", async ({ page }) => {
  await page.setViewportSize(DESKTOP);
  await open(page, "/pricing");
  const table = page.getByRole("table");
  const texts = async (name: string) => {
    const row = table.getByRole("row").filter({
      has: page.getByRole("rowheader", { name, exact: true }),
    });
    return (await row.getByRole("cell").allInnerTexts()).map((s) => s.replace(/\s+/g, " ").trim());
  };
  await expect(table.getByRole("columnheader")).toHaveText([
    "Capability",
    "Free",
    "Basic",
    "Premium",
  ]);
  // The visually hidden "Included" text is part of a cell's text.
  const yes = "✓ Included";
  const no = "— Not included";
  expect(await texts("Branches")).toEqual(["1", "Unlimited", "Unlimited"]);
  expect(await texts("Devices")).toEqual(["20", "60", "200"]);
  expect(await texts("Users")).toEqual(["3", "Unlimited", "Unlimited"]);
  expect(await texts("Owner dashboards")).toEqual([no, yes, yes]);
  expect(await texts("Cloud backup and retention")).toEqual([
    "Manual export",
    "Every 15 min, kept 1 year",
    "Every 5 min, kept 3 years",
  ]);
  expect(await texts("Standby Hub")).toEqual([no, yes, yes]);
  expect(await texts("Tunnel for remote devices")).toEqual([no, "Add-on", yes]);
  expect(await texts("Kitchen display")).toEqual([no, yes, yes]);
  expect(await texts("Ads")).toEqual(["On the Android Hub", "None", "None"]);
});

test("add-ons are per branch per month + GST and no rewarded ads appear", async ({ page }) => {
  await open(page, "/pricing");
  const items = page.locator(".addons li");
  await expect(items).toHaveText([
    "+20 devices ₹50",
    "Tunnel for remote devices ₹200",
    "Additional owner login ₹50 — available soon",
  ]);
  await expect(page.getByText(t("site.pricing.addons.note", { gst: 18 }))).toBeVisible();
  expect(await page.locator("body").innerText()).not.toMatch(/rewarded/i);
});

test("the FAQ is a disclosure list with the exact refund rule", async ({ page }) => {
  await open(page, "/pricing");
  const refund =
    "Full refund within 7 days of your first payment. After that, cancel anytime: your plan stays active to the end of the paid period, then the branch drops to Free. No partial refunds.";
  expect(t("site.pricing.faq.refund.a")).toBe(refund);
  await expect(page.locator(".faq details")).toHaveCount(3);
  const answer = page.getByText(refund, { exact: true });
  await expect(answer).toBeHidden();
  await page.getByText(t("site.pricing.faq.refund.q")).click();
  await expect(answer).toBeVisible();
  await page.getByText(t("site.pricing.faq.offline.q")).focus();
  await page.keyboard.press("Enter");
  await expect(page.getByText(t("site.pricing.faq.offline.a"))).toBeVisible();
});

test("at 390 px the cards stack in order and only the table scrolls, with its first column pinned", async ({
  page,
}) => {
  await page.setViewportSize(PHONE);
  await open(page, "/pricing");
  const rects = await cards(page).evaluateAll((els) =>
    els.map((el) => {
      const r = el.getBoundingClientRect();
      return { left: Math.round(r.left), top: r.top };
    }),
  );
  expect(rects[0]!.top).toBeLessThan(rects[1]!.top);
  expect(rects[1]!.top).toBeLessThan(rects[2]!.top);
  expect(new Set(rects.map((r) => r.left)).size).toBe(1);

  const scroller = page.locator(".table-scroll");
  const dims = await scroller.evaluate((el) => ({ sw: el.scrollWidth, cw: el.clientWidth }));
  expect(dims.sw).toBeGreaterThan(dims.cw);
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  );
  expect(overflow).toBeLessThanOrEqual(0);

  const header = page.getByRole("rowheader", { name: "Devices", exact: true });
  const before = (await header.boundingBox())!.x;
  await scroller.evaluate((el) => (el.scrollLeft = 200));
  expect(Math.abs((await header.boundingBox())!.x - before)).toBeLessThan(2);
});
