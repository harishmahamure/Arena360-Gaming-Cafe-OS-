import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type {
  PaymentRow,
  PrinterReady,
  SessionStaff,
  ShiftState,
  SnapshotBill,
  SnapshotDevice,
} from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { amountText, parseAmount, rowErrors, tally, toPayments, type Row } from "../src/lib/settle";
import { CORS, HUB, device, json, snapshot } from "./floor-fixture";

const SELLER: SessionStaff = {
  staff_id: "01JSELLER000000000000000000",
  display_name: "Counter",
  role_name: "Counter",
  permissions: ["pos.sell", "session.start", "device.screen"],
};

type Sent = {
  payload: {
    kind: string;
    order_id?: string;
    payments?: PaymentRow[];
    receipt?: string;
    business?: { gstin: string; legal_name: string };
    coupon?: string;
  };
  rev?: number;
};
type Hub = {
  devices: SnapshotDevice[];
  bills: SnapshotBill[];
  sent: Sent[];
  shift?: ShiftState;
  /** What `/ui/printer/ready` answers (Story 5.17); absent means the Hub does not answer. */
  printer?: PrinterReady;
  /** The `player_id` the settle bar last asked `/ui/printer/ready` about (Story 5.19). */
  readyFor?: string | null;
  /** What `/ui/coupons/preview` answers per upper-case code: a discount, or the refusal text. */
  coupons?: Record<string, number | string>;
  /** The bodies the bar sent to `/ui/coupons/preview` (Story 5.21). */
  previews?: { code: string; order_id?: string }[];
};

const FREE = device(1, { status: "online" });
const BILL: SnapshotBill = {
  session_id: "S1",
  device_id: FREE.device_id,
  order_id: "O1",
  order_rev: 4,
  lines: [],
  subtotal_minor: 7500,
  paid_minor: 0,
  remaining_minor: 7500,
};

async function open(page: Page, over: Partial<Hub> = {}): Promise<Hub> {
  const hub: Hub = { devices: [FREE], bills: [BILL], sent: [], ...over };
  await page.addInitScript(() => sessionStorage.setItem("arena.hub.token", "tok"));
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot": {
        const snap = snapshot(hub.devices);
        snap.sessions = [];
        snap.bills = hub.bills;
        if (hub.shift) snap.shift = hub.shift;
        return json(route, snap);
      }
      case "/ui/me":
        return json(route, SELLER);
      case "/ui/settings":
        return json(route, []);
      case "/ui/coupons/preview": {
        const body = req.postDataJSON() as { code: string; order_id?: string };
        (hub.previews ??= []).push(body);
        const answer = hub.coupons?.[body.code.toUpperCase()];
        if (typeof answer === "number") {
          return json(route, {
            code: body.code.toUpperCase(),
            discount_minor: answer,
            total_minor: BILL.remaining_minor - answer,
          });
        }
        return route.fulfill({
          status: 422,
          contentType: "application/json",
          headers: CORS,
          body: JSON.stringify({
            code: "COUPON_INVALID",
            message: answer ?? "this coupon code does not exist",
            field: "coupon",
          }),
        });
      }
      case "/ui/printer/ready":
        hub.readyFor = new URL(req.url()).searchParams.get("player_id");
        return hub.printer
          ? json(route, hub.printer)
          : route.fulfill({ status: 500, headers: CORS, body: "" });
      case "/ui/intent": {
        const body = req.postDataJSON() as {
          intent: { payload: Sent["payload"]; base_rev?: number };
        };
        hub.sent.push({ payload: body.intent.payload, rev: body.intent.base_rev });
        hub.bills = [];
        return json(route, {
          applied: { intent_id: "I", changes: [] },
          settled: {
            order_id: "O1",
            settled_at: 1,
            undo_until: 8001,
            irreversible: body.intent.payload.receipt === "print",
          },
        });
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/floor");
  await page.locator("[data-component=floor-tile] [data-tile-face]").first().focus();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("settle-bar")).toBeVisible();
  return hub;
}

