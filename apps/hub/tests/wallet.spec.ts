import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type {
  PaymentRow,
  PlayerHit,
  SessionStaff,
  SnapshotBill,
  SnapshotDevice,
  WalletView,
} from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { methodsFor, rowErrors, toPayments, type Row } from "../src/lib/settle";
import { CORS, HUB, device, json, snapshot } from "./floor-fixture";

const COUNTER: SessionStaff = {
  staff_id: "01JCOUNTER0000000000000000",
  display_name: "Counter",
  role_name: "Counter",
  permissions: ["pos.sell", "player.view", "player.manage", "wallet.topup", "session.start"],
};

const ASHA: PlayerHit = {
  player_id: "P1",
  name: "Asha Rao",
  username: "asha_9",
  phone: "+919876543210",
  tags: [],
};

type Sent = {
  payload: {
    kind: string;
    order_id?: string;
    player_id?: string;
    amount_minor?: number;
    payments?: PaymentRow[];
  };
  rev?: number;
};

type Hub = {
  sent: Sent[];
  wallet: WalletView;
  devices: SnapshotDevice[];
  bills: SnapshotBill[];
};

const FREE = device(1, { status: "online" });
const bill = (extra: Partial<SnapshotBill>): SnapshotBill => ({
  session_id: "S1",
  device_id: FREE.device_id,
  order_id: "O1",
  order_rev: 4,
  lines: [],
  subtotal_minor: 7500,
  paid_minor: 0,
  remaining_minor: 7500,
  ...extra,
});

