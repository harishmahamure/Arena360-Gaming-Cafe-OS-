import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type { ShiftCount, SnapshotShift } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { parseCount } from "../src/lib/shift";
import { CORS, HUB, device, json as ok, snapshot } from "./floor-fixture";

// Closing a shift (Story 5.26) on the Shift screen. The Hub's loopback API is stubbed with
// `page.route`; the stub works out the reveal from a fixed expected cash, so the page can only
// ever see the expected cash for a count it sent, and records every intent and elevation ticket.

const PIN = "2222";
const EXPECTED = 499_000;
const THRESHOLD = 10_000;
const OPEN: SnapshotShift = {
  id: "SH1",
  staff_id: "01JMANAGER0000000000000000",
  staff_name: "Ravi",
  opened_at: Date.UTC(2026, 9, 28, 9, 30),
  float_minor: 500_000,
  events: 4,
};

type Sent = { payload: Record<string, unknown>; ticket?: string };
type Hub = {
  sent: Sent[];
  counts: string[];
  elevations: { items: unknown[]; pin: string }[];
  fail: { status: number; code: string; field?: string } | null;
  closed: boolean;
};

function reveal(counted: number): ShiftCount {
  const difference = counted - EXPECTED;
  return {
    counted_minor: counted,
    expected_minor: EXPECTED,
    difference_minor: difference,
    threshold_minor: THRESHOLD,
    over_threshold: Math.abs(difference) > THRESHOLD,
    breakdown: {
      float_minor: 500_000,
      cash_sales_minor: 20_000,
      cash_refunds_minor: -5_000,
      deposits_minor: -10_000,
      expenses_minor: -6_000,
    },
  };
}