const bar = (page: Page) => page.getByTestId("settle-bar");
const method = (page: Page, name: "cash" | "upi" | "card") =>
  bar(page).locator(`[data-method=${name}]`);
const settleButton = (page: Page) =>
  bar(page).getByRole("button", { name: en["hub.settle.submit"], exact: true });
const field = (page: Page, label: keyof typeof en) =>
  bar(page).getByLabel(en[label], { exact: true });

test("amounts parse to minor units and refuse anything else", () => {
  expect(parseAmount("75", 2)).toBe(7500);
  expect(parseAmount("75.5", 2)).toBe(7550);
  expect(parseAmount(" 0.05 ", 2)).toBe(5);
  expect(parseAmount("75.", 2)).toBe(7500);
  expect(parseAmount("0", 2)).toBeNull();
  expect(parseAmount("1.234", 2)).toBeNull();
  expect(parseAmount("-5", 2)).toBeNull();
  expect(parseAmount("1e3", 2)).toBeNull();
  expect(parseAmount("12.5", 0)).toBeNull();
  expect(parseAmount("12", 0)).toBe(12);
  expect(amountText(7500, 2)).toBe("75.00");
  expect(amountText(5, 2)).toBe("0.05");
  expect(amountText(12, 0)).toBe("12");
  expect(amountText(0, 2)).toBe("");
});

test("the remainder, change and overpayment follow the rows", () => {
  const row = (method: Row["method"], amount: string): Row => ({ method, amount, reference: "" });
  expect(tally(7500, [], 2)).toEqual({ paid: 0, remaining: 7500, change: 0, overpaid: false });
  expect(tally(7500, [row("cash", "50")], 2).remaining).toBe(2500);
  expect(tally(7500, [row("cash", "100")], 2)).toMatchObject({ remaining: 0, change: 2500 });
  expect(tally(7500, [row("cash", "10"), row("upi", "70")], 2)).toMatchObject({
    change: 500,
    overpaid: false,
  });
  expect(tally(7500, [row("upi", "100")], 2)).toMatchObject({ overpaid: true, change: 0 });
  expect(tally(7500, [row("cash", "10"), row("upi", "100")], 2).overpaid).toBe(true);
});

test("change comes off the cash row and references are trimmed", () => {
  const rows: Row[] = [
    { method: "cash", amount: "100", reference: "" },
    { method: "upi", amount: "50", reference: " 1234 " },
  ];
  expect(toPayments(rows, 7500, 2)).toEqual([
    { method: "cash", amount_minor: 2500 },
    { method: "upi", amount_minor: 5000, reference: "1234" },
  ]);
  const exact: Row[] = [
    { method: "cash", amount: "25", reference: "" },
    { method: "upi", amount: "75", reference: "1234" },
  ];
  expect(toPayments(exact, 2500, 2)).toEqual([
    { method: "upi", amount_minor: 7500, reference: "1234" },
  ]);
  expect(rowErrors({ method: "upi", amount: "5", reference: "123" }, 2).reference).toBe(
    "hub.settle.err.upi",
  );
  expect(rowErrors({ method: "card", amount: "5", reference: "  " }, 2).reference).toBe(
    "hub.settle.err.card",
  );
  expect(rowErrors({ method: "cash", amount: "5", reference: "" }, 2)).toEqual({});
});

test("an unpaid bill shows the total, cash UPI and card, and a disabled Settle that says why", async ({
  page,
}) => {
  await open(page);
  await expect(page.getByTestId("settle-total")).toHaveText("₹75.00");
  await expect(page.getByTestId("settle-total")).toHaveClass(/type-hub-numeric/);
  await expect(bar(page).locator("[data-method]")).toHaveText(["Cash", "UPI", "Card"]);
  await expect(bar(page).locator("img, svg, canvas, [data-qr]")).toHaveCount(0);
  await expect(settleButton(page)).toHaveAttribute("aria-disabled", "true");
  await expect(bar(page)).toContainText("Remaining ₹75.00");
  await expect(page.getByTestId("settle-remainder")).toContainText("Remaining ₹75.00");
  const box = (await settleButton(page).boundingBox())!;
  expect(box.height).toBeGreaterThanOrEqual(44);
  expect(
    (await new AxeBuilder({ page }).analyze()).violations.filter(
      (v) => v.impact === "serious" || v.impact === "critical",
    ),
  ).toEqual([]);
});

