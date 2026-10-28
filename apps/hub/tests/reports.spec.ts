import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type { DailySales, SessionsReport } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { addDays, businessDayNow, daySpan, durationText, bpText } from "../src/lib/reports";
import { CORS, HUB, device, json as ok, snapshot } from "./floor-fixture";

// The Reports screen (Story 5.27): the list, Daily sales and Sessions. The Hub's loopback API is
// stubbed with `page.route`; the stub records every range the page asked for.

const hours = Array.from({ length: 24 }, (_, hour) => ({ hour, amount_minor: 0, count: 0 }));
hours[18] = { hour: 18, amount_minor: 88_000, count: 3 };
hours[9] = { hour: 9, amount_minor: 44_000, count: 1 };

const SALES: DailySales = {
  from: "2026-10-28",
  to: "2026-10-28",
  orders: 4,
  sales_minor: 132_000,
  refunds_minor: 12_000,
  net_minor: 120_000,
  tax_minor: 18_000,
  by_type: [
    { key: "play_time", amount_minor: 90_000, count: 2 },
    { key: "products", amount_minor: 30_000, count: 5 },
    { key: "plans", amount_minor: 20_000, count: 1 },
    { key: "discounts", amount_minor: -8_000, count: 1 },
    { key: "tax_added", amount_minor: 0, count: 0 },
  ],
  by_hour: hours,
  by_group: [
    { id: "g1", name: "Gaming PCs", amount_minor: 100_000, count: 3 },
    { id: "", name: "", amount_minor: 32_000, count: 1 },
  ],
  by_category: [
    { id: "Drinks", name: "Drinks", amount_minor: 20_000, count: 4 },
    { id: "Snacks", name: "Snacks", amount_minor: 10_000, count: 1 },
  ],
  received: [
    { key: "topup", amount_minor: 50_000, count: 2 },
    { key: "credit_collect", amount_minor: 0, count: 0 },
  ],
};

const util = Array.from({ length: 24 }, () => 0);
util[18] = 6250;
util[19] = 1250;

const SESSIONS: SessionsReport = {
  from: "2026-10-28",
  to: "2026-10-28",
  days: 1,
  sessions: 3,
  minutes: 135,
  revenue_minor: 22_500,
  by_group: [
    {
      group_id: "g1",
      name: "Gaming PCs",
      devices: 2,
      sessions: 3,
      minutes: 135,
      revenue_minor: 22_500,
      utilisation_bp: util,
    },
  ],
  by_device: [
    {
      device_id: "d1",
      label: "PC 1",
      group_id: "g1",
      sessions: 2,
      minutes: 105,
      revenue_minor: 17_500,
    },
    {
      device_id: "d2",
      label: "PC 2",
      group_id: "g1",
      sessions: 1,
      minutes: 30,
      revenue_minor: 5_000,
    },
  ],
};

type Hub = { asked: string[] };

