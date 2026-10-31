import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type { KitchenTicketView, SnapshotAlert, SnapshotBill } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { CORS, HUB, MANAGER, device, json, snapshot } from "./floor-fixture";

// Hub alerts (Story 3.25): the snapshot carries the open alerts; the tile shows an `alert-badge`
// tag, the zone row and the header a count, a bundled sound plays at once and every 30 s, and
// opening the tile sheet (or its row in the header list) acknowledges. The Hub's loopback API is
// stubbed with `page.route`; a queued `devices.changed` event makes the view reload.

const PC1 = device(1, {}).device_id;
const PC2 = device(2, {}).device_id;
const MIN = 60_000;

type Stub = {
  alerts: SnapshotAlert[];
  bills: SnapshotBill[];
  licence: boolean;
  /** The PC an Android Hub is asked to move the branch to (Story 10.16). */
  move: string | null;
  acks: string[];
  queue: string[];
  locks: number;
};

const raised = (id: string, device_id: string, extra: Partial<SnapshotAlert> = {}) =>
  ({ id, kind: "fixture", device_id, raised_at: Date.now() - 2 * MIN, ...extra }) as SnapshotAlert;

async function open(page: Page, alerts: SnapshotAlert[] = []): Promise<Stub> {
  const hub: Stub = {
    alerts,
    bills: [],
    licence: false,
    move: null,
    acks: [],
    queue: [],
    locks: 0,
  };
  await page.addInitScript(() => {
    sessionStorage.setItem("arena.hub.token", "manager-token");
    const w = window as unknown as { __plays: string[] };
    w.__plays = [];
    HTMLMediaElement.prototype.play = function (this: HTMLMediaElement) {
      w.__plays.push(this.src);
      return Promise.resolve();
    };
  });
  await page.clock.install();
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot": {
        const snap = snapshot([device(1, {}), device(2, {}), device(3, {})]);
        snap.alerts = hub.alerts;
        snap.bills = hub.bills;
        if (hub.licence) snap.licence_unverified = true;
        if (hub.move) snap.move_notice = { id: "M1", target_hub_name: hub.move };
        return json(route, snap);
      }
      case "/ui/me":
        return json(route, { ...MANAGER, locked: false, has_pin: true });
      case "/ui/settings":
        return json(route, []);
      case "/ui/lock":
        hub.locks++;
        return route.fulfill({ status: 204, headers: CORS });
      case "/ui/unlock":
        return json(route, { staff: MANAGER, switched: false });
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: `retry: 300\n\n` + hub.queue.splice(0).join(""),
        });
      case "/ui/intent": {
        const body = req.postDataJSON() as {
          intent: { payload: { kind: string; alert_id: string } };
        };
        expect(body.intent.payload.kind).toBe("alert.ack");
        const id = body.intent.payload.alert_id;
        hub.acks.push(id);
        hub.alerts = hub.alerts.filter((a) => a.id !== id);
        return json(route, {
          applied: {
            intent_id: "I",
            changes: [{ epoch: "1:H", seq: 9, tbl: "alerts", row_id: id, op: "update" }],
          },
        });
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/floor");
  await expect(page.locator("[data-component=floor-tile]").first()).toBeVisible();
  return hub;
}

const raise = (hub: Stub, alert: SnapshotAlert) => {
  hub.alerts = [...hub.alerts, alert];
  hub.queue.push("event: devices.changed\ndata: {}\n\n");
};

const tile = (page: Page, label: string) =>
  page.locator("[data-component=floor-tile]", { hasText: label });
const plays = (page: Page) =>
  page.evaluate(() => (window as unknown as { __plays: string[] }).__plays);
const headerButton = (page: Page) => page.getByTestId("hub-alerts");

async function noSeriousAxe(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  expect(
    results.violations.filter((v) => v.impact === "serious" || v.impact === "critical"),
  ).toEqual([]);
}

test("a raised alert tags the tile, counts in the header and sounds now and every 30 s", async ({
  page,
}) => {
  const hub = await open(page);
  await expect(headerButton(page)).toHaveCount(0);
  expect(await plays(page)).toEqual([]);
  raise(hub, raised("A1", PC1));
  const badge = tile(page, "PC 1").locator("[data-component=alert-badge]");
  await expect(badge).toHaveText(en["ui.alert.tag.new"]);
  await expect(tile(page, "PC 2").locator("[data-component=alert-badge]")).toHaveCount(0);
  await expect(tile(page, "PC 1").locator("[data-tile-face]")).toHaveAttribute(
    "aria-label",
    new RegExp(en["ui.alert.tag.new"]),
  );
  await expect(headerButton(page)).toContainText("1");
  await expect.poll(async () => (await plays(page)).length).toBe(1);
  expect((await plays(page))[0]).toMatch(/\/sounds\/alert\.wav$/);
  await page.clock.runFor(30_000);
  await expect.poll(async () => (await plays(page)).length).toBe(2);
  await page.clock.runFor(30_000);
  await expect.poll(async () => (await plays(page)).length).toBe(3);
  await noSeriousAxe(page);
});