test("cash for the full total settles and the bar shows the settled state", async ({ page }) => {
  const hub = await open(page);
  await method(page, "cash").click();
  await expect(field(page, "hub.settle.amount.cash")).toHaveValue("75.00");
  await expect(page.getByTestId("settle-remainder")).toContainText("Paid in full");
  await settleButton(page).click();
  await expect(page.getByTestId("settle-done")).toHaveText("Bill settled · ₹75.00");
  await expect(page.getByTestId("unlock-status")).toHaveText(en["hub.sheet.done.settled"]);
  expect(hub.sent).toEqual([
    {
      payload: {
        kind: "session.settle",
        order_id: "O1",
        payments: [{ method: "cash", amount_minor: 7500 }],
        receipt: "none",
      },
      rev: 4,
    },
  ]);
  await expect(bar(page)).toHaveCount(0);
});

test("a session bill settles with the Print choice, and a printed bill shows no Undo", async ({
  page,
}) => {
  const hub = await open(page, { printer: { ready: true, default_choice: "print" } });
  await expect(page.getByTestId("printer-ready")).toHaveText(en["hub.settle.printer_ready"]);
  await expect(bar(page).locator('[data-receipt="print"]')).toHaveAttribute("aria-pressed", "true");
  await method(page, "cash").click();
  await settleButton(page).click();
  await expect(page.getByTestId("settle-done")).toBeVisible();
  expect(hub.sent[0].payload).toMatchObject({ kind: "session.settle", receipt: "print" });
  await expect(page.getByTestId("undo-toast")).toHaveCount(0);
});

test("with no printer the bar says nothing will print and the bill still settles", async ({
  page,
}) => {
  const hub = await open(page, {
    printer: { ready: false, default_choice: "print", reason: "unset" },
  });
  await expect(page.getByTestId("printer-missing")).toContainText(en["hub.settle.no_printer"]);
  await expect(bar(page).locator("[data-receipt]")).toHaveCount(0);
  await method(page, "cash").click();
  await settleButton(page).click();
  await expect(page.getByTestId("settle-done")).toBeVisible();
  expect(hub.sent[0].payload.receipt).toBe("none");
});

test("WhatsApp is added beside Print and None for a profiled player the Hub says it offers", async ({
  page,
}) => {
  const hub = await open(page, {
    bills: [{ ...BILL, player_id: "P1" }],
    printer: { ready: true, default_choice: "print", whatsapp: true },
  });
  await expect(bar(page).locator("[data-receipt]")).toHaveCount(3);
  await expect(bar(page).locator('[data-receipt="whatsapp"]')).toHaveText(
    en["hub.settle.receipt.whatsapp"],
  );
  expect(hub.readyFor).toBe("P1");
  await bar(page).locator('[data-receipt="whatsapp"]').click();
  await expect(bar(page).locator('[data-receipt="whatsapp"]')).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await method(page, "cash").click();
  await settleButton(page).click();
  await expect(page.getByTestId("settle-done")).toBeVisible();
  expect(hub.sent[0].payload).toMatchObject({ kind: "session.settle", receipt: "whatsapp" });
});

test("a guest bill is not offered WhatsApp", async ({ page }) => {
  const guest = await open(page, { printer: { ready: true, default_choice: "print" } });
  expect(guest.readyFor).toBeNull();
  await expect(bar(page).locator('[data-receipt="whatsapp"]')).toHaveCount(0);
  await expect(bar(page).locator("[data-receipt]")).toHaveCount(2);
});

