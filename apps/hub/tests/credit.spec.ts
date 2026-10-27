import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type {
  CreditView,
  PaymentRow,
  PlayerHit,
  SessionStaff,
  SnapshotBill,
  SnapshotDevice,
  WalletView,
} from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { creditOffered, headroomOf, methodsFor, toPayments, type Row } from "../src/lib/settle";
import { CORS, HUB, device, json, snapshot } from "./floor-fixture";

const PIN = "2222";

const COUNTER: SessionStaff = {
  staff_id: "01JCOUNTER0000000000000000",
  display_name: "Counter",
  role_name: "Counter",
  permissions: ["pos.sell", "player.view", "session.start"],
};

const MANAGER: SessionStaff = {
  ...COUNTER,
  staff_id: "01JMANAGER0000000000000000",
  display_name: "Manager",
  role_name: "Manager",
  permissions: [...COUNTER.permissions, "credit.manage"],
};

const ASHA: PlayerHit = {
  player_id: "P1",
  name: "Asha Rao",
  username: "asha_9",
  phone: "+919876543210",
  tags: [],
  outstanding_minor: 0,
};

const CREDIT: CreditView = {
  limit_minor: 50_000,
  outstanding_minor: 10_000,
  reminder_days: [3, 7, 14],
  paused: false,
};

type Sent = {
  payload: {
    kind: string;
    player_id?: string;
    limit_minor?: number;
    reminder_days?: number[];
    payments?: PaymentRow[];
    credit_override?: boolean;
  };
  ticket?: string;
};

type Hub = {
  sent: Sent[];
  staff: SessionStaff;
  wallet: WalletView;
  devices: SnapshotDevice[];
  bills: SnapshotBill[];
  refuse?: { code: string; status: number };
};

const FREE = device(1, { status: "online" });
const bill = (extra: Partial<SnapshotBill>): SnapshotBill => ({
  session_id: "S1",
  device_id: FREE.device_id,
  order_id: "O1",
  order_rev: 4,
  wallet_minor: 0,
  lines: [],
  subtotal_minor: 7500,
  paid_minor: 0,
  remaining_minor: 7500,
  ...extra,
});

const newHub = (extra: Partial<Hub> = {}): Hub => ({
  sent: [],
  staff: MANAGER,
  wallet: { player_id: "P1", balance_minor: 0, txns: [] },
  devices: [FREE],
  bills: [],
  ...extra,
});

async function serve(page: Page, hub: Hub) {
  await page.addInitScript(() => sessionStorage.setItem("arena.hub.token", "tok"));
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot": {
        const snap = snapshot(hub.devices);
        snap.sessions = [];
        snap.bills = hub.bills;
        return json(route, snap);
      }
      case "/ui/me":
        return json(route, hub.staff);
      case "/ui/settings":
        return json(route, []);
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: `retry: 300\n\n`,
        });
      case "/ui/players":
        return json(route, [ASHA]);
      case "/ui/players/profile":
        return json(route, {
          player: ASHA,
          consent: { granted: true, policy_version: "1" },
          rev: 3,
          wallet_minor: hub.wallet.balance_minor,
          email: "",
          dob: "",
          notes: "",
        });
      case "/ui/players/wallet":
        return json(route, hub.wallet);
      case "/ui/elevate": {
        const body = req.postDataJSON() as { items: unknown[]; pin: string };
        if (body.pin !== PIN) {
          return route.fulfill({
            status: 403,
            contentType: "application/json",
            headers: CORS,
            body: JSON.stringify({ code: "PIN_REJECTED", message: "no" }),
          });
        }
        return json(route, { ticket: "T1", items: body.items, expires_in_s: 60 });
      }
      case "/ui/intent": {
        const body = req.postDataJSON() as {
          intent: { payload: Sent["payload"] };
          elevation_ticket?: string;
        };
        hub.sent.push({ payload: body.intent.payload, ticket: body.elevation_ticket });
        if (hub.refuse && !body.elevation_ticket) {
          return route.fulfill({
            status: hub.refuse.status,
            contentType: "application/json",
            headers: CORS,
            body: JSON.stringify({ code: hub.refuse.code, message: "no" }),
          });
        }
        const p = body.intent.payload;
        if (p.kind === "credit.set_limit") {
          hub.wallet.credit = {
            limit_minor: p.limit_minor ?? 0,
            outstanding_minor: hub.wallet.credit?.outstanding_minor ?? 0,
            reminder_days: p.reminder_days ?? [],
            paused: false,
          };
          return json(route, { applied: { intent_id: "I", changes: [] } });
        }
        hub.bills = [];
        return json(route, {
          applied: { intent_id: "I", changes: [] },
          settled: {
            order_id: "O1",
            settled_at: Date.now(),
            undo_until: Date.now() + 8000,
            order_rev: 5,
            total_minor: 7500,
            irreversible: false,
          },
        });
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
}

