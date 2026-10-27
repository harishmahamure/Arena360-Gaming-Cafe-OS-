import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type {
  OwnedPlanView,
  SessionStaff,
  SnapshotDevice,
  SnapshotSession,
} from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { CORS, HUB, device, json, snapshot } from "./floor-fixture";

const STAFF: SessionStaff = {
  staff_id: "01JLOOKUP00000000000000000",
  display_name: "Counter",
  role_name: "Counter",
  permissions: ["session.start", "session.extend", "player.view", "device.screen"],
};

const MIN = 60_000;
const DAY = 86_400_000;
const NOW = Date.now();

const owned = (id: string, over: Partial<OwnedPlanView>): OwnedPlanView => ({
  id,
  plan_id: `PL-${id}`,
  plan_kind: "pack",
  name: "5 h pack",
  minutes_left: 150,
  minutes_total: 300,
  group_ids: ["G1"],
  bought_at: NOW - DAY,
  expires_at: NOW + 6 * DAY,
  expired: false,
  ...over,
});

const PLANS: OwnedPlanView[] = [
  owned("PK0", { name: "Running pack" }),
  owned("PK1", {}),
  owned("PK2", { name: "VIP pack", group_ids: ["G2"] }),
  owned("PK3", { name: "Old pack", expired: true }),
  owned("PK4", { plan_kind: "pass", name: "Night pass", minutes_left: undefined }),
];

type Sent = { payload: Record<string, unknown> & { kind: string }; key: string };
type Hub = {
  devices: SnapshotDevice[];
  sessions: SnapshotSession[];
  sent: Sent[];
  fail?: { status: number; code: string; message?: string; field?: string };
  credit?: {
    limit_minor: number;
    outstanding_minor: number;
    reminder_days: number[];
    paused: boolean;
  };
};

const running = (over: Partial<SnapshotSession> = {}): SnapshotSession => ({
  id: "S1",
  device_id: device(1, {}).device_id,
  state: "active",
  started_at: NOW - 20 * MIN,
  ends_at: NOW + 40 * MIN,
  paused_total_ms: 0,
  rev: 3,
  player_id: "P1",
  username: "asha",
  mode: "pack",
  pack_id: "PK0",
  ...over,
});

