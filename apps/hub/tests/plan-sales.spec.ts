import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type {
  OwnedPlanView,
  PaymentRow,
  PlanView,
  PlayerHit,
  SessionStaff,
  WalletView,
} from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
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
  outstanding_minor: 0,
};

const NOW = Date.now();
const DAY = 24 * 60 * 60 * 1000;

const PACK: PlanView = {
  id: "PL1",
  plan_kind: "pack",
  name: "5 h pack",
  price_minor: 30000,
  minutes: 300,
  validity_days: 7,
  group_ids: ["G1"],
  active: true,
  rev: 1,
};
const PASS: PlanView = {
  id: "PL2",
  plan_kind: "pass",
  name: "Night pass",
  price_minor: 90000,
  validity_days: 30,
  window_start_min: 1320,
  window_end_min: 360,
  group_ids: ["G1"],
  active: true,
  rev: 1,
};
const BLOCK: PlanView = {
  id: "PL3",
  plan_kind: "block",
  name: "Evening block",
  price_minor: 20000,
  window_start_min: 1080,
  window_end_min: 1410,
  group_ids: ["G1"],
  active: true,
  rev: 1,
};
const OFF: PlanView = { ...PACK, id: "PL4", name: "Retired pack", active: false };

type Sent = {
  payload: { kind: string; plan_id?: string; order_id?: string; payments?: PaymentRow[] };
  rev?: number;
};

type Hub = { sent: Sent[]; plans: PlanView[]; wallet: WalletView; refuse?: string };

const owned = (extra: Partial<OwnedPlanView>): OwnedPlanView => ({
  id: "K1",
  plan_id: "PL1",
  plan_kind: "pack",
  name: "5 h pack",
  minutes_left: 105,
  minutes_total: 300,
  group_ids: ["G1"],
  bought_at: NOW - DAY,
  expires_at: NOW + 6 * DAY,
  expired: false,
  ...extra,
});

const newHub = (extra: Partial<Hub> = {}): Hub => ({
  sent: [],
  plans: [PACK, PASS, BLOCK, OFF],
  wallet: { player_id: "P1", balance_minor: 0, txns: [], plans: [] },
  ...extra,
});