async function open(page: Page, permissions: string[], path = "/reports"): Promise<Hub> {
  const hub: Hub = { asked: [] };
  await page.addInitScript(() => sessionStorage.setItem("arena.hub.token", "tok"));
  await page.route(`${HUB}/**`, async (route: Route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    const url = new URL(req.url());
    switch (url.pathname) {
      case "/ui/snapshot":
        return ok(route, snapshot([device(1, {})]));
      case "/ui/me":
        return ok(route, {
          staff_id: "01JMANAGER0000000000000000",
          display_name: "Ravi",
          role_name: "Manager",
          permissions,
          locked: false,
          has_pin: true,
        });
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: `retry: 300\n\n`,
        });
      case "/ui/reports/daily-sales":
      case "/ui/reports/sessions": {
        const from = url.searchParams.get("from") ?? "";
        const to = url.searchParams.get("to") ?? "";
        hub.asked.push(`${url.pathname.split("/").pop()} ${from} ${to}`);
        if (daySpan(from, to) < 1 || daySpan(from, to) > 90) {
          return route.fulfill({
            status: 422,
            contentType: "application/json",
            headers: CORS,
            body: JSON.stringify({ code: "VALIDATION_FAILED", message: "no", field: "to" }),
          });
        }
        return ok(route, url.pathname.endsWith("sessions") ? SESSIONS : SALES);
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto(path);
  return hub;
}

test("range helpers do calendar arithmetic and find the business day", () => {
  expect(addDays("2026-10-31", 1)).toBe("2026-11-01");
  expect(addDays("2026-03-01", -1)).toBe("2026-02-28");
  expect(daySpan("2026-10-01", "2026-10-01")).toBe(1);
  expect(daySpan("2026-10-02", "2026-10-01")).toBe(0);
  expect(daySpan("soon", "2026-10-01")).toBe(0);
  expect(durationText(135)).toBe("2:15");
  expect(bpText(6250)).toBe("62.5%");
  const locale = snapshot([]).locale!;
  // 01:00 in Kolkata with a 06:00 day start is still the previous business day.
  const at = Date.UTC(2026, 9, 27, 19, 30);
  expect(businessDayNow(locale, at)).toBe("2026-10-27");
  expect(businessDayNow(locale, at + 5 * 3_600_000)).toBe("2026-10-28");
});

test("the list opens Daily sales and Sessions for a member with report.view", async ({ page }) => {
  await open(page, ["report.view"]);
  await expect(page.getByTestId("report-link-daily-sales")).toContainText(
    en["hub.reports.daily_sales"],
  );
  await expect(page.getByTestId("report-link-sessions")).toContainText(en["hub.reports.sessions"]);
  await page.getByTestId("report-link-sessions").click();
  await expect(page.getByTestId("sessions-report-screen")).toBeVisible();
});

test("without report.view the list says so and offers no report", async ({ page }) => {
  const hub = await open(page, ["pos.sell"]);
  await expect(page.getByTestId("reports-denied")).toHaveText(en["hub.reports.no_access"]);
  await expect(page.getByTestId("report-link-daily-sales")).toHaveCount(0);
  await page.goto("/reports/daily-sales");
  await expect(page.getByTestId("reports-denied")).toBeVisible();
  expect(hub.asked).toEqual([]);
});

test("Daily sales opens on today and shows every breakdown", async ({ page }) => {
  const hub = await open(page, ["report.view"], "/reports/daily-sales");
  await expect(page.getByTestId("sales-gross")).toHaveText("₹1,320.00");
  expect(hub.asked).toHaveLength(1);
  expect(hub.asked[0]).toMatch(/^daily-sales (\d{4}-\d{2}-\d{2}) \1$/);
  await expect(page.getByTestId("sales-refunds")).toHaveText("−₹120.00");
  await expect(page.getByTestId("sales-net")).toHaveText("₹1,200.00");
  await expect(page.getByTestId("sales-tax")).toHaveText("₹180.00");
  await expect(page.getByTestId("sales-orders")).toHaveText("4");
  // Play time, products and plans are separate rows; discounts come off.
  await expect(page.getByTestId("type-play_time")).toContainText("₹900.00");
  await expect(page.getByTestId("type-products")).toContainText("₹300.00");
  await expect(page.getByTestId("type-plans")).toContainText("₹200.00");
  await expect(page.getByTestId("type-discounts")).toContainText("−₹80.00");
  // Only hours with sales show, in order.
  await expect(page.getByTestId("sales-by-hour").locator("tbody tr")).toHaveCount(2);
  await expect(page.getByTestId("hour-9")).toContainText("09:00");
  await expect(page.getByTestId("hour-18")).toContainText("₹880.00");
  await expect(page.getByTestId("group-row").first()).toContainText("Gaming PCs");
  await expect(page.getByTestId("group-row").nth(1)).toContainText(en["hub.reports.counter"]);
  await expect(page.getByTestId("category-row").first()).toContainText("Drinks");
  // Top-ups are not sales.
  await expect(page.getByTestId("received-topup")).toContainText("₹500.00");
  await expect(page.getByRole("heading", { name: en["hub.reports.received_title"] })).toBeVisible();
});

test("a preset changes the range and a bad range is explained, not sent", async ({ page }) => {
  const hub = await open(page, ["report.view"], "/reports/daily-sales");
  await expect(page.getByTestId("sales-gross")).toBeVisible();
  await page.getByRole("button", { name: en["hub.reports.last_7"] }).click();
  await expect.poll(() => hub.asked.length).toBe(2);
  const [kind, from, to] = hub.asked[1].split(" ");
  expect(kind).toBe("daily-sales");
  expect(daySpan(from, to)).toBe(7);
  await page.getByTestId("report-from").fill(addDays(to, 1));
  await expect(page.getByTestId("report-range-problem")).toHaveText(en["hub.reports.range_order"]);
  await page.getByTestId("report-from").fill(addDays(to, -120));
  await expect(page.getByTestId("report-range-problem")).toHaveText(en["hub.reports.range_long"]);
  await page.getByRole("button", { name: en["hub.reports.show"] }).click();
  expect(hub.asked).toHaveLength(2);
});

test("Sessions shows count, billed time and revenue by group and device with utilisation", async ({
  page,
}) => {
  await open(page, ["report.view"], "/reports/sessions");
  await expect(page.getByTestId("sessions-count")).toHaveText("3");
  await expect(page.getByTestId("sessions-minutes")).toHaveText("2:15");
  await expect(page.getByTestId("sessions-revenue")).toHaveText("₹225.00");
  await expect(page.getByTestId("sessions-by-group").locator("tbody tr")).toHaveCount(1);
  await expect(page.getByTestId("sessions-by-group")).toContainText("Gaming PCs");
  await expect(page.getByTestId("device-row")).toHaveCount(2);
  await expect(page.getByTestId("device-row").first()).toContainText("PC 1");
  // Only the hours with use are listed.
  await expect(page.getByTestId("sessions-utilisation").locator("tbody tr")).toHaveCount(2);
  await expect(page.getByTestId("utilisation-18")).toContainText("62.5%");
  await expect(page.getByTestId("utilisation-19")).toContainText("12.5%");
});

test("the report screens have no accessibility violations", async ({ page }) => {
  await open(page, ["report.view"], "/reports/daily-sales");
  await expect(page.getByTestId("sales-gross")).toBeVisible();
  const sales = await new AxeBuilder({ page }).analyze();
  expect(sales.violations).toEqual([]);
  await page.goto("/reports/sessions");
  await expect(page.getByTestId("sessions-count")).toBeVisible();
  const sessions = await new AxeBuilder({ page }).analyze();
  expect(sessions.violations).toEqual([]);
});
