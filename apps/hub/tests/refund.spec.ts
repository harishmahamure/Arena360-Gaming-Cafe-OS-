import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type { RefundInvoice } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { CORS, HUB, device, json as ok, snapshot } from "./floor-fixture";

// Refund (Story 5.24): look an invoice up on POS, pick lines, reason and method, and send
// `refund.create`; a member without `refund.issue` asks a manager's PIN in place.
// The Hub's loopback API is stubbed with `page.route`.

const NO = "DEV01/26-27/E1k/000007";
const PIN = "2222";

type Sent = {
  payload: Record<string, unknown> & { kind: string };
  reason?: string;
  ticket?: string;
};
type Hub = {
  sent: Sent[];
  elevations: { items: unknown[]; pin: string }[];
  invoice: RefundInvoice;
};

const invoice = (extra: Partial<RefundInvoice> = {}): RefundInvoice => ({
  order_id: "O7",
  invoice_no: NO,
  settled_at: Date.UTC(2026, 9, 28, 9, 30),
  total_minor: 21000,
  paid_minor: 21000,
  refunded_minor: 0,
  has_player: false,
  lines: [
    {
      line_id: "L1",
      kind: "product",
      label: "Cola",
      qty: 3,
      refunded_qty: 1,
      amount_minor: 12000,
      refunded_minor: 4000,
      tracked: true,
    },
    {
      line_id: "L2",
      kind: "plan",
      label: "5 h pack",
      qty: 1,
      refunded_qty: 0,
      amount_minor: 9000,
      refunded_minor: 0,
      tracked: false,
      refusal: "plan_used",
    },
  ],
  refunds: [],
  ...extra,
});

async function open(page: Page, permissions: string[]): Promise<Hub> {
  const hub: Hub = { sent: [], elevations: [], invoice: invoice() };
  await page.addInitScript(() => sessionStorage.setItem("arena.hub.token", "staff-token"));
  await page.route(`${HUB}/**`, async (route: Route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    const url = new URL(req.url());
    switch (url.pathname) {
      case "/ui/snapshot":
        return ok(route, snapshot([device(1, {})]));
      case "/ui/me":
        return ok(route, {
          staff_id: "01JSTAFF0000000000000000000",
          display_name: "Staff",
          role_name: "Counter",
          permissions,
          locked: false,
          has_pin: true,
        });
      case "/ui/products":
        return ok(route, []);
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: `retry: 300\n\n`,
        });
      case "/ui/refund/invoice":
        if (url.searchParams.get("no") !== NO) {
          return route.fulfill({
            status: 404,
            contentType: "application/json",
            headers: CORS,
            body: JSON.stringify({ code: "INVOICE_NOT_FOUND", message: "no" }),
          });
        }
        return ok(route, hub.invoice);
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
          intent: { payload: Sent["payload"]; reason?: string };
          elevation_ticket?: string;
        };
        hub.sent.push({
          payload: body.intent.payload,
          reason: body.intent.reason,
          ticket: body.elevation_ticket,
        });
        hub.invoice = invoice({
          refunded_minor: 8000,
          refunds: [
            {
              refund_id: "R1",
              credit_note_no: "DEV01/26-27/E1k/000008",
              amount_minor: 8000,
              created_at: Date.UTC(2026, 9, 28, 10, 0),
              reason: "mistake",
            },
          ],
        });
        return ok(route, { applied: { intent_id: "I", changes: [] } });
      }
      default:
        return route.fulfill({ status: 404, headers: CORS, body: "{}" });
    }
  });
  await page.goto("/pos");
  await expect(page.getByTestId("pos")).toBeVisible();
  await page.getByTestId("refund-toggle").click();
  await expect(page.getByTestId("refund")).toBeVisible();
  return hub;
}

const find = async (page: Page, no: string) => {
  await page.getByLabel(en["hub.refund.invoice_label"]).fill(no);
  await page.getByRole("button", { name: en["hub.refund.find"] }).click();
};