test("with no printer a player can still take the receipt on WhatsApp", async ({ page }) => {
  const hub = await open(page, {
    bills: [{ ...BILL, player_id: "P1" }],
    printer: { ready: false, default_choice: "print", reason: "unset", whatsapp: true },
  });
  await expect(page.getByTestId("printer-missing")).toContainText(en["hub.settle.no_printer"]);
  await expect(bar(page).locator("[data-receipt]")).toHaveCount(2);
  await expect(bar(page).locator('[data-receipt="none"]')).toHaveAttribute("aria-pressed", "true");
  await bar(page).locator('[data-receipt="whatsapp"]').click();
  await method(page, "cash").click();
  await settleButton(page).click();
  await expect(page.getByTestId("settle-done")).toBeVisible();
  expect(hub.sent[0].payload.receipt).toBe("whatsapp");
});

test("cash above the remainder shows Change and sends only the bill amount", async ({ page }) => {
  const hub = await open(page);
  await method(page, "cash").click();
  await page.keyboard.type("100");
  await expect(page.getByTestId("settle-change")).toHaveText("Change ₹25.00");
  await expect(page.getByTestId("settle-remainder")).toContainText("Paid in full");
  await settleButton(page).click();
  await expect(page.getByTestId("settle-done")).toBeVisible();
  expect(hub.sent[0].payload.payments).toEqual([{ method: "cash", amount_minor: 7500 }]);
});

test("a two-method split settles from the keyboard with Ctrl+Enter", async ({ page }) => {
  const hub = await open(page);
  await method(page, "cash").focus();
  await page.keyboard.press("Enter");
  await page.keyboard.type("50");
  await expect(page.getByTestId("settle-remainder")).toContainText("Remaining ₹25.00");
  await page.keyboard.press("Control+Enter");
  expect(hub.sent).toHaveLength(0);
  await method(page, "cash").focus();
  await page.keyboard.press("ArrowRight");
  await expect(method(page, "upi")).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(field(page, "hub.settle.amount.upi")).toHaveValue("25.00");
  await page.keyboard.press("Tab");
  await page.keyboard.type("1234");
  await expect(page.getByTestId("settle-remainder")).toContainText("Paid in full");
  await page.keyboard.press("Control+Enter");
  await expect(page.getByTestId("settle-done")).toBeVisible();
  expect(hub.sent).toHaveLength(1);
  expect(hub.sent[0].payload.payments).toEqual([
    { method: "cash", amount_minor: 5000 },
    { method: "upi", amount_minor: 2500, reference: "1234" },
  ]);
});

test("a UPI row without 4 digits and a card row without a reference cannot settle", async ({
  page,
}) => {
  const hub = await open(page);
  await method(page, "upi").click();
  await field(page, "hub.settle.reference.upi").fill("12");
  await settleButton(page).click();
  await expect(bar(page)).toContainText(en["hub.settle.err.upi"]);
  await expect(field(page, "hub.settle.reference.upi")).toBeFocused();
  expect(hub.sent).toHaveLength(0);
  await field(page, "hub.settle.amount.upi").fill("50");
  await method(page, "card").click();
  await settleButton(page).click();
  await expect(bar(page)).toContainText(en["hub.settle.err.card"]);
  expect(hub.sent).toHaveLength(0);
  await field(page, "hub.settle.reference.card").fill("TXN-9");
  await field(page, "hub.settle.reference.upi").fill("1234");
  await page.keyboard.press("Control+Enter");
  await expect(page.getByTestId("settle-done")).toBeVisible();
  expect(hub.sent[0].payload.payments).toEqual([
    { method: "upi", amount_minor: 5000, reference: "1234" },
    { method: "card", amount_minor: 2500, reference: "TXN-9" },
  ]);
});

test("only cash may exceed the bill", async ({ page }) => {
  const hub = await open(page);
  await method(page, "upi").click();
  await field(page, "hub.settle.amount.upi").fill("100");
  await expect(page.getByTestId("settle-remainder")).toContainText(en["hub.settle.over"]);
  await expect(settleButton(page)).toHaveAttribute("aria-disabled", "true");
  await page.keyboard.press("Control+Enter");
  expect(hub.sent).toHaveLength(0);
});

test("a row can be removed and the remainder returns", async ({ page }) => {
  await open(page);
  await method(page, "cash").click();
  await bar(page).getByRole("button", { name: "Remove Cash" }).click();
  await expect(bar(page).getByTestId("split-row")).toHaveCount(0);
  await expect(page.getByTestId("settle-remainder")).toContainText("Remaining ₹75.00");
});

