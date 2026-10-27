import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type {
  OwnedPlanView,
  PlanView,
  PlayerHit,
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
  permissions: ["session.start", "player.view", "device.screen"],
};

const PLAYERS: PlayerHit[] = [
  {
    player_id: "P1",
    name: "Asha Rao",
    username: "asha",
    phone: "+91 98765 43210",
    tags: [],
    outstanding_minor: 0,
  },
];

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
  owned("PK1", {}),
  owned("PK2", { name: "VIP pack", group_ids: ["G2"] }),
  owned("PK3", {
    name: "Old pack",
    minutes_left: 60,
    expires_at: NOW - DAY,
    expired: true,
  }),
  owned("PK4", {
    plan_kind: "pass",
    name: "Night pass",
    minutes_left: undefined,
    window_start_min: 1320,
    window_end_min: 360,
  }),
];

type Sent = { payload: Record<string, unknown> & { kind: string }; key: string };
type Hub = {
  devices: SnapshotDevice[];
  sessions: SnapshotSession[];
  sent: Sent[];
  fail?: { status: number; code: string; message?: string };
  plans?: PlanView[];
};

const BLOCK: PlanView = {
  id: "BL1",
  plan_kind: "block",
  name: "Night block",
  price_minor: 20000,
  window_start_min: 1320,
  window_end_min: 360,
  group_ids: ["G1"],
  active: true,
  rev: 1,
};