const newHub = (extra: Partial<Hub> = {}): Hub => ({
  sent: [],
  wallet: {
    player_id: "P1",
    balance_minor: 2000,
    txns: [
      { id: "T2", at: 1_800_000_100_000, delta_minor: -3000, reason: "bill", staff: "Counter" },
      {
        id: "T1",
        at: 1_800_000_000_000,
        delta_minor: 5000,
        reason: "topup",
        method: "cash",
        staff: "Counter",
      },
    ],
  },
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
        return json(route, COUNTER);
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
      case "/ui/intent": {
        const body = req.postDataJSON() as {
          intent: { payload: Sent["payload"]; base_rev?: number };
        };
        hub.sent.push({ payload: body.intent.payload, rev: body.intent.base_rev });
        if (body.intent.payload.kind === "wallet.topup") {
          const amount = body.intent.payload.amount_minor ?? 0;
          hub.wallet.balance_minor += amount;
          return json(route, {
            applied: { intent_id: "I", changes: [] },
            settled: {
              order_id: "TO1",
              settled_at: Date.now(),
              undo_until: Date.now() + 8000,
              order_rev: 1,
              total_minor: amount,
              irreversible: false,
            },
          });
        }
        if (body.intent.payload.kind === "wallet.topup.reverted") {
          hub.wallet.balance_minor = 2000;
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

test("only a profiled bill with a wallet offers the wallet method", () => {
  expect(methodsFor(undefined)).toEqual(["cash", "upi", "card"]);
  expect(methodsFor(null)).toEqual(["cash", "upi", "card"]);
  expect(methodsFor(0)).toEqual(["cash", "upi", "card", "wallet"]);
  const row = (amount: string): Row => ({ method: "wallet", amount, reference: "" });
  expect(rowErrors(row("20"), 2, 2000)).toEqual({});
  expect(rowErrors(row("20.01"), 2, 2000).amount).toBe("hub.settle.err.wallet");
  expect(toPayments([row("20")], 0, 2)).toEqual([{ method: "wallet", amount_minor: 2000 }]);
});

test("the profile shows the wallet balance and lists transactions newest first with a running balance", async ({
  page,
}) => {
  await openProfile(page, newHub());
  await expect(page.getByTestId("wallet-balance")).toHaveText("₹20.00");
  await page.getByRole("button", { name: en["hub.wallet.list.show"] }).click();
  const rows = page.getByTestId("wallet-row");
  await expect(rows).toHaveCount(2);
  await expect(rows.nth(0).getByTestId("wallet-delta")).toHaveText("−₹30.00");
  await expect(rows.nth(0).getByTestId("wallet-running")).toHaveText("₹20.00");
  await expect(rows.nth(0)).toContainText(en["hub.wallet.reason.bill"]);
  await expect(rows.nth(1).getByTestId("wallet-delta")).toHaveText("+₹50.00");
  await expect(rows.nth(1).getByTestId("wallet-running")).toHaveText("₹50.00");
  await expect(rows.nth(1)).toContainText(en["hub.wallet.method.cash"]);
  const axe = await new AxeBuilder({ page }).analyze();
  expect(axe.violations.filter((v) => v.impact === "serious" || v.impact === "critical")).toEqual(
    [],
  );
});

test("Top up opens the settle bar with cash, UPI and card only, then settles and offers Undo", async ({
  page,
}) => {
  const hub = newHub();
  await openProfile(page, hub);
  await page.getByRole("button", { name: en["hub.wallet.topup"], exact: true }).click();
  await page.getByLabel(en["hub.wallet.topup.amount"]).fill("0");
  await page.getByRole("button", { name: en["hub.wallet.topup.continue"] }).click();
  await expect(page.getByText(en["hub.wallet.topup.err.amount"])).toBeVisible();
  await page.getByLabel(en["hub.wallet.topup.amount"]).fill("500");
  await page.getByRole("button", { name: en["hub.wallet.topup.continue"] }).click();

  await expect(page.getByTestId("settle-total")).toHaveText("₹500.00");
  await expect(bar(page).locator("[data-method]")).toHaveText(["Cash", "UPI", "Card"]);
  await expect(method(page, "wallet")).toHaveCount(0);

  await method(page, "cash").click();
  await settleButton(page).click();

  await expect.poll(() => hub.sent.length).toBe(1);
  expect(hub.sent[0].payload).toMatchObject({
    kind: "wallet.topup",
    player_id: "P1",
    amount_minor: 50000,
    payments: [{ method: "cash", amount_minor: 50000 }],
  });
  expect(hub.sent[0].rev).toBeUndefined();
  await expect(page.getByTestId("undo-message")).toHaveText("Wallet topped up · ₹500.00");
  await expect(page.getByTestId("wallet-balance")).toHaveText("₹520.00");

  await page.getByTestId("undo-action").click();
  await expect.poll(() => hub.sent.length).toBe(2);
  expect(hub.sent[1].payload).toMatchObject({ kind: "wallet.topup.reverted", order_id: "TO1" });
  expect(hub.sent[1].rev).toBe(1);
  await expect(page.getByTestId("undo-notice")).toHaveText(en["hub.undo.done.topup"]);
});

test("a profiled bill offers a wallet row capped at the balance and settles with it", async ({
  page,
}) => {
  const hub = newHub({ bills: [bill({ wallet_minor: 5000 })] });
  await openBill(page, hub);
  await expect(bar(page).locator("[data-method]")).toHaveText(["Cash", "UPI", "Card", "Wallet"]);
  await method(page, "wallet").click();
  const wallet = bar(page).locator("[data-row=wallet]");
  await expect(wallet.getByLabel(en["hub.settle.amount.wallet"])).toHaveValue("50.00");
  await expect(wallet.getByTestId("wallet-balance")).toHaveText("Wallet balance ₹50.00");
  await expect(page.getByTestId("settle-remainder")).toContainText("₹25.00");

  await wallet.getByLabel(en["hub.settle.amount.wallet"]).fill("60");
  await method(page, "cash").click();
  await bar(page).locator("[data-row=cash]").getByLabel(en["hub.settle.amount.cash"]).fill("15");
  await settleButton(page).click();
  await expect(bar(page).getByText(en["hub.settle.err.wallet"])).toBeVisible();
  expect(hub.sent).toHaveLength(0);

  await wallet.getByLabel(en["hub.settle.amount.wallet"]).fill("50");
  await bar(page).locator("[data-row=cash]").getByLabel(en["hub.settle.amount.cash"]).fill("25");
  await settleButton(page).click();
  await expect.poll(() => hub.sent.length).toBe(1);
  expect(hub.sent[0].payload).toMatchObject({
    kind: "session.settle",
    order_id: "O1",
    payments: [
      { method: "wallet", amount_minor: 5000 },
      { method: "cash", amount_minor: 2500 },
    ],
  });
  const axe = await new AxeBuilder({ page }).analyze();
  expect(axe.violations.filter((v) => v.impact === "serious" || v.impact === "critical")).toEqual(
    [],
  );
});

test("a guest bill offers no wallet method", async ({ page }) => {
  await openBill(page, newHub({ bills: [bill({})] }));
  await expect(bar(page).locator("[data-method]")).toHaveText(["Cash", "UPI", "Card"]);
  await expect(method(page, "wallet")).toHaveCount(0);
});