async function openProfile(page: Page, hub: Hub) {
  await serve(page, hub);
  await page.goto("/players");
  await page.getByLabel(en["hub.players.lookup.label"]).fill("98765");
  await page.getByTestId("player-lookup").getByText("Asha Rao").first().click();
  await expect(page.getByTestId("player-profile")).toBeVisible();
}

async function openBill(page: Page, hub: Hub) {
  await serve(page, hub);
  await page.goto("/floor");
  await page.locator("[data-component=floor-tile] [data-tile-face]").first().focus();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("settle-bar")).toBeVisible();
}

const bar = (page: Page) => page.getByTestId("settle-bar");
const method = (page: Page, name: string) => bar(page).locator(`[data-method=${name}]`);
const settleButton = (page: Page) =>
  bar(page).getByRole("button", { name: en["hub.settle.submit"], exact: true });
const serious = (violations: { impact?: string | null }[]) =>
  violations.filter((v) => v.impact === "serious" || v.impact === "critical");

test("credit is offered only for an enabled, unpaused credit with a limit", () => {
  expect(creditOffered(CREDIT)).toBe(true);
  expect(creditOffered(null)).toBe(false);
  expect(creditOffered(undefined)).toBe(false);
  expect(creditOffered({ ...CREDIT, paused: true })).toBe(false);
  expect(creditOffered({ ...CREDIT, limit_minor: 0 })).toBe(false);
  expect(headroomOf(CREDIT)).toBe(40_000);
  expect(headroomOf({ ...CREDIT, outstanding_minor: 60_000 })).toBe(0);
  expect(methodsFor(0, CREDIT)).toEqual(["cash", "upi", "card", "wallet", "credit"]);
  expect(methodsFor(0, null)).toEqual(["cash", "upi", "card", "wallet"]);
  expect(methodsFor(undefined, CREDIT)).toEqual(["cash", "upi", "card"]);
  const row: Row = { method: "credit", amount: "75", reference: "" };
  expect(toPayments([row], 0, 2)).toEqual([{ method: "credit", amount_minor: 7500 }]);
});

test("a manager enables credit with a limit and reminder days", async ({ page }) => {
  const hub = newHub();
  await openProfile(page, hub);
  await expect(page.getByTestId("credit-off")).toBeVisible();
  await page.getByRole("button", { name: en["hub.credit.enable"] }).click();
  await expect(page.getByLabel(en["hub.credit.days_field"])).toHaveValue("3, 7, 14");

  await page.getByLabel(en["hub.credit.limit_field"]).fill("0");
  await page.getByRole("button", { name: en["hub.credit.save"] }).click();
  await expect(page.getByText(en["hub.credit.err.limit"])).toBeVisible();
  await page.getByLabel(en["hub.credit.limit_field"]).fill("500");
  await page.getByLabel(en["hub.credit.days_field"]).fill("14, 3, x");
  await page.getByRole("button", { name: en["hub.credit.save"] }).click();
  await expect(page.getByText(en["hub.credit.err.days"])).toBeVisible();
  expect(hub.sent).toHaveLength(0);

  await page.getByLabel(en["hub.credit.days_field"]).fill("14, 3");
  await page.getByRole("button", { name: en["hub.credit.save"] }).click();
  await expect.poll(() => hub.sent.length).toBe(1);
  expect(hub.sent[0].payload).toEqual({
    kind: "credit.set_limit",
    player_id: "P1",
    limit_minor: 50_000,
    reminder_days: [3, 14],
  });
  await expect(page.getByTestId("credit-saved")).toHaveText(en["hub.credit.saved"]);
  await expect(page.getByTestId("credit-limit")).toHaveText("₹500.00");
  await expect(page.getByTestId("credit-outstanding")).toHaveText("₹0.00");
  await expect(page.getByTestId("credit-headroom")).toHaveText("₹500.00");
  await expect(page.getByTestId("credit-days")).toHaveText("3, 14");
  expect(serious((await new AxeBuilder({ page }).analyze()).violations)).toEqual([]);
});

test("a limit below what is owed shows the reason on the field", async ({ page }) => {
  const hub = newHub({
    wallet: { player_id: "P1", balance_minor: 0, txns: [], credit: CREDIT },
    refuse: { code: "CREDIT_BELOW_OUTSTANDING", status: 409 },
  });
  await openProfile(page, hub);
  await expect(page.getByTestId("credit-outstanding")).toHaveText("₹100.00");
  await page.getByRole("button", { name: en["hub.credit.edit"] }).click();
  await expect(page.getByLabel(en["hub.credit.limit_field"])).toHaveValue("500.00");
  await page.getByLabel(en["hub.credit.limit_field"]).fill("50");
  await page.getByRole("button", { name: en["hub.credit.save"] }).click();
  await expect(page.getByText(en["hub.credit.err.below"])).toBeVisible();
  await expect(page.getByTestId("credit-limit")).toHaveText("₹500.00");
});