async function open(page: Page, over: Partial<Hub> = {}): Promise<Hub> {
  const hub: Hub = {
    devices: [device(1, { status: "online", manual: true })],
    sessions: [],
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
      case "/ui/players": {
        const digits = (url.searchParams.get("q") ?? "").replace(/\D/g, "");
        return json(
          route,
          digits ? PLAYERS.filter((p) => p.phone.replace(/\D/g, "").includes(digits)) : [],
        );
      }
      case "/ui/plans":
        return json(route, hub.plans ?? []);
      case "/ui/players/wallet":
        return json(route, { player_id: "P1", balance_minor: 25000, txns: [], plans: PLANS });
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
            body: JSON.stringify({ message: "no", ...hub.fail }),
          });
        }
        return json(route, {
          applied: {
            intent_id: "I",
            changes: [{ epoch: "1:H", seq: 9, tbl: "sessions", row_id: "S9", op: "insert" }],
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
const options = (page: Page) => page.getByTestId("start-options");

async function openSheet(page: Page) {
  await tile(page, "PC 1").locator("[data-tile-face]").focus();
  await page.keyboard.press("Enter");
  await expect(sheet(page)).toHaveAccessibleName("PC 1");
}

async function pickPlayer(page: Page) {
  await sheet(page).getByLabel(en["hub.players.lookup.label"]).fill("98765");
  await page
    .getByTestId("player-hits")
    .getByRole("button", { name: /Asha Rao/ })
    .click();
  await expect(options(page)).toBeVisible();
  await expect(page.getByTestId("start-option-wallet")).toBeVisible();
}

test("a picked player is offered Pay later, packs for this group and the wallet", async ({
  page,
}) => {
  await open(page);
  await openSheet(page);
  await pickPlayer(page);
  await expect(page.getByTestId("start-option-postpaid")).toHaveText(
    en["hub.session.opt.postpaid"],
  );
  const packs = page.getByTestId("start-option-pack");
  await expect(packs).toHaveCount(2);
  await expect(packs.nth(0)).toContainText("5 h pack");
  await expect(packs.nth(0)).toContainText("2 h 30 min left");
  await expect(packs.nth(0)).toHaveAttribute("data-expired", "false");
  await expect(packs.nth(1)).toContainText("Old pack");
  await expect(packs.nth(1)).toContainText("Expired");
  await expect(packs.nth(1)).toHaveAttribute("aria-disabled", "true");
  await expect(options(page)).not.toContainText("VIP pack");
  const passes = page.getByTestId("start-option-pass");
  await expect(passes).toHaveCount(1);
  await expect(passes.nth(0)).toContainText("Night pass");
  await expect(passes.nth(0)).toContainText("22:00–06:00");
  await expect(page.getByTestId("start-option-wallet")).toContainText("₹250.00");
  await expect(page.getByTestId("start-option-wallet")).toContainText("₹100.00/hr");
  for (const el of await options(page).getByRole("button").all()) {
    expect((await el.boundingBox())!.height).toBeGreaterThanOrEqual(44);
  }
  expect(
    (await new AxeBuilder({ page }).analyze()).violations.filter(
      (v) => v.impact === "serious" || v.impact === "critical",
    ),
  ).toEqual([]);
});

test("an expired pack cannot be selected", async ({ page }) => {
  await open(page);
  await openSheet(page);
  await pickPlayer(page);
  await page.getByTestId("start-option-pack").nth(1).click({ force: true });
  await expect(page.getByTestId("start-option-postpaid")).toHaveAttribute("aria-pressed", "true");
  await expect(page.getByTestId("start-option-pack").nth(1)).toHaveAttribute(
    "aria-pressed",
    "false",
  );
});

test("starting on a pack sends the pack id", async ({ page }) => {
  const hub = await open(page);
  await openSheet(page);
  await pickPlayer(page);
  await page.getByTestId("start-option-pack").nth(0).click();
  await act(page, en["hub.session.start_pack"]).click();
  await expect(status(page)).toHaveText(en["hub.sheet.done.started"]);
  expect(hub.sent.map((s) => s.payload)).toEqual([
    {
      kind: "session.start",
      device_id: hub.devices[0].device_id,
      mode: "pack",
      player_id: "P1",
      pack_id: "PK1",
    },
  ]);
});

test("starting on the wallet sends mode wallet", async ({ page }) => {
  const hub = await open(page);
  await openSheet(page);
  await pickPlayer(page);
  await page.getByTestId("start-option-wallet").click();
  await act(page, en["hub.session.start_wallet"]).click();
  await expect(status(page)).toHaveText(en["hub.sheet.done.started"]);
  expect(hub.sent.map((s) => s.payload)).toEqual([
    { kind: "session.start", device_id: hub.devices[0].device_id, mode: "wallet", player_id: "P1" },
  ]);
});

test("Pay later still sends a plain postpaid start", async ({ page }) => {
  const hub = await open(page);
  await openSheet(page);
  await pickPlayer(page);
  await act(page, en["hub.session.start"]).click();
  await expect(status(page)).toHaveText(en["hub.sheet.done.started"]);
  expect(hub.sent[0].payload).toEqual({
    kind: "session.start",
    device_id: hub.devices[0].device_id,
    mode: "postpaid",
    player_id: "P1",
  });
});

test("a guest sees no start options", async ({ page }) => {
  await open(page);
  await openSheet(page);
  await act(page, "Guest").click();
  await expect(options(page)).toHaveCount(0);
});

for (const [code, text] of [
  ["PACK_EXPIRED", en["hub.session.pack_expired"]],
  ["PLAN_NOT_FOR_GROUP", en["hub.session.pack_group"]],
] as const) {
  test(`${code} is refused inline with the reason`, async ({ page }) => {
    await open(page, { fail: { status: 409, code } });
    await openSheet(page);
    await pickPlayer(page);
    await page.getByTestId("start-option-pack").nth(0).click();
    await act(page, en["hub.session.start_pack"]).click();
    await expect(status(page)).toContainText(text);
  });
}

test("a wallet below the first bill shows the balance message from the Hub", async ({ page }) => {
  await open(page, {
    fail: {
      status: 409,
      code: "INSUFFICIENT_BALANCE",
      message: "Balance ₹10.00. Top up at counter. Rate ₹100.00 an hour, minimum ₹50.00 (30 min).",
    },
  });
  await openSheet(page);
  await pickPlayer(page);
  await page.getByTestId("start-option-wallet").click();
  await act(page, en["hub.session.start_wallet"]).click();
  await expect(status(page)).toContainText("Balance ₹10.00. Top up at counter.");
});

test("a picked player starts on a pass and sends the owned pass id", async ({ page }) => {
  const hub = await open(page);
  await openSheet(page);
  await pickPlayer(page);
  await page.getByTestId("start-option-pass").click();
  await act(page, en["hub.session.start_pass"]).click();
  await expect(status(page)).toHaveText(en["hub.sheet.done.started"]);
  expect(hub.sent[0].payload).toEqual({
    kind: "session.start",
    device_id: hub.devices[0].device_id,
    mode: "pass",
    player_id: "P1",
    pack_id: "PK4",
  });
});

test("a guest can start a block for this group and the Hub gets the plan id", async ({ page }) => {
  const hub = await open(page, { plans: [BLOCK, { ...BLOCK, id: "BL2", group_ids: ["G2"] }] });
  await openSheet(page);
  await act(page, "Guest").click();
  await expect(options(page)).toBeVisible();
  const blocks = page.getByTestId("start-option-block");
  await expect(blocks).toHaveCount(1);
  await expect(blocks.nth(0)).toContainText("Night block");
  await expect(blocks.nth(0)).toContainText("₹200.00");
  await expect(blocks.nth(0)).toContainText("22:00–06:00");
  await expect(page.getByTestId("start-option-wallet")).toHaveCount(0);
  await blocks.nth(0).click();
  await act(page, en["hub.session.start_block"]).click();
  await expect(status(page)).toHaveText(en["hub.sheet.done.started"]);
  expect(hub.sent[0].payload).toEqual({
    kind: "session.start",
    device_id: hub.devices[0].device_id,
    mode: "block",
    plan_id: "BL1",
  });
});

test("a start outside the window is refused inline with the window from the Hub", async ({
  page,
}) => {
  await open(page, {
    plans: [BLOCK],
    fail: {
      status: 409,
      code: "PASS_OUTSIDE_WINDOW",
      message: "This block starts 22:00–06:00.",
    },
  });
  await openSheet(page);
  await act(page, "Guest").click();
  await page.getByTestId("start-option-block").click();
  await act(page, en["hub.session.start_block"]).click();
  await expect(status(page)).toContainText("This block starts 22:00–06:00.");
});
