import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type {
  DailySales,
  GstExported,
  GstMonth,
  PlayersReport,
  SessionsReport,
  ShiftList,
  StaffReport,
  ZReport,
} from "@arena/ui/generated";
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

const Z: ZReport = {
  shift_id: "SH1",
  opened_at: Date.UTC(2026, 9, 28, 3, 30),
  closed_at: Date.UTC(2026, 9, 28, 11, 30),
  opened_by: "Meera",
  closed_by: "Asha",
  rebuilt: false,
  orders: 6,
  sales_minor: 240_000,
  tax_minor: 36_000,
  by_type: [
    { key: "play_time", amount_minor: 150_000, count: 3 },
    { key: "products", amount_minor: 100_000, count: 8 },
    { key: "plans", amount_minor: 0, count: 0 },
    { key: "discounts", amount_minor: -10_000, count: 1 },
    { key: "tax_added", amount_minor: 0, count: 0 },
  ],
  payments: [
    { key: "cash", amount_minor: 120_000, count: 3 },
    { key: "upi", amount_minor: 80_000, count: 2 },
    { key: "card", amount_minor: 40_000, count: 1 },
  ],
  references: [
    {
      method: "upi",
      rows: [
        {
          at: Date.UTC(2026, 9, 28, 5, 0),
          invoice_no: "DEV01/26-27/E1k/000007",
          amount_minor: 80_000,
          reference: "1234",
        },
      ],
    },
    {
      method: "card",
      rows: [
        { at: Date.UTC(2026, 9, 28, 6, 0), invoice_no: "", amount_minor: 40_000, reference: "T-9" },
      ],
    },
  ],
  received: [
    { key: "topup", amount_minor: 50_000, count: 2 },
    { key: "credit_collect", amount_minor: 0, count: 0 },
  ],
  refunds: [{ credit_note_no: "DEV01/26-27/E1k/000008", method: "cash", amount_minor: -4_000 }],
  refunds_minor: -4_000,
  waste: [{ reason: "spoiled", qty: 2, value_minor: 4_000 }],
  expenses: [
    {
      category: "supplies",
      reference: "",
      note: "Tissue",
      paid_from: "drawer",
      amount_minor: 5_000,
    },
  ],
  deposits: [{ category: "", reference: "SLIP-1", note: "", paid_from: "", amount_minor: 20_000 }],
  voids: [{ reason: "mistake", count: 1, amount_minor: 9_000 }],
  drawer_opens: 4,
  float_minor: 100_000,
  expected_minor: 191_000,
  counted_minor: 190_500,
  variance_minor: -500,
  variance_reason: "counting_error",
  approver: "Rohan",
};

const SHIFTS: ShiftList = {
  shifts: [
    {
      shift_id: "SH1",
      opened_at: Z.opened_at,
      closed_at: Z.closed_at,
      opened_by: "Meera",
      closed_by: "Asha",
      counted_minor: 190_500,
      variance_minor: -500,
    },
  ],
};

const GST_TAX = JSON.stringify({
  country: "IN",
  gst: true,
  gstin: "29ABCDE1234F1Z5",
  inclusive: true,
  product_bp: 500,
  play_bp: 1800,
  sac: "9996",
  label: "GST",
  rate_bp: 0,
});
const SIMPLE_TAX = JSON.stringify({
  country: "IN",
  gst: false,
  gstin: "",
  inclusive: true,
  product_bp: 500,
  play_bp: 1800,
  sac: "",
  label: "Tax",
  rate_bp: 0,
});

const section = (
  key: string,
  documents: number,
  taxable: number,
  c: number,
  s: number,
  i: number,
) => ({
  key,
  documents,
  taxable_minor: taxable,
  central_minor: c,
  state_minor: s,
  integrated_minor: i,
});

const GST_MONTH: GstMonth = {
  month: "2026-10",
  sections: [
    section("b2b", 2, 30_000, 1_000, 1_000, 500),
    section("b2cs", 3, 100_000, 4_000, 4_000, 0),
    section("cdnr", 1, 10_000, 500, 500, 0),
    section("hsn", 6, 120_000, 4_500, 4_500, 500),
  ],
  invoices: 5,
  credit_notes: 1,
  untaxed: 1,
  unnumbered: 2,
  empty: false,
};

// ---- Story 5.30: Players and Staff ----