test("a member without credit.manage sees the credit but no way to change it", async ({ page }) => {
  await openProfile(
    page,
    newHub({
      staff: COUNTER,
      wallet: { player_id: "P1", balance_minor: 0, txns: [], credit: CREDIT },
    }),
  );
  await expect(page.getByTestId("credit-limit")).toHaveText("₹500.00");
  await expect(page.getByTestId("credit-actions")).toHaveCount(0);
});

test("a credit-enabled bill offers a credit row with outstanding, limit and headroom", async ({
  page,
}) => {
  const hub = newHub({ staff: COUNTER, bills: [bill({ credit: CREDIT })] });
  await openBill(page, hub);
  await expect(bar(page).locator("[data-method]")).toHaveText([
    "Cash",
    "UPI",
    "Card",
    "Wallet",
    "Credit",
  ]);
  await method(page, "credit").click();
  const row = bar(page).locator("[data-row=credit]");
  await expect(row.getByLabel(en["hub.settle.amount.credit"])).toHaveValue("75.00");
  await expect(row.getByTestId("credit-info")).toHaveText(
    "Owes ₹100.00 · Limit ₹500.00 · Headroom ₹400.00",
  );
  await expect(row.locator("[data-reference]")).toHaveCount(0);
  expect(serious((await new AxeBuilder({ page }).analyze()).violations)).toEqual([]);
  await settleButton(page).click();
  await expect.poll(() => hub.sent.length).toBe(1);
  expect(hub.sent[0].payload).toMatchObject({
    kind: "session.settle",
    payments: [{ method: "credit", amount_minor: 7500 }],
  });
  expect(hub.sent[0].payload.credit_override).toBeUndefined();
});

test("a guest bill, a player without credit and paused credit offer no credit row", async ({
  page,
}) => {
  const guest = newHub({ staff: COUNTER, bills: [bill({ wallet_minor: undefined })] });
  await openBill(page, guest);
  await expect(method(page, "credit")).toHaveCount(0);
  await page.unrouteAll({ behavior: "ignoreErrors" });

  const plain = newHub({ staff: COUNTER, bills: [bill({})] });
  await openBill(page, plain);
  await expect(method(page, "credit")).toHaveCount(0);
  await page.unrouteAll({ behavior: "ignoreErrors" });

  const paused = newHub({
    staff: COUNTER,
    bills: [bill({ credit: { ...CREDIT, paused: true } })],
  });
  await openBill(page, paused);
  await expect(method(page, "credit")).toHaveCount(0);
});

test("going over the limit shows the reason and a manager PIN override settles it", async ({
  page,
}) => {
  const hub = newHub({
    staff: COUNTER,
    bills: [bill({ credit: { ...CREDIT, outstanding_minor: 48_000 } })],
    refuse: { code: "CREDIT_LIMIT", status: 409 },
  });
  await openBill(page, hub);
  await method(page, "credit").click();
  await bar(page)
    .locator("[data-row=credit]")
    .getByLabel(en["hub.settle.amount.credit"])
    .fill("75");
  await settleButton(page).click();
  await expect(page.getByTestId("unlock-status")).toHaveText(`⚠ ${en["hub.settle.credit_limit"]}`);
  const override = page.getByRole("button", { name: en["hub.settle.credit_override"] });
  await expect(override).toBeVisible();
  await override.click();
  await page.keyboard.type(PIN);
  await page.keyboard.press("Enter");
  await expect.poll(() => hub.sent.length).toBe(2);
  expect(hub.sent[0].ticket).toBeUndefined();
  expect(hub.sent[0].payload.credit_override).toBeUndefined();
  expect(hub.sent[1].ticket).toBe("T1");
  expect(hub.sent[1].payload).toMatchObject({
    kind: "session.settle",
    credit_override: true,
    payments: [{ method: "credit", amount_minor: 7500 }],
  });
  await expect(page.getByTestId("settle-done")).toBeVisible();
});

test("a second override on the same day is refused in place", async ({ page }) => {
  const hub = newHub({
    staff: MANAGER,
    bills: [bill({ credit: { ...CREDIT, outstanding_minor: 48_000 } })],
    refuse: { code: "CREDIT_LIMIT", status: 409 },
  });
  await openBill(page, hub);
  await method(page, "credit").click();
  await bar(page)
    .locator("[data-row=credit]")
    .getByLabel(en["hub.settle.amount.credit"])
    .fill("75");
  await settleButton(page).click();
  hub.refuse = { code: "OVERRIDE_USED_TODAY", status: 409 };
  await page.getByRole("button", { name: en["hub.settle.credit_override"] }).click();
  await expect.poll(() => hub.sent.length).toBe(2);
  expect(hub.sent[1].ticket).toBeUndefined();
  expect(hub.sent[1].payload.credit_override).toBe(true);
  await expect(page.getByTestId("unlock-status")).toHaveText(`⚠ ${en["hub.settle.override_used"]}`);
  await expect(page.getByRole("button", { name: en["hub.settle.credit_override"] })).toHaveCount(0);
});