test("with no open shift the bar offers no cash, says why, and UPI still settles", async ({
  page,
}) => {
  const hub = await open(page, { shift: { open: null, carried_minor: 0 } });
  await expect(bar(page).locator("[data-method]")).toHaveText(["UPI", "Card"]);
  const notice = page.getByTestId("settle-shift-closed");
  await expect(notice).toContainText(en["hub.settle.shift_closed"]);
  await expect(notice.getByRole("link", { name: en["hub.settle.shift_link"] })).toHaveAttribute(
    "href",
    "/shift",
  );
  await method(page, "upi").click();
  await field(page, "hub.settle.amount.upi").fill("75");
  await bar(page).getByLabel(en["hub.settle.reference.upi"], { exact: true }).fill("1234");
  await settleButton(page).click();
  await expect.poll(() => hub.sent.length).toBe(1);
  expect(hub.sent[0].payload.payments).toEqual([
    { method: "upi", amount_minor: 7500, reference: "1234" },
  ]);
});

test("with an open shift the bar offers cash and no shift notice", async ({ page }) => {
  await open(page, {
    shift: {
      open: {
        id: "SH1",
        staff_id: "S1",
        staff_name: "Ravi",
        opened_at: 1,
        float_minor: 0,
        events: 1,
      },
      carried_minor: 0,
    },
  });
  await expect(bar(page).locator("[data-method]")).toHaveText(["Cash", "UPI", "Card"]);
  await expect(page.getByTestId("settle-shift-closed")).toHaveCount(0);
});

// Story 5.20: Business invoice on a GST branch.
const BUSINESS_PRINTER: PrinterReady = { ready: true, default_choice: "print", business: true };

test("a simple-tax branch is not offered Business invoice", async ({ page }) => {
  await open(page, { printer: { ready: true, default_choice: "print" } });
  await expect(bar(page).locator("[data-receipt]")).toHaveCount(2);
  await expect(page.getByTestId("business-invoice")).toHaveCount(0);
});

test("Business invoice keeps Settle disabled until the GSTIN and legal name are valid", async ({
  page,
}) => {
  const hub = await open(page, { printer: BUSINESS_PRINTER });
  await method(page, "cash").click();
  await expect(settleButton(page)).toBeEnabled();

  await page.locator('[data-business="toggle"]').click();
  await expect(page.getByTestId("business-fields")).toBeVisible();
  await expect(settleButton(page)).toBeDisabled();

  const fields = page.getByTestId("business-fields");
  await fields.getByLabel(en["hub.settle.business.gstin"]).fill("29AABCT1332L1ZX");
  await fields.getByLabel(en["hub.settle.business.name"]).fill("Tata Consultancy Services");
  await expect(fields).toContainText(en["hub.settle.business.err.gstin"]);
  await expect(settleButton(page)).toBeDisabled();

  await fields.getByLabel(en["hub.settle.business.gstin"]).fill(" 29aabct1332l1za ");
  await expect(fields).not.toContainText(en["hub.settle.business.err.gstin"]);
  await expect(settleButton(page)).toBeEnabled();
  await settleButton(page).click();
  await expect(page.getByTestId("settle-done")).toBeVisible();
  expect(hub.sent[0].payload).toMatchObject({
    kind: "session.settle",
    business: { gstin: "29AABCT1332L1ZA", legal_name: "Tata Consultancy Services" },
  });
});

test("turning Business invoice off clears the block and sends no buyer", async ({ page }) => {
  const hub = await open(page, { printer: BUSINESS_PRINTER });
  await method(page, "cash").click();
  await page.locator('[data-business="toggle"]').click();
  await expect(settleButton(page)).toBeDisabled();
  await page.locator('[data-business="toggle"]').click();
  await expect(page.getByTestId("business-fields")).toHaveCount(0);
  await settleButton(page).click();
  await expect(page.getByTestId("settle-done")).toBeVisible();
  expect(hub.sent[0].payload.business).toBeUndefined();
});