const PLAYERS: PlayersReport = {
  from: "2026-10-28",
  to: "2026-10-28",
  top: [
    { player_id: "P1", name: "Asha Rao", erased: false, visits: 3, spend_minor: 90_000 },
    { player_id: "P2", name: "", erased: true, visits: 1, spend_minor: 40_000 },
    { player_id: "P3", name: "Bala", erased: false, visits: 1, spend_minor: 10_000 },
  ],
  guest: { bills: 4, spend_minor: 35_000 },
  new_players: { players: 2, spend_minor: 50_000 },
  returning_players: { players: 1, spend_minor: 90_000 },
  credit_as_of: "2026-10-28",
  credit_buckets: [
    { key: "0_7", amount_minor: 8_000 },
    { key: "8_14", amount_minor: 30_000 },
    { key: "15_plus", amount_minor: 20_000 },
  ],
  credit_outstanding_minor: 58_000,
  debtors: [
    { player_id: "P2", name: "", erased: true, outstanding_minor: 30_000, oldest_age_days: 10 },
    {
      player_id: "P3",
      name: "Bala",
      outstanding_minor: 28_000,
      erased: false,
      oldest_age_days: 20,
    },
  ],
};

const STAFF: StaffReport = {
  from: "2026-10-28",
  to: "2026-10-28",
  staff: [
    {
      staff_id: "S1",
      name: "Meera",
      sales_minor: 120_000,
      sales_count: 6,
      sessions_started: 4,
      discounts_count: 2,
      discounts_minor: 15_000,
      voids_count: 1,
      voids_minor: 12_000,
      refunds_count: 1,
      refunds_minor: 9_000,
      shifts: [{ shift_id: "SH1", closed_at: 1_793_000_000_000, variance_minor: -500 }],
      variance_minor: -500,
    },
    {
      staff_id: "GONE",
      name: "",
      sales_minor: 0,
      sales_count: 0,
      sessions_started: 0,
      discounts_count: 0,
      discounts_minor: 0,
      voids_count: 0,
      voids_minor: 0,
      refunds_count: 2,
      refunds_minor: 1_000,
      shifts: [],
      variance_minor: 0,
    },
  ],
};

type Hub = {
  asked: string[];
  sent: unknown[];
  shiftStatus: number;
  exported: unknown[];
  empty: boolean;
};