const pickCola = async (page: Page) => {
  await find(page, NO);
  const line = page.getByTestId("refund-line").filter({ hasText: "Cola" });
  await line.getByRole("button", { name: "Refund one more Cola" }).click();
  await line.getByRole("button", { name: "Refund one more Cola" }).click();
  await expect(line.getByTestId("refund-qty")).toHaveText("2");
  await line.getByLabel(en["hub.refund.line.return_stock"]).check();
  await page.getByRole("button", { name: en["hub.refund.submit"] }).click();
  await page.getByRole("radio", { name: en["hub.refund.reason.mistake"] }).check();
  await page.getByRole("button", { name: en["ui.reason.confirm"] }).click();
};

test("an unknown invoice number is refused inline", async ({ page }) => {
  await open(page, ["pos.sell", "refund.issue"]);
  await find(page, "NOPE/1");
  await expect(page.getByTestId("refund-lookup-error")).toHaveText(en["hub.refund.err.not_found"]);
  await expect(page.getByTestId("refund-invoice")).toHaveCount(0);
});

test("the invoice opens with its lines, a used plan is disabled and says why", async ({ page }) => {
  await open(page, ["pos.sell", "refund.issue"]);
  await find(page, NO);
  await expect(page.getByTestId("refund-invoice-no")).toContainText(NO);
  await expect(page.getByTestId("refund-line")).toHaveCount(2);
  const plan = page.getByTestId("refund-line").filter({ hasText: "5 h pack" });
  await expect(plan.getByTestId("refund-line-refusal")).toHaveText(en["hub.refund.line.plan_used"]);
  await expect(plan.getByRole("checkbox")).toHaveCount(0);
  // Nothing chosen: the form refuses before any intent is sent.
  await page.getByRole("button", { name: en["hub.refund.submit"] }).click();
  await expect(page.getByTestId("refund-error")).toHaveText(en["hub.refund.err.pick"]);
  expect(await new AxeBuilder({ page }).analyze().then((r) => r.violations.length)).toBe(0);
});

test("a member with refund.issue refunds lines by cash and sees the credit note", async ({
  page,
}) => {
  const hub = await open(page, ["pos.sell", "refund.issue"]);
  await pickCola(page);
  await expect.poll(() => hub.sent.length).toBe(1);
  expect(hub.sent[0].payload).toEqual({
    kind: "refund.create",
    invoice_no: NO,
    lines: [{ line_id: "L1", qty: 2, return_to_stock: true }],
    method: "cash",
  });
  expect(hub.sent[0].reason).toBe("mistake");
  expect(hub.sent[0].ticket).toBeUndefined();
  expect(hub.elevations).toHaveLength(0);
  await expect(page.getByTestId("refund-done")).toContainText("DEV01/26-27/E1k/000008");
  await expect(page.getByTestId("refund-notes")).toContainText("DEV01/26-27/E1k/000008");
});

test("without refund.issue the manager PIN opens in place and its ticket rides the refund", async ({
  page,
}) => {
  const hub = await open(page, ["pos.sell"]);
  await pickCola(page);
  await expect(page.getByRole("group", { name: en["hub.refund.pin"] })).toBeVisible();
  expect(hub.sent).toHaveLength(0);
  await page.keyboard.type("9999");
  await page.keyboard.press("Enter");
  await expect(page.getByText(en["ui.pin.rejected"])).toBeVisible();
  expect(hub.sent).toHaveLength(0);
  await page.keyboard.type(PIN);
  await page.keyboard.press("Enter");
  await expect.poll(() => hub.sent.length).toBe(1);
  expect(hub.elevations.at(-1)?.items).toEqual([{ intent_kind: "refund.create" }]);
  expect(hub.sent[0].ticket).toBe("T1");
  expect(hub.sent[0].reason).toBe("mistake");
});
