import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type {
  PaymentRow,
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

type Sent = { payload: { kind: string; order_id?: string; payments?: PaymentRow[] }; rev?: number };
type Hub = {
  devices: SnapshotDevice[];
  bills: SnapshotBill[];
  sent: Sent[];
  shift?: ShiftState;
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
      case "/ui/intent": {
        const body = req.postDataJSON() as {
          intent: { payload: Sent["payload"]; base_rev?: number };
        };
        hub.sent.push({ payload: body.intent.payload, rev: body.intent.base_rev });
        hub.bills = [];
        return json(route, {
          applied: { intent_id: "I", changes: [] },
          settled: { order_id: "O1", settled_at: 1, undo_until: 8001 },
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
      },
      rev: 4,
    },
  ]);
  await expect(bar(page)).toHaveCount(0);
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