test("a kiosk login lockout tags the tile LOCKED OUT, sounds and names the alert in the list", async ({
  page,
}) => {
  const hub = await open(page);
  raise(hub, raised("L1", PC1, { kind: "login_lockout" }));
  await expect(tile(page, "PC 1").locator("[data-component=alert-badge]")).toHaveText(
    en["ui.alert.tag.locked_out"],
  );
  await expect.poll(async () => (await plays(page)).length).toBe(1);
  await headerButton(page).click();
  await expect(page.getByTestId("alert-row")).toContainText(en["hub.alerts.kind.login_lockout"]);
});

test("a staff call tags the tile CALL, sounds and names the alert in the list", async ({
  page,
}) => {
  const hub = await open(page);
  raise(hub, raised("C1", PC1, { kind: "call_staff" }));
  await expect(tile(page, "PC 1").locator("[data-component=alert-badge]")).toHaveText(
    en["ui.alert.tag.call"],
  );
  await expect.poll(async () => (await plays(page)).length).toBe(1);
  await headerButton(page).click();
  await expect(page.getByTestId("alert-row")).toContainText(en["hub.alerts.kind.call_staff"]);
});

test("a kiosk order tags the tile ORDER, sounds and names the alert in the list", async ({
  page,
}) => {
  const hub = await open(page);
  raise(hub, raised("O1", PC1, { kind: "kiosk_order" }));
  await expect(tile(page, "PC 1").locator("[data-component=alert-badge]")).toHaveText(
    en["ui.alert.tag.order"],
  );
  await expect.poll(async () => (await plays(page)).length).toBe(1);
  await headerButton(page).click();
  await expect(page.getByTestId("alert-row")).toContainText(en["hub.alerts.kind.kiosk_order"]);
});

/** A running bill on PC 1 whose kitchen ticket K-5 is at `stage`. */
const ticketBill = (...stages: string[]): SnapshotBill => ({
  session_id: "S1",
  device_id: PC1,
  session_rev: 1,
  lines: [],
  items: [],
  tickets: stages.map((stage, i): KitchenTicketView => ({
    id: `T-${i}`,
    order_id: "O1",
    number: 5 + i,
    seat: "PC 1",
    stage,
    placed_at: Date.now() - MIN,
    rev: 2,
    lines: [{ line_id: `L-${i}`, label: "Burger", qty: 1, notes: [] }],
  })),
  subtotal_minor: 0,
  paid_minor: 0,
  remaining_minor: 0,
});

const badge = (page: Page, label: string) =>
  tile(page, label).locator("[data-component=alert-badge]");

test("a ticket reaching Ready turns the tile READY, chimes once and is announced once", async ({
  page,
}) => {
  const hub = await open(page);
  hub.bills = [ticketBill("preparing")];
  hub.queue.push("event: devices.changed\ndata: {}\n\n");
  await expect(badge(page, "PC 1")).toHaveText(en["ui.alert.tag.order"]);
  expect(await plays(page)).toEqual([]);

  hub.bills = [ticketBill("ready")];
  raise(hub, raised("R1", PC1, { kind: "order_ready" }));
  await expect(badge(page, "PC 1")).toHaveText(en["ui.alert.tag.ready"]);
  await expect.poll(async () => (await plays(page)).length).toBe(1);
  expect((await plays(page))[0]).toMatch(/\/sounds\/order-ready\.wav$/);
  await expect(page.getByTestId("alert-announce").locator("p")).toHaveText(
    en["hub.alerts.announce"]
      .replace("{where}", "PC 1")
      .replace("{what}", en["hub.alerts.kind.order_ready"]),
  );
  // It never repeats.
  await page.clock.runFor(60_000);
  expect(await plays(page)).toHaveLength(1);
  await expect(page.getByTestId("alert-announce").locator("p")).toHaveCount(1);
});