async function open(page: Page, over: Partial<Hub> = {}): Promise<Hub> {
  const hub: Hub = {
    devices: [device(1, { status: "in_session" })],
    sessions: [running()],
    sent: [],
    ...over,
  };
  await page.addInitScript(() => {
    sessionStorage.setItem("arena.hub.token", "tok");
    class FakeEventSource {
      static CLOSED = 2;
      readyState = 1;
      addEventListener() {}
      close() {}
    }
    (window as unknown as Record<string, unknown>).EventSource = FakeEventSource;
  });
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    const url = new URL(req.url());
    switch (url.pathname) {
      case "/ui/snapshot": {
        const snap = snapshot(hub.devices, 10000);
        snap.sessions = hub.sessions;
        return json(route, snap);
      }
      case "/ui/me":
        return json(route, STAFF);
      case "/ui/settings":
        return json(route, []);
      case "/ui/players/wallet":
        return json(route, {
          player_id: "P1",
          balance_minor: 25000,
          txns: [],
          plans: PLANS,
          credit: hub.credit,
        });
      case "/ui/intent": {
        const body = req.postDataJSON() as {
          intent: { payload: Sent["payload"]; idempotency_key: string };
        };
        hub.sent.push({ payload: body.intent.payload, key: body.intent.idempotency_key });
        if (hub.fail) {
          return route.fulfill({
            status: hub.fail.status,
            contentType: "application/json",
            headers: CORS,
            body: JSON.stringify({ ...hub.fail }),
          });
        }
        return json(route, {
          applied: {
            intent_id: "I",
            changes: [{ epoch: "1:H", seq: 9, tbl: "sessions", row_id: "S1", op: "update" }],
          },
        });
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/floor");
  return hub;
}

const tile = (page: Page, label: string) =>
  page.locator("[data-component=floor-tile]", { hasText: label });
const sheet = (page: Page) => page.getByRole("complementary");
const act = (page: Page, name: string) => sheet(page).getByRole("button", { name, exact: true });
const status = (page: Page) => page.getByTestId("unlock-status");
const panel = (page: Page) => page.getByTestId("extend-options");

async function openSheet(page: Page) {
  await tile(page, "PC 1").locator("[data-tile-face]").focus();
  await page.keyboard.press("Enter");
  await expect(sheet(page)).toHaveAccessibleName("PC 1");
}

async function openExtend(page: Page) {
  await openSheet(page);
  await act(page, en["hub.session.extend"]).click();
  await expect(panel(page)).toBeVisible();
  await expect(page.getByTestId("extend-source-wallet")).toBeVisible();
}

test("Extend lists +30 min, +1 h and +2 h with live prices and the payment sources", async ({
  page,
}) => {
  await open(page, {
    credit: { limit_minor: 50000, outstanding_minor: 12000, reminder_days: [3], paused: false },
  });
  await openExtend(page);
  await expect(page.getByTestId("extend-minutes-30")).toHaveText("+30 min · ₹50.00");
  await expect(page.getByTestId("extend-minutes-60")).toHaveText("+1 h · ₹100.00");
  await expect(page.getByTestId("extend-minutes-120")).toHaveText("+2 h · ₹200.00");
  await expect(page.getByTestId("extend-source-wallet")).toContainText("₹250.00");
  const packs = page.getByTestId("extend-source-pack");
  await expect(packs).toHaveCount(2);
  await expect(packs.nth(0)).toContainText("5 h pack");
  await expect(packs.nth(0)).toContainText("2 h 30 min left");
  await expect(packs.nth(1)).toContainText("Old pack");
  await expect(packs.nth(1)).toHaveAttribute("aria-disabled", "true");
  await expect(panel(page)).not.toContainText("VIP pack");
  await expect(panel(page)).not.toContainText("Running pack");
  await expect(panel(page)).not.toContainText("Night pass");
  await expect(page.getByTestId("extend-source-credit")).toContainText("₹380.00");
  for (const el of await panel(page).getByRole("button").all()) {
    expect((await el.boundingBox())!.height).toBeGreaterThanOrEqual(44);
  }
  expect(
    (await new AxeBuilder({ page }).analyze()).violations.filter(
      (v) => v.impact === "serious" || v.impact === "critical",
    ),
  ).toEqual([]);
});

test("Extend stays disabled until a time and a source are chosen, then sends the wallet", async ({
  page,
}) => {
  const hub = await open(page);
  await openExtend(page);
  await expect(page.getByTestId("extend-source-credit")).toHaveCount(0);
  const confirm = act(page, en["hub.extend.confirm"]);
  await expect(confirm).toHaveAttribute("aria-disabled", "true");
  await expect(panel(page)).toContainText(en["hub.extend.pick_time"]);
  await page.getByTestId("extend-minutes-60").click();
  await expect(panel(page)).toContainText(en["hub.extend.pick_source"]);
  await page.getByTestId("extend-source-wallet").click();
  await confirm.click();
  await expect(status(page)).toHaveText(en["hub.sheet.done.extended"]);
  await expect(panel(page)).toHaveCount(0);
  expect(hub.sent.map((s) => s.payload)).toEqual([
    { kind: "session.extend", session_id: "S1", minutes: 60, source: "wallet" },
  ]);
});

test("Extending from a pack sends the pack id", async ({ page }) => {
  const hub = await open(page);
  await openExtend(page);
  await page.getByTestId("extend-minutes-30").click();
  await page.getByTestId("extend-source-pack").nth(0).click();
  await act(page, en["hub.extend.confirm"]).click();
  await expect(status(page)).toHaveText(en["hub.sheet.done.extended"]);
  expect(hub.sent.map((s) => s.payload)).toEqual([
    { kind: "session.extend", session_id: "S1", minutes: 30, source: "pack", pack_id: "PK1" },
  ]);
});

test("a refusal shows its reason and keeps the options open", async ({ page }) => {
  await open(page, { fail: { status: 409, code: "PAST_CLOSING", message: "x", field: "minutes" } });
  await openExtend(page);
  await page.getByTestId("extend-minutes-120").click();
  await page.getByTestId("extend-source-wallet").click();
  await act(page, en["hub.extend.confirm"]).click();
  await expect(status(page)).toContainText(en["hub.extend.past_closing"]);
  await expect(panel(page)).toBeVisible();
  await expect(page.getByTestId("extend-minutes-120")).toHaveAttribute("aria-pressed", "true");
});

test("a short wallet shows the balance the Hub reports", async ({ page }) => {
  await open(page, {
    fail: {
      status: 409,
      code: "INSUFFICIENT_BALANCE",
      message: "Balance ₹29.00. Top up at counter.",
      field: "source",
    },
  });
  await openExtend(page);
  await page.getByTestId("extend-minutes-30").click();
  await page.getByTestId("extend-source-wallet").click();
  await act(page, en["hub.extend.confirm"]).click();
  await expect(status(page)).toContainText("Balance ₹29.00. Top up at counter.");
  await expect(panel(page)).toBeVisible();
});

test("a postpaid session is not offered Extend", async ({ page }) => {
  await open(page, { sessions: [running({ mode: "postpaid", pack_id: null, ends_at: null })] });
  await openSheet(page);
  await expect(act(page, en["hub.session.end"])).toBeVisible();
  await expect(act(page, en["hub.session.extend"])).toHaveCount(0);
});

test("a wallet session in its grace period can be extended", async ({ page }) => {
  await open(page, {
    sessions: [
      running({
        mode: "wallet",
        pack_id: null,
        state: "grace",
        ends_at: NOW - MIN,
        grace_ends_at: NOW + MIN,
      }),
    ],
  });
  await openExtend(page);
  await expect(page.getByTestId("extend-source-pack")).toHaveCount(3);
});