async function serve(page: Page, hub: Hub) {
  await page.addInitScript(() => sessionStorage.setItem("arena.hub.token", "tok"));
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot": {
        const snap = snapshot([device(1, { status: "online" })]);
        snap.sessions = [];
        snap.bills = [];
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
      case "/ui/plans":
        return json(route, hub.plans);
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
        if (hub.refuse) {
          return route.fulfill({
            status: 409,
            headers: CORS,
            contentType: "application/json",
            body: JSON.stringify({ code: hub.refuse, message: "refused", field: "plan_id" }),
          });
        }
        if (body.intent.payload.kind === "plan.sell") {
          const plan = hub.plans.find((p) => p.id === body.intent.payload.plan_id);
          hub.wallet.plans = [
            owned({
              plan_id: plan?.id ?? "",
              plan_kind: plan?.plan_kind ?? "pack",
              name: plan?.name ?? "",
              minutes_left: plan?.minutes ?? undefined,
              minutes_total: plan?.minutes ?? undefined,
              window_start_min: plan?.window_start_min,
              window_end_min: plan?.window_end_min,
            }),
          ];
          return json(route, {
            applied: { intent_id: "I", changes: [] },
            settled: {
              order_id: "SO1",
              settled_at: Date.now(),
              undo_until: Date.now() + 8000,
              order_rev: 1,
              total_minor: plan?.price_minor ?? 0,
              irreversible: false,
            },
          });
        }
        hub.wallet.plans = [];
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

test("Packs & passes lists each owned plan with time left, window, expiry and groups", async ({
  page,
}) => {
  const hub = newHub({
    wallet: {
      player_id: "P1",
      balance_minor: 0,
      txns: [],
      plans: [
        owned({}),
        owned({
          id: "K2",
          plan_id: "PL2",
          plan_kind: "pass",
          name: "Night pass",
          minutes_left: undefined,
          minutes_total: undefined,
          window_start_min: 1320,
          window_end_min: 360,
          expires_at: NOW + 20 * DAY,
        }),
        owned({
          id: "K3",
          name: "Old pack",
          minutes_left: 45,
          expired: true,
          expires_at: NOW - DAY,
        }),
      ],
    },
  });
  await openProfile(page, hub);
  const rows = page.getByTestId("owned-row");
  await expect(rows).toHaveCount(3);
  await expect(rows.nth(0).getByTestId("owned-left")).toHaveText("1 h 45 min left");
  await expect(rows.nth(0)).toContainText("Gaming");
  await expect(rows.nth(0).getByTestId("owned-expiry")).toContainText("Expires");
  await expect(rows.nth(1).getByTestId("owned-window")).toHaveText("Plays 22:00–06:00");
  await expect(rows.nth(2)).toHaveAttribute("data-expired", "true");
  await expect(rows.nth(2).getByTestId("owned-expired")).toHaveText("Expired");
  await expect(rows.nth(2).getByTestId("owned-left")).toHaveText("0 h 45 min unused");
  const axe = await new AxeBuilder({ page }).analyze();
  expect(axe.violations.filter((v) => v.impact === "serious" || v.impact === "critical")).toEqual(
    [],
  );
});

test("a profile with no plans says so", async ({ page }) => {
  await openProfile(page, newHub());
  await expect(page.getByTestId("owned-empty")).toHaveText(en["hub.plans.owned.empty"]);
});

test("Sell plan offers active packs and passes only, settles with the plan line and offers Undo", async ({
  page,
}) => {
  const hub = newHub();
  await openProfile(page, hub);
  await page.getByRole("button", { name: en["hub.plans.sell"], exact: true }).click();
  const list = page.getByTestId("sell-list");
  await expect(list.locator("li")).toHaveCount(2);
  await expect(list).toContainText("5 h pack");
  await expect(list).toContainText("Night pass");
  await expect(list).not.toContainText("Evening block");
  await expect(list).not.toContainText("Retired pack");

  await list.getByRole("button", { name: "Sell 5 h pack" }).click();
  await expect(page.getByTestId("sell-line")).toContainText("5 h pack");
  await expect(page.getByTestId("settle-total")).toHaveText("₹300.00");
  await expect(bar(page).locator("[data-method]")).toHaveText(["Cash", "UPI", "Card", "Wallet"]);
  await bar(page).locator("[data-method=cash]").click();
  await bar(page).getByRole("button", { name: en["hub.settle.submit"], exact: true }).click();

  await expect.poll(() => hub.sent.length).toBe(1);
  expect(hub.sent[0].payload).toMatchObject({
    kind: "plan.sell",
    plan_id: "PL1",
    payments: [{ method: "cash", amount_minor: 30000 }],
  });
  await expect(page.getByTestId("undo-message")).toHaveText("Plan sold · ₹300.00");
  await expect(page.getByTestId("owned-row")).toHaveCount(1);
  await expect(page.getByTestId("owned-left")).toHaveText("5 h left");

  await page.getByTestId("undo-action").click();
  await expect.poll(() => hub.sent.length).toBe(2);
  expect(hub.sent[1].payload).toMatchObject({ kind: "plan.sell.reverted", order_id: "SO1" });
  expect(hub.sent[1].rev).toBe(1);
  await expect(page.getByTestId("undo-notice")).toHaveText(en["hub.undo.done.plan"]);
});

test("a second pass is refused in place with the plain reason", async ({ page }) => {
  const hub = newHub({ refuse: "PASS_ALREADY_ACTIVE" });
  await openProfile(page, hub);
  await page.getByRole("button", { name: en["hub.plans.sell"], exact: true }).click();
  await page.getByTestId("sell-list").getByRole("button", { name: "Sell Night pass" }).click();
  await bar(page).locator("[data-method=cash]").click();
  await bar(page).getByRole("button", { name: en["hub.settle.submit"], exact: true }).click();
  await expect(page.getByTestId("sell-error")).toHaveText(
    `⚠ ${en["hub.plans.sell.pass_active"]}`.replace(/\s+/g, " "),
  );
  await expect(bar(page)).toBeVisible();
});

test("with no active pack or pass the list says where to add one", async ({ page }) => {
  await openProfile(page, newHub({ plans: [BLOCK, OFF] }));
  await page.getByRole("button", { name: en["hub.plans.sell"], exact: true }).click();
  await expect(page.getByTestId("sell-none")).toHaveText(en["hub.plans.sell.none"]);
});

test("a guest cannot buy a plan: staff are asked to create or link a profile first", async ({
  page,
}) => {
  await serve(page, newHub());
  await page.goto("/players");
  await page.getByRole("button", { name: en["hub.players.lookup.guest"] }).click();
  await page
    .getByTestId("guest-actions")
    .getByRole("button", { name: en["hub.plans.sell"] })
    .click();
  await expect(page.getByTestId("guest-sell-note")).toHaveText(en["hub.plans.sell.guest"]);
});