async function open(
  page: Page,
  permissions: string[],
  path = "/reports",
  tax: string = GST_TAX,
  empty = false,
): Promise<Hub> {
  const hub: Hub = { asked: [], sent: [], shiftStatus: 200, exported: [], empty };
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
      case "/ui/settings":
        return ok(route, [
          { key: "branch.tax", value: tax, rev: 1 },
          { key: "system.backup_dir", value: "/mnt/usb", rev: 1 },
        ]);
      case "/ui/reports/gst": {
        const month = url.searchParams.get("month") ?? "";
        hub.asked.push(`gst ${month}`);
        if (!/^\d{4}-(0[1-9]|1[0-2])$/.test(month)) {
          return route.fulfill({
            status: 422,
            contentType: "application/json",
            headers: CORS,
            body: JSON.stringify({ code: "VALIDATION_FAILED", message: "m", field: "month" }),
          });
        }
        const none = month === "2026-11";
        return ok(
          route,
          none
            ? {
                ...GST_MONTH,
                month,
                empty: true,
                invoices: 0,
                credit_notes: 0,
                untaxed: 0,
                unnumbered: 0,
              }
            : { ...GST_MONTH, month },
        );
      }
      case "/ui/reports/gst/export": {
        const body = route.request().postDataJSON() as { month: string; folder: string };
        hub.exported.push(body);
        if (body.folder === "/bad") {
          return route.fulfill({
            status: 422,
            contentType: "application/json",
            headers: CORS,
            body: JSON.stringify({
              code: "VALIDATION_FAILED",
              message: "not_writable",
              field: "folder",
            }),
          });
        }
        const done: GstExported = {
          file_name: `GSTR1_DEV01_${body.month}.zip`,
          path: `${body.folder}/GSTR1_DEV01_${body.month}.zip`,
          bytes: 2048,
          month: { ...GST_MONTH, month: body.month },
        };
        return ok(route, done);
      }
      case "/ui/reports/shifts":
        hub.asked.push("shifts");
        return ok(route, SHIFTS);
      case "/ui/reports/shifts/SH1":
      case "/ui/reports/shifts/OPEN":
      case "/ui/reports/shifts/GONE":
        hub.asked.push(url.pathname.split("/").pop() ?? "");
        if (url.pathname.endsWith("OPEN")) {
          return route.fulfill({
            status: 409,
            contentType: "application/json",
            headers: CORS,
            body: JSON.stringify({ code: "STALE", message: "open" }),
          });
        }
        if (url.pathname.endsWith("GONE")) {
          return route.fulfill({
            status: 404,
            contentType: "application/json",
            headers: CORS,
            body: JSON.stringify({ code: "NOT_FOUND", message: "gone" }),
          });
        }
        return ok(route, Z);
      case "/ui/intent":
        hub.sent.push(route.request().postDataJSON().intent.payload);
        if (hub.shiftStatus !== 200) {
          return route.fulfill({
            status: hub.shiftStatus,
            contentType: "application/json",
            headers: CORS,
            body: JSON.stringify({ code: "STALE", message: "x" }),
          });
        }
        return ok(route, { applied: { intent_id: "I", changes: [] } });
      case "/ui/reports/players":
      case "/ui/reports/staff": {
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
        if (hub.empty) {
          return ok(
            route,
            url.pathname.endsWith("staff")
              ? { ...STAFF, staff: [] }
              : {
                  ...PLAYERS,
                  top: [],
                  debtors: [],
                  guest: { bills: 0, spend_minor: 0 },
                  new_players: { players: 0, spend_minor: 0 },
                  returning_players: { players: 0, spend_minor: 0 },
                  credit_outstanding_minor: 0,
                  credit_buckets: PLAYERS.credit_buckets.map((b) => ({ ...b, amount_minor: 0 })),
                },
          );
        }
        return ok(route, url.pathname.endsWith("staff") ? STAFF : PLAYERS);
      }
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

// ---- Story 5.28: Shifts and the Z-report ----

test("the list offers Shifts and the Shifts list opens a closed shift's Z-report", async ({
  page,
}) => {
  const hub = await open(page, ["report.view"]);
  await expect(page.getByTestId("report-link-shifts")).toContainText(en["hub.reports.shifts"]);
  await page.getByTestId("report-link-shifts").click();
  await expect(page.getByTestId("shifts-report-screen")).toBeVisible();
  const row = page.getByTestId("shift-row");
  await expect(row).toHaveCount(1);
  await expect(row).toContainText("Meera");
  await expect(row).toContainText("Asha");
  await expect(row).toContainText("₹1,905.00");
  await expect(row).toContainText("−₹5.00");
  await page.getByTestId("shift-open-SH1").click();
  await expect(page.getByTestId("z-report-screen")).toBeVisible();
  expect(hub.asked).toEqual(["shifts", "SH1"]);
});

test("the Z-report shows every section with the stored figures", async ({ page }) => {
  await open(page, ["report.view"], "/reports/shifts/SH1");
  await expect(page.getByTestId("z-sales-total")).toContainText("₹2,400.00");
  await expect(page.getByTestId("z-type-play_time")).toContainText("₹1,500.00");
  await expect(page.getByTestId("z-type-discounts")).toContainText("−₹100.00");
  await expect(page.getByTestId("z-payment-cash")).toContainText("₹1,200.00");
  await expect(page.getByTestId("z-payment-cash")).toContainText("3");
  await expect(page.getByTestId("z-payment-upi")).toContainText("₹800.00");
  await expect(page.getByTestId("z-received")).toContainText("₹500.00");
  // UPI and card references, each under its method.
  const upi = page.getByTestId("z-references-upi").getByTestId("z-reference-row");
  await expect(upi).toContainText("DEV01/26-27/E1k/000007");
  await expect(upi).toContainText("₹800.00");
  await expect(upi).toContainText("1234");
  const card = page.getByTestId("z-references-card").getByTestId("z-reference-row");
  await expect(card).toContainText("T-9");
  // Refunds are negative lines.
  await expect(page.getByTestId("z-refund-row")).toContainText("DEV01/26-27/E1k/000008");
  await expect(page.getByTestId("z-refund-row")).toContainText("−₹40.00");
  await expect(page.getByTestId("z-refunds-total")).toContainText("−₹40.00");
  await expect(page.getByTestId("z-waste-row")).toContainText(en["reason.spoiled"]);
  await expect(page.getByTestId("z-expense-row")).toContainText(
    en["hub.shift.cash.category.supplies"],
  );
  await expect(page.getByTestId("z-expense-row")).toContainText("₹50.00");
  await expect(page.getByTestId("z-deposit-row")).toContainText("SLIP-1");
  await expect(page.getByTestId("z-void-row")).toContainText(en["reason.mistake"]);
  await expect(page.getByTestId("z-drawer-opens")).toContainText("4");
  // Expected, counted and the signed variance with its reason and approver.
  await expect(page.getByTestId("z-expected")).toHaveText("₹1,910.00");
  await expect(page.getByTestId("z-counted")).toHaveText("₹1,905.00");
  await expect(page.getByTestId("z-variance")).toHaveText("−₹5.00");
  await expect(page.getByTestId("z-reason")).toContainText(
    en["hub.shift.close.reason.counting_error"],
  );
  await expect(page.getByTestId("z-approver")).toHaveText("Rohan");
  await expect(page.getByTestId("z-rebuilt")).toHaveCount(0);
});

test("Reprint sends shift.reprint_z for the shift and says it is queued", async ({ page }) => {
  const hub = await open(page, ["report.view"], "/reports/shifts/SH1");
  await page.getByRole("button", { name: en["hub.zreport.reprint"] }).click();
  await expect(page.getByTestId("z-reprint-status")).toHaveText(en["hub.zreport.reprint_queued"]);
  expect(hub.sent).toEqual([{ kind: "shift.reprint_z", shift_id: "SH1" }]);
});

test("an open or unknown shift is explained and offers no Reprint", async ({ page }) => {
  await open(page, ["report.view"], "/reports/shifts/OPEN");
  await expect(page.getByTestId("report-problem")).toContainText(en["hub.zreport.still_open"]);
  await expect(page.getByRole("button", { name: en["hub.zreport.reprint"] })).toHaveCount(0);
  await page.goto("/reports/shifts/GONE");
  await expect(page.getByTestId("report-problem")).toContainText(en["hub.zreport.not_found"]);
});

test("without report.view the Shifts screens say so and ask for nothing", async ({ page }) => {
  const hub = await open(page, ["pos.sell"], "/reports/shifts");
  await expect(page.getByTestId("reports-denied")).toHaveText(en["hub.reports.no_access"]);
  await page.goto("/reports/shifts/SH1");
  await expect(page.getByTestId("reports-denied")).toBeVisible();
  expect(hub.asked).toEqual([]);
});

test("the Shifts screens have no accessibility violations", async ({ page }) => {
  await open(page, ["report.view"], "/reports/shifts");
  await expect(page.getByTestId("shift-row")).toBeVisible();
  const list = await new AxeBuilder({ page }).analyze();
  expect(list.violations).toEqual([]);
  await page.goto("/reports/shifts/SH1");
  await expect(page.getByTestId("z-sales-total")).toBeVisible();
  const z = await new AxeBuilder({ page }).analyze();
  expect(z.violations).toEqual([]);
});

// ---- Story 5.29: the GST return ----

test("a GST branch lists the GST return and its month shows every section", async ({ page }) => {
  const hub = await open(page, ["report.view"]);
  await expect(page.getByTestId("report-link-gst")).toContainText(en["hub.reports.gst"]);
  await page.getByTestId("report-link-gst").click();
  await expect(page.getByTestId("gst-screen")).toBeVisible();
  await expect(page.getByTestId("gst-month")).toHaveValue(/^\d{4}-\d{2}$/);
  await expect(page.getByTestId("gst-counts")).toHaveText("5 invoices · 1 credit notes");
  await expect(page.getByTestId("gst-section-b2b")).toContainText("₹300.00");
  await expect(page.getByTestId("gst-section-b2b")).toContainText("₹5.00");
  await expect(page.getByTestId("gst-section-b2cs")).toContainText("₹1,000.00");
  await expect(page.getByTestId("gst-section-cdnr")).toContainText("₹100.00");
  await expect(page.getByTestId("gst-section-hsn")).toContainText("₹1,200.00");
  await expect(page.getByTestId("gst-untaxed")).toContainText("1 invoices");
  await expect(page.getByTestId("gst-unnumbered")).toContainText("2 sales");
  expect(hub.asked.at(-1)).toMatch(/^gst \d{4}-\d{2}$/);
});

test("a simple-tax branch does not list the GST return", async ({ page }) => {
  await open(page, ["report.view"], "/reports", SIMPLE_TAX);
  await expect(page.getByTestId("report-link-shifts")).toBeVisible();
  await expect(page.getByTestId("report-link-gst")).toHaveCount(0);
  await page.goto("/reports/gst");
  await expect(page.getByTestId("gst-not-gst")).toHaveText(en["hub.gst.not_gst"]);
});

test("picking a month asks for it, and an empty month says so", async ({ page }) => {
  const hub = await open(page, ["report.view"], "/reports/gst");
  await expect(page.getByTestId("gst-counts")).toBeVisible();
  await page.getByTestId("gst-month").fill("2026-11");
  await page.getByRole("button", { name: en["hub.gst.show"] }).click();
  await expect(page.getByTestId("gst-empty")).toHaveText("No invoices in 2026-11.");
  expect(hub.asked.at(-1)).toBe("gst 2026-11");
});

test("Export writes the month into the folder and says where", async ({ page }) => {
  const hub = await open(page, ["report.view"], "/reports/gst");
  // The backup folder is offered as the first guess.
  await expect(page.getByTestId("gst-folder")).toHaveValue("/mnt/usb");
  await page.getByTestId("gst-month").fill("2026-10");
  await page.getByTestId("gst-folder").fill("/media/usb");
  await page.getByRole("button", { name: en["hub.gst.export"] }).click();
  await expect(page.getByTestId("gst-saved")).toHaveText(
    "Saved /media/usb/GSTR1_DEV01_2026-10.zip.",
  );
  expect(hub.exported).toEqual([{ month: "2026-10", folder: "/media/usb" }]);
});

test("a folder the Hub refuses is explained and nothing is saved", async ({ page }) => {
  await open(page, ["report.view"], "/reports/gst");
  await page.getByTestId("gst-folder").fill("/bad");
  await page.getByRole("button", { name: en["hub.gst.export"] }).click();
  await expect(page.getByTestId("gst-folder-error")).toContainText(
    en["hub.settings.system.invalid.not_writable"],
  );
  await expect(page.getByTestId("gst-saved")).toHaveCount(0);
  await page.getByTestId("gst-folder").fill("");
  await page.getByRole("button", { name: en["hub.gst.export"] }).click();
  await expect(page.getByTestId("gst-folder-error")).toContainText(en["hub.gst.folder_required"]);
});

test("without report.view the GST screen says so and asks for nothing", async ({ page }) => {
  const hub = await open(page, ["pos.sell"], "/reports/gst");
  await expect(page.getByTestId("reports-denied")).toHaveText(en["hub.reports.no_access"]);
  expect(hub.asked).toEqual([]);
});

test("the GST screen has no accessibility violations", async ({ page }) => {
  await open(page, ["report.view"], "/reports/gst");
  await expect(page.getByTestId("gst-section-hsn")).toBeVisible();
  const gst = await new AxeBuilder({ page }).analyze();
  expect(gst.violations).toEqual([]);
});

// ---- Story 5.30: Players and Staff ----

test("the list offers Players and Staff after Shifts", async ({ page }) => {
  await open(page, ["report.view"]);
  await expect(page.getByTestId("report-link-staff")).toBeVisible();
  const ids = await page
    .locator("[data-testid^='report-link-']")
    .evaluateAll((links) => links.map((l) => l.getAttribute("data-testid")));
  expect(ids.slice(0, 5)).toEqual([
    "report-link-daily-sales",
    "report-link-sessions",
    "report-link-shifts",
    "report-link-players",
    "report-link-staff",
  ]);
  await page.getByTestId("report-link-players").click();
  await expect(page.getByTestId("players-report-screen")).toBeVisible();
});

test("Players shows top spenders, new and returning, the guest total and credit by age", async ({
  page,
}) => {
  const hub = await open(page, ["report.view"], "/reports/players");
  await expect(page.getByTestId("players-top")).toBeVisible();
  expect(hub.asked[0]).toMatch(/^players (\d{4}-\d{2}-\d{2}) \1$/);
  const rows = page.getByTestId("top-row");
  await expect(rows).toHaveCount(3);
  await expect(rows.nth(0)).toContainText("Asha Rao");
  await expect(rows.nth(0)).toContainText("₹900.00");
  await expect(rows.nth(0)).toContainText("3");
  await expect(page.getByTestId("new-count")).toHaveText("2");
  await expect(page.getByTestId("new-spend")).toHaveText("₹500.00");
  await expect(page.getByTestId("returning-count")).toHaveText("1");
  await expect(page.getByTestId("returning-spend")).toHaveText("₹900.00");
  await expect(page.getByTestId("guest-row")).toContainText(en["hub.reports.guest"]);
  await expect(page.getByTestId("guest-bills")).toHaveText("4");
  await expect(page.getByTestId("guest-spend")).toHaveText("₹350.00");
  await expect(page.getByTestId("credit-bucket-0_7")).toHaveText("₹80.00");
  await expect(page.getByTestId("credit-bucket-8_14")).toHaveText("₹300.00");
  await expect(page.getByTestId("credit-bucket-15_plus")).toHaveText("₹200.00");
  await expect(page.getByTestId("credit-total")).toHaveText("₹580.00");
  await expect(page.getByTestId("debtor-row")).toHaveCount(2);
  await expect(page.getByTestId("debtor-row").nth(1)).toContainText("20");
});

test("an erased player reads as Erased player with no personal data", async ({ page }) => {
  await open(page, ["report.view"], "/reports/players");
  const erased = page.getByTestId("top-row").nth(1);
  await expect(erased).toHaveAttribute("data-erased", "true");
  await expect(erased).toContainText(en["hub.reports.erased_player"]);
  await expect(page.getByTestId("debtor-row").first()).toContainText(
    en["hub.reports.erased_player"],
  );
  await expect(page.getByTestId("players-report-screen")).not.toContainText("phone");
});

test("an empty Players range says so and still shows the credit buckets", async ({ page }) => {
  await open(page, ["report.view"], "/reports/players", GST_TAX, true);
  await expect(page.getByTestId("players-none")).toHaveText(en["hub.reports.no_players"]);
  await expect(page.getByTestId("credit-bucket-0_7")).toHaveText("₹0.00");
  await expect(page.getByTestId("guest-bills")).toHaveText("0");
  await expect(page.getByTestId("debtors-none")).toBeVisible();
});

test("Staff shows each member's figures and the variance per shift", async ({ page }) => {
  const hub = await open(page, ["report.view"], "/reports/staff");
  await expect(page.getByTestId("staff-table")).toBeVisible();
  expect(hub.asked[0]).toMatch(/^staff (\d{4}-\d{2}-\d{2}) \1$/);
  const meera = page.getByTestId("staff-row-S1");
  await expect(meera).toContainText("Meera");
  await expect(meera.getByTestId("staff-sales")).toHaveText("6 · ₹1,200.00");
  await expect(meera.getByTestId("staff-sessions")).toHaveText("4");
  await expect(meera.getByTestId("staff-discounts")).toHaveText("2 · ₹150.00");
  await expect(meera.getByTestId("staff-voids")).toHaveText("1 · ₹120.00");
  await expect(meera.getByTestId("staff-refunds")).toHaveText("1 · ₹90.00");
  await expect(meera.getByTestId("staff-variance")).toHaveText("−₹5.00");
  // A member no longer in the staff table has an empty name.
  await expect(page.getByTestId("staff-row-GONE")).toContainText(en["hub.reports.former_staff"]);
  await expect(page.getByTestId("shift-variance-row")).toHaveCount(1);
  await expect(page.getByTestId("shift-variance")).toHaveText("−₹5.00");
  await expect(page.getByTestId("shift-open-SH1")).toHaveAttribute("href", "/reports/shifts/SH1");
});

test("an empty Staff range says so", async ({ page }) => {
  await open(page, ["report.view"], "/reports/staff", GST_TAX, true);
  await expect(page.getByTestId("staff-none")).toHaveText(en["hub.reports.no_staff"]);
});

test("a preset on Players and Staff asks for the new range", async ({ page }) => {
  const hub = await open(page, ["report.view"], "/reports/staff");
  await expect(page.getByTestId("staff-table")).toBeVisible();
  await page.getByRole("button", { name: en["hub.reports.last_7"] }).click();
  await expect.poll(() => hub.asked.length).toBe(2);
  const [kind, from, to] = hub.asked[1].split(" ");
  expect(kind).toBe("staff");
  expect(daySpan(from, to)).toBe(7);
});

test("without report.view Players and Staff say so and ask for nothing", async ({ page }) => {
  const hub = await open(page, ["pos.sell"], "/reports/players");
  await expect(page.getByTestId("reports-denied")).toHaveText(en["hub.reports.no_access"]);
  await page.goto("/reports/staff");
  await expect(page.getByTestId("reports-denied")).toBeVisible();
  expect(hub.asked).toEqual([]);
});

test("the Players and Staff screens have no accessibility violations", async ({ page }) => {
  await open(page, ["report.view"], "/reports/players");
  await expect(page.getByTestId("players-top")).toBeVisible();
  const players = await new AxeBuilder({ page }).analyze();
  expect(players.violations).toEqual([]);
  await page.goto("/reports/staff");
  await expect(page.getByTestId("staff-table")).toBeVisible();
  const staff = await new AxeBuilder({ page }).analyze();
  expect(staff.violations).toEqual([]);
});
