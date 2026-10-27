import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type {
  PlayerHit,
  SessionListRow,
  SessionStaff,
  SnapshotBill,
  SnapshotDevice,
  SnapshotSession,
} from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { CORS, HUB, device, json, snapshot } from "./floor-fixture";

const COUNTER: SessionStaff = {
  staff_id: "01JCOUNTER0000000000000000",
  display_name: "Counter",
  role_name: "Counter",
  permissions: [
    "pos.sell",
    "player.view",
    "player.manage",
    "wallet.topup",
    "session.start",
    "session.end",
    "session.pause",
  ],
};

const ASHA: PlayerHit = {
  player_id: "P1",
  name: "Asha Rao",
  username: "asha_9",
  phone: "+919876543210",
  tags: [],
};

type Sent = { kind: string; session_id?: string; player_id?: string };

type Hub = {
  sent: { payload: Sent; rev?: number }[];
  converted: { session_id: string; base_rev: number; form: { username: string } }[];
  devices: SnapshotDevice[];
  sessions: SnapshotSession[];
  bills: SnapshotBill[];
  rows: SessionListRow[];
};

const DEVICE = device(1, { status: "in_session" });
const FREE = device(2, { status: "online" });

const running = (extra: Partial<SnapshotSession> = {}): SnapshotSession => ({
  id: "S1",
  device_id: DEVICE.device_id,
  state: "active",
  started_at: Date.now() - 600_000,
  rev: 3,
  guest_no: 2,
  paused_total_ms: 0,
  ...extra,
});

const unpaid = (extra: Partial<SnapshotBill> = {}): SnapshotBill => ({
  session_id: "S9",
  device_id: FREE.device_id,
  order_id: "O9",
  order_rev: 4,
  session_rev: 6,
  guest: true,
  lines: [],
  subtotal_minor: 7500,
  paid_minor: 0,
  remaining_minor: 7500,
  ...extra,
});

const row = (extra: Partial<SessionListRow>): SessionListRow => ({
  id: "S1",
  device_id: DEVICE.device_id,
  device: "PC 1",
  guest_no: 2,
  mode: "postpaid",
  started_at: 1_800_000_000_000,
  state: "running",
  ...extra,
});