test("the READY tag follows the ticket stages: ORDER while another is cooking, none when all close", async ({
  page,
}) => {
  const hub = await open(page);
  hub.bills = [ticketBill("ready", "new")];
  raise(hub, raised("R1", PC1, { kind: "order_ready" }));
  await expect(badge(page, "PC 1")).toHaveText(en["ui.alert.tag.ready"]);

  // The ready one is served (its alert acknowledged): the other is still New.
  hub.alerts = [];
  hub.bills = [ticketBill("served", "new")];
  hub.queue.push("event: devices.changed\ndata: {}\n\n");
  await expect(badge(page, "PC 1")).toHaveText(en["ui.alert.tag.order"]);

  // An undone Ready returns the tile to ORDER and makes no sound.
  hub.bills = [ticketBill("preparing")];
  hub.queue.push("event: devices.changed\ndata: {}\n\n");
  await expect(badge(page, "PC 1")).toHaveText(en["ui.alert.tag.order"]);

  hub.bills = [];
  hub.queue.push("event: devices.changed\ndata: {}\n\n");
  await expect(badge(page, "PC 1")).toHaveCount(0);
  // Only the Ready itself chimed: the served ticket and the undone Ready made no sound.
  expect(await plays(page)).toHaveLength(1);
});

test("a Free branch's ready order (no ticket, no alert) keeps the plain tile", async ({ page }) => {
  const hub = await open(page);
  hub.bills = [{ ...ticketBill(), tickets: [], ready: true }];
  hub.queue.push("event: devices.changed\ndata: {}\n\n");
  await page.waitForTimeout(800);
  await expect(badge(page, "PC 1")).toHaveCount(0);
  expect(await plays(page)).toEqual([]);
});

test("a screen reader hears each new alert once", async ({ page }) => {
  const hub = await open(page);
  const region = page.getByTestId("alert-announce");
  await expect(region).toHaveAttribute("aria-live", "polite");
  await expect(region).toHaveText("");
  raise(hub, raised("A1", PC1));
  await expect(region).toHaveText(
    en["hub.alerts.announce"]
      .replace("{where}", "PC 1")
      .replace("{what}", en["hub.alerts.kind.fixture"]),
  );
  await page.waitForTimeout(800);
  await expect(region.locator("p")).toHaveCount(1);
  raise(hub, raised("A2", PC2, { kind: "time_up" }));
  await expect(region.locator("p")).toHaveCount(2);
  await expect(region.locator("p").last()).toHaveText(
    en["hub.alerts.announce"]
      .replace("{where}", "PC 2")
      .replace("{what}", en["hub.alerts.kind.time_up"]),
  );
  await expect(tile(page, "PC 2").locator("[data-component=alert-badge]")).toHaveText(
    en["ui.alert.tag.time_up"],
  );
});

test("opening the tile sheet acknowledges its alerts and silences the sound", async ({ page }) => {
  const hub = await open(page, [raised("A1", PC1), raised("A2", PC1, { kind: "time_up" })]);
  await expect(headerButton(page)).toContainText("2");
  await expect.poll(async () => (await plays(page)).length).toBe(1);
  await tile(page, "PC 1").locator("[data-tile-face]").click();
  await expect(page.locator("[data-component=tile-sheet]")).toBeVisible();
  await expect.poll(() => hub.acks.sort()).toEqual(["A1", "A2"]);
  await expect(tile(page, "PC 1").locator("[data-component=alert-badge]")).toHaveCount(0);
  await expect(headerButton(page)).toHaveCount(0);
  await page.clock.runFor(90_000);
  expect((await plays(page)).length).toBe(1);
});

test("the header list shows device, kind and age, and a row opens the sheet", async ({ page }) => {
  const hub = await open(page, [raised("A1", PC1), raised("A2", PC2, { kind: "time_up" })]);
  await headerButton(page).click();
  const rows = page.getByTestId("alert-row");
  await expect(rows).toHaveCount(2);
  await expect(rows.first()).toContainText("PC 1");
  await expect(rows.first()).toContainText(en["hub.alerts.kind.fixture"]);
  await expect(rows.first()).toContainText(en["hub.alerts.age_minutes"].replace("{count}", "2"));
  await expect(rows.last()).toContainText("PC 2");
  await expect(rows.last()).toContainText(en["hub.alerts.kind.time_up"]);
  await noSeriousAxe(page);
  await rows.last().getByRole("button").click();
  const sheet = page.locator("[data-component=tile-sheet]");
  await expect(sheet).toBeVisible();
  await expect(sheet).toContainText("PC 2");
  await expect(page.getByTestId("alert-list")).toHaveCount(0);
  await expect.poll(() => hub.acks).toContain("A2");
  await expect(tile(page, "PC 2").locator("[data-component=alert-badge]")).toHaveCount(0);
  await expect(tile(page, "PC 1").locator("[data-component=alert-badge]")).toHaveText(
    en["ui.alert.tag.new"],
  );
});

test("Escape closes the list and returns focus to the header button", async ({ page }) => {
  await open(page, [raised("A1", PC1)]);
  await headerButton(page).click();
  await expect(page.getByTestId("alert-list")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("alert-list")).toHaveCount(0);
  await expect(headerButton(page)).toBeFocused();
});