// Story 5.21: coupons applied at settle.
const apply = async (page: Page, code: string) => {
  const toggle = page.getByTestId("coupon-toggle");
  if (await toggle.isVisible()) await toggle.click();
  await field(page, "hub.settle.coupon").fill(code);
  await bar(page).getByRole("button", { name: en["hub.settle.coupon.apply"] }).click();
};

test("a coupon shows its line and the new total, and the settle sends the code", async ({
  page,
}) => {
  const hub = await open(page, { coupons: { SAVE10: 750 } });
  await apply(page, " save10 ");
  await expect(page.getByTestId("coupon-line")).toContainText("Coupon SAVE10");
  await expect(page.getByTestId("coupon-amount")).toHaveText("−₹7.50");
  await expect(page.getByTestId("settle-total")).toHaveText("₹67.50");
  expect(hub.previews).toEqual([{ code: "SAVE10", order_id: "O1" }]);

  await method(page, "cash").click();
  await expect(field(page, "hub.settle.amount.cash")).toHaveValue("67.50");
  await expect(page.getByTestId("settle-remainder")).toContainText("Paid in full");
  await settleButton(page).click();
  await expect(page.getByTestId("settle-done")).toBeVisible();
  expect(hub.sent[0].payload).toMatchObject({
    kind: "session.settle",
    payments: [{ method: "cash", amount_minor: 6750 }],
    coupon: "SAVE10",
  });
});

test("a refused coupon shows its reason in place and leaves the bill unchanged", async ({
  page,
}) => {
  const hub = await open(page, { coupons: { OLD: "this coupon has expired" } });
  await apply(page, "old");
  await expect(page.getByTestId("coupon-error")).toContainText("this coupon has expired");
  await expect(page.getByTestId("settle-total")).toHaveText("₹75.00");
  await expect(page.getByTestId("coupon-line")).toHaveCount(0);
  await method(page, "cash").click();
  await settleButton(page).click();
  await expect(page.getByTestId("settle-done")).toBeVisible();
  expect(hub.sent[0].payload.coupon).toBeUndefined();
});

test("removing the coupon brings the total back and sends no code", async ({ page }) => {
  const hub = await open(page, { coupons: { SAVE10: 750 } });
  await apply(page, "SAVE10");
  await expect(page.getByTestId("settle-total")).toHaveText("₹67.50");
  await page.getByTestId("coupon-remove").click();
  await expect(page.getByTestId("settle-total")).toHaveText("₹75.00");
  await expect(page.getByTestId("coupon-line")).toHaveCount(0);
  await method(page, "cash").click();
  await settleButton(page).click();
  await expect(page.getByTestId("settle-done")).toBeVisible();
  expect(hub.sent[0].payload.coupon).toBeUndefined();
});

test("a coupon that takes the whole bill settles with no payment", async ({ page }) => {
  const hub = await open(page, { coupons: { FREE100: 7500 } });
  await apply(page, "free100");
  await expect(page.getByTestId("settle-total")).toHaveText("₹0.00");
  await expect(page.getByTestId("settle-remainder")).toContainText("Paid in full");
  await expect(settleButton(page)).toBeEnabled();
  await settleButton(page).click();
  await expect(page.getByTestId("settle-done")).toBeVisible();
  expect(hub.sent[0].payload).toMatchObject({
    kind: "session.settle",
    payments: [],
    coupon: "FREE100",
  });
});

test("the coupon sits in the total row and opens its field on demand", async ({ page }) => {
  await open(page);
  await expect(page.getByTestId("coupon-toggle")).toHaveText(en["hub.settle.coupon.add"]);
  await expect(bar(page).getByLabel(en["hub.settle.coupon"], { exact: true })).toHaveCount(0);
  await expect(bar(page).locator("p.total").getByTestId("coupon-toggle")).toBeVisible();
  await page.getByTestId("coupon-toggle").click();
  await expect(field(page, "hub.settle.coupon")).toBeVisible();
  await expect(page.getByTestId("coupon-toggle")).toHaveCount(0);
});
