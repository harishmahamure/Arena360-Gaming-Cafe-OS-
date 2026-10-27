import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type {
  CreditCollection,
  CreditView,
  PaymentRow,
  PlayerHit,
  SessionStaff,
  WalletView,
} from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { CORS, HUB, json, snapshot } from "./floor-fixture";

const COUNTER: SessionStaff = {
  staff_id: "01JCOUNTER0000000000000000",
  display_name: "Counter",
  role_name: "Counter",
  permissions: ["pos.sell", "player.view", "session.start", "credit.collect"],
};

const VIEWER: SessionStaff = {
  ...COUNTER,
  permissions: ["pos.sell", "player.view", "session.start"],
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
    order_id?: string;
    amount_minor?: number;
    payments?: PaymentRow[];
  };
  rev?: number;
};

type Hub = {
  sent: Sent[];
  staff: SessionStaff;
  wallet: WalletView;
  refuse?: { code: string; status: number };
};

const newHub = (extra: Partial<Hub> = {}): Hub => ({
  sent: [],
  staff: COUNTER,
  wallet: { player_id: "P1", balance_minor: 2_500, txns: [], credit: { ...CREDIT } },
  ...extra,
});

async function serve(page: Page, hub: Hub) {
  await page.addInitScript(() => sessionStorage.setItem("arena.hub.token", "tok"));
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot": {
        const snap = snapshot([]);
        snap.sessions = [];
        snap.bills = [];
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
      case "/ui/intent": {
        const body = req.postDataJSON() as {
          intent: { payload: Sent["payload"]; base_rev?: number };
        };
        const p = body.intent.payload;
        hub.sent.push({ payload: p, rev: body.intent.base_rev });
        if (hub.refuse) {
          return route.fulfill({
            status: hub.refuse.status,
            contentType: "application/json",
            headers: CORS,
            body: JSON.stringify({ code: hub.refuse.code, message: "no" }),
          });
        }
        if (p.kind === "credit.collect" && hub.wallet.credit) {
          const amount = p.amount_minor ?? 0;
          const paidFromWallet = (p.payments ?? [])
            .filter((x) => x.method === "wallet")
            .reduce((sum, x) => sum + x.amount_minor, 0);
          hub.wallet.credit.outstanding_minor -= amount;
          hub.wallet.balance_minor -= paidFromWallet;
          const collection: CreditCollection = {
            player_id: "P1",
            amount_minor: amount,
            cleared: [
              {
                order_id: "01JSPEND0000000000AAAAAA",
                at: 1_700_000_000_000,
                spend_minor: 6_000,
                cleared_minor: 6_000,
              },
              {
                order_id: "01JSPEND0000000000BBBBBB",
                at: 1_700_000_100_000,
                spend_minor: 4_000,
                cleared_minor: amount - 6_000,
              },
            ].filter((c) => c.cleared_minor > 0),
            outstanding_after_minor: hub.wallet.credit.outstanding_minor,
          };
          return json(route, {
            applied: { intent_id: "I", changes: [] },
            settled: {
              order_id: "CO1",
              settled_at: Date.now(),
              undo_until: Date.now() + 8000,
              order_rev: 1,
              total_minor: amount,
              irreversible: false,
              collection,
            },
          });
        }
        if (p.kind === "credit.collect.reverted" && hub.wallet.credit) {
          hub.wallet.credit.outstanding_minor = CREDIT.outstanding_minor;
          return json(route, { applied: { intent_id: "I", changes: [] } });
        }
        return json(route, { applied: { intent_id: "I", changes: [] } });
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

const bar = (page: Page) => page.getByTestId("settle-bar");
const method = (page: Page, name: string) => bar(page).locator(`[data-method=${name}]`);
const settleButton = (page: Page) =>
  bar(page).getByRole("button", { name: en["hub.settle.submit"], exact: true });
const collectButton = (page: Page) =>
  page.getByRole("button", { name: en["hub.credit.collect"], exact: true });
const serious = (violations: { impact?: string | null }[]) =>
  violations.filter((v) => v.impact === "serious" || v.impact === "critical");

test("Collect opens prefilled with the full outstanding and refuses zero or too much", async ({
  page,
}) => {
  const hub = newHub();
  await openProfile(page, hub);
  await expect(page.getByTestId("credit-outstanding")).toHaveText("₹100.00");
  await collectButton(page).click();
  const amount = page.getByLabel(en["hub.credit.collect.amount"]);
  await expect(amount).toHaveValue("100.00");

  await amount.fill("0");
  await page.getByRole("button", { name: en["hub.credit.collect.continue"] }).click();
  await expect(page.getByText(en["hub.credit.collect.err.amount"])).toBeVisible();
  await amount.fill("100.01");
  await page.getByRole("button", { name: en["hub.credit.collect.continue"] }).click();
  await expect(page.getByText(en["hub.credit.collect.err.over"])).toBeVisible();
  expect(hub.sent).toHaveLength(0);
  expect(serious((await new AxeBuilder({ page }).analyze()).violations)).toEqual([]);

  await page.getByRole("button", { name: en["hub.credit.collect.cancel"] }).click();
  await expect(page.getByTestId("collect")).toHaveCount(0);
  await expect(collectButton(page)).toBeVisible();
});

test("a partial collection takes cash and the wallet, shows what it cleared and can be undone", async ({
  page,
}) => {
  const hub = newHub();
  await openProfile(page, hub);
  await collectButton(page).click();
  await page.getByLabel(en["hub.credit.collect.amount"]).fill("80");
  await page.getByRole("button", { name: en["hub.credit.collect.continue"] }).click();

  await expect(page.getByTestId("settle-total")).toHaveText("₹80.00");
  await expect(bar(page).locator("[data-method]")).toHaveText(["Cash", "UPI", "Card", "Wallet"]);
  await expect(method(page, "credit")).toHaveCount(0);
  await method(page, "wallet").click();
  await bar(page)
    .locator("[data-row=wallet]")
    .getByLabel(en["hub.settle.amount.wallet"])
    .fill("20");
  await method(page, "cash").click();
  await settleButton(page).click();

  await expect.poll(() => hub.sent.length).toBe(1);
  expect(hub.sent[0].payload).toMatchObject({
    kind: "credit.collect",
    player_id: "P1",
    amount_minor: 8_000,
  });
  expect(hub.sent[0].payload.payments).toEqual([
    { method: "wallet", amount_minor: 2_000 },
    { method: "cash", amount_minor: 6_000 },
  ]);
  expect(hub.sent[0].rev).toBeUndefined();

  const summary = page.getByTestId("collection-summary");
  await expect(summary).toBeVisible();
  await expect(page.getByTestId("collection-amount")).toHaveText("₹80.00");
  await expect(page.getByTestId("collection-after")).toHaveText("₹20.00");
  await expect(page.getByTestId("collection-spend")).toHaveCount(2);
  await expect(page.getByTestId("collection-spend").first()).toContainText("AAAAAA");
  await expect(page.getByTestId("collection-spend").first()).toContainText("₹60.00 of ₹60.00");
  await expect(page.getByTestId("collection-spend").nth(1)).toContainText("₹20.00 of ₹40.00");
  await expect(page.getByTestId("credit-outstanding")).toHaveText("₹20.00");
  await expect(page.getByTestId("undo-message")).toHaveText("Credit collected · ₹80.00");
  expect(serious((await new AxeBuilder({ page }).analyze()).violations)).toEqual([]);

  await page.getByTestId("undo-action").click();
  await expect.poll(() => hub.sent.length).toBe(2);
  expect(hub.sent[1].payload).toMatchObject({ kind: "credit.collect.reverted", order_id: "CO1" });
  expect(hub.sent[1].rev).toBe(1);
  await expect(page.getByTestId("undo-notice")).toHaveText(en["hub.undo.done.collect"]);
});

test("a collection the Hub refuses stays open with the reason", async ({ page }) => {
  const hub = newHub({ refuse: { code: "VALIDATION_FAILED", status: 422 } });
  await openProfile(page, hub);
  await collectButton(page).click();
  await page.getByRole("button", { name: en["hub.credit.collect.continue"] }).click();
  await method(page, "cash").click();
  await settleButton(page).click();
  await expect(page.getByTestId("collect-error")).toHaveText(
    `⚠ ${en["hub.credit.collect.refused"]}`,
  );
  await expect(page.getByTestId("collection-summary")).toHaveCount(0);
});

test("no Collect without credit.collect or without anything owed", async ({ page }) => {
  await openProfile(page, newHub({ staff: VIEWER }));
  await expect(page.getByTestId("credit-outstanding")).toHaveText("₹100.00");
  await expect(collectButton(page)).toHaveCount(0);
  await page.unrouteAll({ behavior: "ignoreErrors" });

  const settled = newHub();
  settled.wallet.credit = { ...CREDIT, outstanding_minor: 0 };
  await openProfile(page, settled);
  await expect(page.getByTestId("credit-outstanding")).toHaveText("₹0.00");
  await expect(collectButton(page)).toHaveCount(0);
});