test("a collapsed zone shows its alert count and follows raise and clear", async ({ page }) => {
  const hub = await open(page);
  await page.getByRole("button", { name: "Main hall" }).click();
  await expect(page.getByTestId("zone-device")).toHaveCount(0);
  const zone = page.getByTestId("zone-alerts");
  await expect(zone).toHaveCount(0);
  raise(hub, raised("A1", PC1));
  await expect(zone).toContainText("1");
  raise(hub, raised("A2", PC2));
  await expect(zone).toContainText("2");
  await expect(page.getByTestId("zone-device")).toHaveCount(0);
  await headerButton(page).click();
  await page.getByTestId("alert-row").first().getByRole("button").click();
  await expect(zone).toContainText("1");
  await expect(page.getByTestId("zone-device")).toHaveCount(0);
});

test("open alerts survive a reload and the idle lock", async ({ page }) => {
  const hub = await open(page, [raised("A1", PC1)]);
  await expect(tile(page, "PC 1").locator("[data-component=alert-badge]")).toBeVisible();
  await page.reload();
  await expect(tile(page, "PC 1").locator("[data-component=alert-badge]")).toHaveText(
    en["ui.alert.tag.new"],
  );
  await expect(headerButton(page)).toContainText("1");

  const before = (await plays(page)).length;
  await page.clock.runFor(5 * MIN);
  const lock = page.getByRole("dialog", { name: en["hub.lock.title"] });
  await expect(lock).toBeVisible();
  expect(hub.locks).toBeGreaterThan(0);
  await page.clock.runFor(MIN);
  expect((await plays(page)).length).toBeGreaterThan(before + 8);
  await page.keyboard.type("1234");
  await page.keyboard.press("Enter");
  await expect(lock).toHaveCount(0);
  await expect(tile(page, "PC 1").locator("[data-component=alert-badge]")).toHaveText(
    en["ui.alert.tag.new"],
  );
  await expect(headerButton(page)).toContainText("1");
});

test("licence trouble is a branch row with no sound, and it clears with the condition", async ({
  page,
}) => {
  const hub = await open(page);
  hub.licence = true;
  hub.queue.push("event: devices.changed\ndata: {}\n\n");
  await expect(headerButton(page)).toContainText("1");
  await headerButton(page).click();
  const row = page.getByTestId("alert-row");
  await expect(row).toContainText(en["hub.alerts.branch"]);
  await expect(row).toContainText(en["hub.licence.unverified"]);
  await expect(row.getByRole("button")).toHaveCount(0);
  await page.clock.runFor(MIN);
  expect(await plays(page)).toEqual([]);
  await expect(page.getByTestId("hub-status-slot")).toHaveCount(0);
  hub.licence = false;
  hub.queue.push("event: devices.changed\ndata: {}\n\n");
  await expect(headerButton(page)).toHaveCount(0);
});

const DEVICES_CHANGED = "event: devices.changed\ndata: {}\n\n";

test("a Hub move is a persistent branch row naming the PC, with no sound, until the cloud stops asking", async ({
  page,
}) => {
  const hub = await open(page);
  hub.move = "Front desk";
  hub.queue.push(DEVICES_CHANGED);
  await expect(headerButton(page)).toContainText("1");
  await headerButton(page).click();
  const row = page.getByTestId("alert-row");
  await expect(row).toContainText(en["hub.alerts.branch"]);
  await expect(row).toContainText("Move this branch to Front desk");
  await expect(row.getByRole("button")).toHaveCount(0);
  await page.clock.runFor(MIN);
  expect(await plays(page)).toEqual([]);
  hub.move = null;
  hub.queue.push(DEVICES_CHANGED);
  await expect(headerButton(page)).toHaveCount(0);
});

test("a Printer offline alert is a branch row with no tile tag and no sound, and a click acknowledges it", async ({
  page,
}) => {
  const hub = await open(page, [
    { id: "P1", kind: "printer_offline", device_id: null, raised_at: Date.now() - MIN },
  ]);
  await expect(headerButton(page)).toBeVisible();
  await headerButton(page).click();
  const row = page.getByTestId("alert-row");
  await expect(row).toHaveCount(1);
  await expect(row).toContainText(en["hub.alerts.branch"]);
  await expect(row).toContainText(en["hub.alerts.kind.printer_offline"]);
  await expect(tile(page, "PC 1").locator("[data-component=alert-badge]")).toHaveCount(0);
  expect(await plays(page)).toHaveLength(0);
  await row.getByRole("button").click();
  await expect.poll(() => hub.acks).toEqual(["P1"]);
  await expect(page.getByTestId("alert-list")).toHaveCount(0);
});