const newHub = (extra: Partial<Hub> = {}): Hub => ({
  sent: [],
  converted: [],
  devices: [DEVICE, FREE],
  sessions: [running()],
  bills: [],
  rows: [],
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
        snap.sessions = hub.sessions;
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
      case "/ui/sessions":
        return json(route, { business_day: "2026-10-27", sessions: hub.rows });
      case "/ui/sessions/convert": {
        const body = req.postDataJSON() as Hub["converted"][number];
        hub.converted.push(body);
        hub.sessions = hub.sessions.map((s) => ({
          ...s,
          player_id: "P1",
          username: body.form.username.toLowerCase(),
        }));
        hub.bills = hub.bills.map((b) => ({ ...b, guest: undefined, wallet_minor: 0 }));
        return json(route, { applied: { intent_id: "I", changes: [] } });
      }
      case "/ui/intent": {
        const body = req.postDataJSON() as { intent: { payload: Sent; base_rev?: number } };
        hub.sent.push({ payload: body.intent.payload, rev: body.intent.base_rev });
        hub.bills = hub.bills.map((b) => ({ ...b, guest: undefined, wallet_minor: 0 }));
        hub.sessions = hub.sessions.map((s) => ({
          ...s,
          player_id: "P1",
          username: ASHA.username,
        }));
        return json(route, { applied: { intent_id: "I", changes: [] } });
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
}

async function openSheet(page: Page, hub: Hub, name: string) {
  await serve(page, hub);
  await page.goto("/floor");
  await page
    .locator("[data-component=floor-tile]", { hasText: name })
    .locator("[data-tile-face]")
    .focus();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("tile-sheet")).toBeVisible();
}

const bar = (page: Page) => page.getByTestId("settle-bar");
const serious = (violations: { impact?: string | null }[]) =>
  violations.filter((v) => v.impact === "serious" || v.impact === "critical");

test("a running guest session converts to a new profile in the sheet", async ({ page }) => {
  const hub = newHub();
  await openSheet(page, hub, "PC 1");
  const convert = page.getByTestId("session-bill").getByTestId("convert-guest");
  await convert.getByRole("button", { name: en["hub.session.convert"] }).click();
  await expect(page.getByTestId("player-form")).toBeVisible();

  await page.getByLabel(en["hub.players.username"]).fill("Asha_9");
  await page.getByLabel(en["hub.players.password"]).fill("secret1");
  await page.getByLabel(en["hub.players.phone"]).fill("+919876543210");
  await page.getByLabel(en["hub.players.name"], { exact: true }).fill("Asha Rao");
  await page.getByLabel(en["player.signup.consent"]).check();
  await page.getByRole("button", { name: en["hub.players.submit"] }).click();

  await expect.poll(() => hub.converted.length).toBe(1);
  expect(hub.converted[0]).toMatchObject({ session_id: "S1", base_rev: 3 });
  expect(hub.converted[0].form.username).toBe("Asha_9");
  await expect(page.getByTestId("convert-done")).toHaveText(
    en["hub.session.converted"].replace("{name}", "asha_9"),
  );
  await expect(page.getByTestId("session-line")).toHaveText("asha_9");
  await expect(page.getByTestId("session-bill").getByTestId("convert-guest")).toHaveCount(0);
  expect(serious((await new AxeBuilder({ page }).analyze()).violations)).toEqual([]);
});

test("an unpaid guest bill links to an existing player and gains the wallet method", async ({
  page,
}) => {
  const hub = newHub({ devices: [FREE], sessions: [], bills: [unpaid()] });
  await openSheet(page, hub, "PC 2");
  await expect(bar(page).locator("[data-method]")).toHaveText(["Cash", "UPI", "Card"]);

  await bar(page).getByRole("button", { name: en["hub.session.link"] }).click();
  await page.getByLabel(en["hub.players.lookup.label"]).fill("98765");
  await page.getByTestId("player-hits").getByText("Asha Rao").click();

  await expect.poll(() => hub.sent.length).toBe(1);
  expect(hub.sent[0]).toMatchObject({
    payload: { kind: "session.assign_player", session_id: "S9", player_id: "P1" },
    rev: 6,
  });
  await expect(bar(page).locator("[data-method]")).toHaveText(["Cash", "UPI", "Card", "Wallet"]);
  await expect(page.getByTestId("convert-done")).toContainText("Asha Rao");
  await expect(bar(page).getByTestId("convert-guest")).toHaveCount(0);
});

test("a profiled bill offers no conversion", async ({ page }) => {
  const hub = newHub({
    devices: [FREE],
    sessions: [],
    bills: [unpaid({ guest: undefined, wallet_minor: 0 })],
  });
  await openSheet(page, hub, "PC 2");
  await expect(bar(page)).toBeVisible();
  await expect(page.getByTestId("convert-guest")).toHaveCount(0);
});

test("the Sessions screen lists today's sessions, filters by state and opens an unpaid bill", async ({
  page,
}) => {
  const hub = newHub({
    devices: [DEVICE, FREE],
    bills: [unpaid()],
    rows: [
      row({
        id: "S9",
        device_id: FREE.device_id,
        device: "PC 2",
        guest_no: 1,
        state: "unpaid",
        ended_at: 1_800_003_600_000,
        bill_minor: 7500,
        order_id: "O9",
      }),
      row({ id: "S1", username: "asha_9", guest_no: 2 }),
      row({
        id: "S0",
        device_id: "D3",
        device: "PC 3",
        guest_no: 3,
        state: "settled",
        ended_at: 1_800_001_000_000,
        bill_minor: 3000,
      }),
    ],
  });
  await serve(page, hub);
  await page.goto("/sessions");
  const rows = page.getByTestId("session-row");
  await expect(rows).toHaveCount(3);
  await expect(page.getByTestId("sessions-day")).toContainText("2026-10-27");
  await expect(rows.nth(0).getByTestId("session-player")).toHaveText("Guest 1");
  await expect(rows.nth(0).getByTestId("session-state")).toContainText(
    en["hub.sessions.state.unpaid"],
  );
  await expect(rows.nth(1).getByTestId("session-player")).toHaveText("asha_9");
  await expect(rows.nth(1).getByTestId("session-state")).toContainText(
    en["hub.sessions.state.running"],
  );
  await expect(rows.nth(2)).toContainText("₹30.00");
  expect(serious((await new AxeBuilder({ page }).analyze()).violations)).toEqual([]);

  await page.getByLabel(en["hub.sessions.filter.label"]).selectOption("settled");
  await expect(rows).toHaveCount(1);
  await expect(rows.first()).toHaveAttribute("data-state", "settled");
  await page.getByLabel(en["hub.sessions.filter.label"]).selectOption("unpaid");
  await expect(rows).toHaveCount(1);

  await rows.first().getByRole("button", { name: en["hub.sessions.open"] }).click();
  await expect(page).toHaveURL(/\/floor\?device=/);
  await expect(page.getByTestId("tile-sheet")).toBeVisible();
  await expect(bar(page)).toBeVisible();
});