async function open(page: Page, permissions: string[]): Promise<Hub> {
  const hub: Hub = { sent: [], counts: [], elevations: [], fail: null, closed: false };
  await page.addInitScript(() => sessionStorage.setItem("arena.hub.token", "tok"));
  await page.route(`${HUB}/**`, async (route: Route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    const url = new URL(req.url());
    switch (url.pathname) {
      case "/ui/snapshot": {
        const snap = snapshot([device(1, {})]);
        snap.shift = hub.closed
          ? { open: null, carried_minor: 495_000 }
          : { open: OPEN, carried_minor: 500_000 };
        return ok(route, snap);
      }
      case "/ui/me":
        return ok(route, {
          staff_id: "01JCOUNTER0000000000000000",
          display_name: "Asha",
          role_name: "Counter",
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
      case "/ui/shift/count": {
        const counted = url.searchParams.get("counted_minor") ?? "";
        hub.counts.push(counted);
        return ok(route, reveal(Number(counted)));
      }
      case "/ui/elevate": {
        const body = req.postDataJSON() as { items: unknown[]; pin: string };
        hub.elevations.push(body);
        if (body.pin !== PIN) {
          return route.fulfill({
            status: 403,
            contentType: "application/json",
            headers: CORS,
            body: JSON.stringify({ code: "PIN_REJECTED", message: "no" }),
          });
        }
        return ok(route, { ticket: "T1", items: body.items, expires_in_s: 60 });
      }
      case "/ui/intent": {
        const body = req.postDataJSON() as {
          intent: { payload: Sent["payload"] };
          elevation_ticket?: string;
        };
        hub.sent.push({ payload: body.intent.payload, ticket: body.elevation_ticket });
        if (hub.fail) {
          return route.fulfill({
            status: hub.fail.status,
            contentType: "application/json",
            headers: CORS,
            body: JSON.stringify({ message: "no", ...hub.fail }),
          });
        }
        hub.closed = true;
        return ok(route, { applied: { intent_id: "I", changes: [] } });
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/shift");
  await expect(page.getByTestId("shift-open")).toBeVisible();
  return hub;
}

async function startClose(page: Page) {
  await page.getByRole("button", { name: en["hub.shift.close.start"] }).click();
  await expect(page.getByTestId("close-shift")).toBeVisible();
}

async function singleTotal(page: Page, amount: string) {
  await page.getByLabel(en["hub.shift.close.single_total"]).check();
  await page.getByLabel(en["hub.shift.close.counted_cash"]).fill(amount);
  await page.getByRole("button", { name: en["hub.shift.close.confirm_count"] }).click();
}

test("a count of notes is whole numbers and empty is zero", () => {
  expect(parseCount("")).toBe(0);
  expect(parseCount(" 12 ")).toBe(12);
  expect(parseCount("1.5")).toBeNull();
  expect(parseCount("-1")).toBeNull();
  expect(parseCount("x")).toBeNull();
});

test("the expected cash stays hidden until a count is confirmed", async ({ page }) => {
  const hub = await open(page, ["shift.close"]);
  await startClose(page);
  await expect(page.getByTestId("variance-blind")).toHaveText(en["hub.shift.close.blind"]);
  await expect(page.getByTestId("variance-expected")).toHaveCount(0);
  expect(hub.counts).toEqual([]);
  await singleTotal(page, "4850");
  await expect(page.getByTestId("variance-counted")).toHaveText("₹4,850.00");
  await expect(page.getByTestId("variance-expected")).toHaveText("₹4,990.00");
  await expect(page.getByTestId("variance-difference")).toHaveText("−₹140.00");
  await expect(page.getByTestId("variance-line")).toHaveText(
    "Counted ₹4,850.00. Expected ₹4,990.00. Difference −₹140.00 short.",
  );
  expect(hub.counts).toEqual(["485000"]);
  // Changing the count hides the reveal again.
  await page.getByLabel(en["hub.shift.close.counted_cash"]).fill("4900");
  await expect(page.getByTestId("variance-expected")).toHaveCount(0);
  await expect(page.getByTestId("variance-blind")).toBeVisible();
});

test("the breakdown lists float, cash sales, refunds, deposits and expenses", async ({ page }) => {
  await open(page, ["shift.close"]);
  await startClose(page);
  await singleTotal(page, "4990");
  await page.getByText(en["hub.shift.close.breakdown"], { exact: true }).click();
  await expect(page.getByTestId("breakdown-float")).toHaveText("₹5,000.00");
  await expect(page.getByTestId("breakdown-sales")).toHaveText("₹200.00");
  await expect(page.getByTestId("breakdown-refunds")).toHaveText("−₹50.00");
  await expect(page.getByTestId("breakdown-deposits")).toHaveText("−₹100.00");
  await expect(page.getByTestId("breakdown-expenses")).toHaveText("−₹60.00");
  await expect(page.getByTestId("variance-line")).toHaveText(
    "Counted ₹4,990.00. Expected ₹4,990.00. No difference.",
  );
});

test("the notes add up to a running total", async ({ page }) => {
  const hub = await open(page, ["shift.close"]);
  await startClose(page);
  await page.getByTestId("count-50000").fill("9");
  await page.getByTestId("count-10000").fill("4");
  await page.getByTestId("count-2000").fill("3");
  await page.getByLabel(en["hub.shift.close.coins"]).fill("12.50");
  await expect(page.getByTestId("count-total")).toContainText("₹4,972.50");
  await page.getByRole("button", { name: en["hub.shift.close.confirm_count"] }).click();
  await expect(page.getByTestId("variance-line")).toBeVisible();
  expect(hub.counts).toEqual(["497250"]);
});

test("a close within the threshold needs no PIN and takes under 10 seconds from the first tap", async ({
  page,
}) => {
  const hub = await open(page, ["shift.close"]);
  const started = Date.now();
  await startClose(page);
  await singleTotal(page, "4950");
  await expect(page.getByTestId("variance-over")).toHaveCount(0);
  await page.getByRole("button", { name: en["hub.shift.close.submit"] }).click();
  await expect(page.getByTestId("shift-closed")).toHaveText(en["hub.shift.close.closed"]);
  expect(Date.now() - started).toBeLessThan(10_000);
  expect(hub.sent).toEqual([{ payload: { kind: "shift.close", counted_minor: 495_000 } }]);
  expect(hub.elevations).toEqual([]);
});

test("a close over the threshold shows a warning, needs a reason and a manager's PIN", async ({
  page,
}) => {
  const hub = await open(page, ["shift.close"]);
  await startClose(page);
  await singleTotal(page, "4000");
  await expect(page.getByTestId("variance-over")).toContainText(
    en["hub.shift.close.over_threshold"],
  );
  // No reason yet: nothing is sent.
  await page.getByRole("button", { name: en["hub.shift.close.submit"] }).click();
  await expect(page.getByText(en["hub.shift.close.err.reason"])).toBeVisible();
  expect(hub.sent).toHaveLength(0);
  await page.getByLabel(en["hub.shift.close.reason"]).selectOption("other");
  await page.getByRole("button", { name: en["hub.shift.close.submit"] }).click();
  await expect(page.getByText(en["hub.shift.close.err.note"])).toBeVisible();
  await page.getByLabel(en["hub.shift.close.note"]).fill("tray knocked over");
  await page.getByRole("button", { name: en["hub.shift.close.submit"] }).click();
  await expect(page.getByRole("group", { name: en["hub.shift.close.pin"] })).toBeVisible();
  expect(hub.sent).toHaveLength(0);
  await page.keyboard.type("9999");
  await page.keyboard.press("Enter");
  await expect(page.getByText(en["ui.pin.rejected"])).toBeVisible();
  await page.keyboard.type(PIN);
  await page.keyboard.press("Enter");
  await expect.poll(() => hub.sent.length).toBe(1);
  expect(hub.elevations.at(-1)?.items).toEqual([{ intent_kind: "shift.close" }]);
  expect(hub.sent[0]).toEqual({
    payload: {
      kind: "shift.close",
      counted_minor: 400_000,
      reason: "other",
      note: "tray knocked over",
    },
    ticket: "T1",
  });
});

test("a manager who holds the override closes over the threshold with a reason and no PIN", async ({
  page,
}) => {
  const hub = await open(page, ["shift.close", "shift.override_variance"]);
  await startClose(page);
  await singleTotal(page, "5200");
  await page.getByLabel(en["hub.shift.close.reason"]).selectOption("counting_error");
  await page.getByRole("button", { name: en["hub.shift.close.submit"] }).click();
  await expect(page.getByTestId("shift-closed")).toBeVisible();
  expect(hub.sent).toEqual([
    {
      payload: { kind: "shift.close", counted_minor: 520_000, reason: "counting_error" },
    },
  ]);
  expect(hub.elevations).toEqual([]);
});

test("a deposit at close carries its slip reference and cannot exceed the count", async ({
  page,
}) => {
  const hub = await open(page, ["shift.close"]);
  await startClose(page);
  await singleTotal(page, "4990");
  await page.getByLabel(en["hub.shift.close.deposit"]).check();
  await page.getByLabel(en["hub.shift.cash.amount"]).fill("5000");
  await page.getByLabel(en["hub.shift.cash.reference"]).fill("SLIP-3");
  await page.getByRole("button", { name: en["hub.shift.close.submit"] }).click();
  await expect(page.getByText(en["hub.shift.close.err.deposit"])).toBeVisible();
  expect(hub.sent).toHaveLength(0);
  await page.getByLabel(en["hub.shift.cash.amount"]).fill("4000");
  await page.getByRole("button", { name: en["hub.shift.close.submit"] }).click();
  await expect(page.getByTestId("shift-closed")).toBeVisible();
  expect(hub.sent).toEqual([
    {
      payload: {
        kind: "shift.close",
        counted_minor: 499_000,
        deposit: { amount_minor: 400_000, reference: "SLIP-3" },
      },
    },
  ]);
});

test("a refused close names the over-limit rule and stays accessible", async ({ page }) => {
  const hub = await open(page, ["shift.close"]);
  await startClose(page);
  await singleTotal(page, "4950");
  hub.fail = { status: 409, code: "VARIANCE_OVER_THRESHOLD" };
  await page.getByRole("button", { name: en["hub.shift.close.submit"] }).click();
  await expect(page.getByTestId("close-error")).toContainText(en["hub.shift.close.err.over"]);
  const results = await new AxeBuilder({ page }).analyze();
  const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  expect(bad.map((v) => v.id)).toEqual([]);
});

test("a member without shift.close sees no Close shift button", async ({ page }) => {
  await open(page, []);
  await expect(page.getByRole("button", { name: en["hub.shift.close.start"] })).toHaveCount(0);
});
