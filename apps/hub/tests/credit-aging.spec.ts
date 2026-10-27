import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type {
  CreditAging,
  CreditView,
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

const ASHA: PlayerHit = {
  player_id: "P1",
  name: "Asha Rao",
  username: "asha_9",
  phone: "+919876543210",
  tags: [],
  outstanding_minor: 25_000,
};

const CREDIT: CreditView = {
  limit_minor: 50_000,
  outstanding_minor: 25_000,
  reminder_days: [3, 7, 14],
  paused: false,
};

const AGING: CreditAging = {
  oldest_unpaid_day: "2026-10-05",
  oldest_age_days: 19,
  bucket_0_7_minor: 5_000,
  bucket_8_14_minor: 8_000,
  bucket_15_plus_minor: 12_000,
};

const CLEAR: CreditAging = {
  oldest_unpaid_day: null,
  oldest_age_days: 0,
  bucket_0_7_minor: 0,
  bucket_8_14_minor: 0,
  bucket_15_plus_minor: 0,
};

type Hub = { hit: PlayerHit; wallet: WalletView; sent: { kind: string }[] };

const newHub = (extra: Partial<Hub> = {}): Hub => ({
  hit: { ...ASHA },
  wallet: {
    player_id: "P1",
    balance_minor: 0,
    txns: [],
    credit: { ...CREDIT },
    aging: { ...AGING },
  },
  sent: [],
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
        return json(route, [hub.hit]);
      case "/ui/players/profile":
        return json(route, {
          player: hub.hit,
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
        const body = req.postDataJSON() as { intent: { payload: { kind: string } } };
        hub.sent.push({ kind: body.intent.payload.kind });
        if (body.intent.payload.kind === "credit.collect" && hub.wallet.credit) {
          hub.wallet.credit.outstanding_minor = 0;
          hub.wallet.credit.paused = false;
          hub.wallet.aging = { ...CLEAR };
          return json(route, {
            applied: { intent_id: "I", changes: [] },
            settled: {
              order_id: "CO1",
              settled_at: Date.now(),
              undo_until: Date.now() + 8000,
              order_rev: 1,
              total_minor: 25_000,
              irreversible: false,
              collection: {
                player_id: "P1",
                amount_minor: 25_000,
                cleared: [],
                outstanding_after_minor: 0,
              },
            },
          });
        }
        return json(route, { applied: { intent_id: "I", changes: [] } });
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
}

async function lookup(page: Page, hub: Hub) {
  await serve(page, hub);
  await page.goto("/players");
  await page.getByLabel(en["hub.players.lookup.label"]).fill("98765");
  await expect(page.getByTestId("player-hits")).toBeVisible();
}

async function openProfile(page: Page, hub: Hub) {
  await lookup(page, hub);
  await page.getByTestId("player-lookup").getByText("Asha Rao").first().click();
  await expect(page.getByTestId("player-profile")).toBeVisible();
}

const serious = (violations: { impact?: string | null }[]) =>
  violations.filter((v) => v.impact === "serious" || v.impact === "critical");

test("a lookup row puts what the player owes first, before name and phone", async ({ page }) => {
  await lookup(page, newHub());
  const row = page.getByTestId("player-hits").getByRole("button").first();
  await expect(page.getByTestId("hit-owes")).toHaveText("⚠ Owes ₹250.00");
  const text = (await row.innerText()).replace(/\s+/g, " ");
  expect(text.indexOf("Owes ₹250.00")).toBeLessThan(text.indexOf("Asha Rao"));
  expect(text.indexOf("Asha Rao")).toBeLessThan(text.indexOf("+919876543210"));
  expect(serious((await new AxeBuilder({ page }).analyze()).violations)).toEqual([]);
});

test("a lookup row for a player who owes nothing has no warning", async ({ page }) => {
  await lookup(page, newHub({ hit: { ...ASHA, outstanding_minor: 0 } }));
  await expect(page.getByTestId("player-hits").getByText("Asha Rao")).toBeVisible();
  await expect(page.getByTestId("hit-owes")).toHaveCount(0);
});

test("the profile opens with the outstanding total, the three buckets and the oldest unpaid date", async ({
  page,
}) => {
  await openProfile(page, newHub());
  await expect(page.getByTestId("outstanding-total")).toHaveText("₹250.00");
  await expect(page.getByTestId("aging-0-7")).toHaveText("₹50.00");
  await expect(page.getByTestId("aging-8-14")).toHaveText("₹80.00");
  await expect(page.getByTestId("aging-15-plus")).toHaveText("₹120.00");
  await expect(page.getByTestId("aging-oldest")).toHaveText("2026-10-05");
  await expect(page.getByTestId("credit-paused")).toHaveCount(0);

  const outstanding = await page.getByTestId("outstanding").boundingBox();
  const profile = await page.getByTestId("player-profile").boundingBox();
  expect(outstanding!.y).toBeLessThan(profile!.y);
  expect(serious((await new AxeBuilder({ page }).analyze()).violations)).toEqual([]);
});

test("a paused player shows why, and a collection that clears the tab reopens credit", async ({
  page,
}) => {
  const hub = newHub();
  hub.wallet.credit!.paused = true;
  await openProfile(page, hub);
  await expect(page.getByTestId("credit-paused")).toHaveText(`⚠ ${en["hub.credit.paused"]}`);
  expect(en["hub.credit.paused"]).toBe("Credit paused — 15+ days overdue");
  expect(serious((await new AxeBuilder({ page }).analyze()).violations)).toEqual([]);

  await page.getByRole("button", { name: en["hub.credit.collect"], exact: true }).click();
  await page.getByRole("button", { name: en["hub.credit.collect.continue"] }).click();
  const bar = page.getByTestId("settle-bar");
  await bar.locator("[data-method=cash]").click();
  await bar.getByRole("button", { name: en["hub.settle.submit"], exact: true }).click();
  await expect.poll(() => hub.sent.length).toBe(1);
  await expect(page.getByTestId("outstanding")).toHaveCount(0);
  await expect(page.getByTestId("credit-paused")).toHaveCount(0);
});

test("a player who owes nothing has no outstanding section", async ({ page }) => {
  const hub = newHub();
  hub.wallet.credit = { ...CREDIT, outstanding_minor: 0 };
  hub.wallet.aging = { ...CLEAR };
  await openProfile(page, hub);
  await expect(page.getByTestId("credit-outstanding")).toHaveText("₹0.00");
  await expect(page.getByTestId("outstanding")).toHaveCount(0);
});
