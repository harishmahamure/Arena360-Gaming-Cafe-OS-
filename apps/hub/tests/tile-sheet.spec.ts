import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type TestInfo } from "@playwright/test";
import type {
  SessionStaff,
  SnapshotBill,
  SnapshotDevice,
  SnapshotSession,
} from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { CORS, HUB, device, json, snapshot } from "./floor-fixture";

const STAFF: Record<string, SessionStaff> = {
  "manager-token": {
    staff_id: "01JMANAGER0000000000000000",
    display_name: "Manager",
    role_name: "Manager",
    permissions: ["device.remote", "device.manage", "device.screen"],
  },
  "counter-token": {
    staff_id: "01JCOUNTER0000000000000000",
    display_name: "Counter",
    role_name: "Counter",
    permissions: ["device.screen", "session.start"],
  },
  "seller-token": {
    staff_id: "01JSELLER00000000000000000",
    display_name: "Seller",
    role_name: "Counter",
    permissions: ["device.screen", "session.start", "pos.sell"],
  },
};

type Sent = {
  payload: Record<string, unknown> & { kind: string };
  key: string;
  base_rev?: number;
  reason?: string;
  ticket?: string;
};
type Hub = {
  devices: SnapshotDevice[];
  sessions: SnapshotSession[];
  bills?: SnapshotBill[];
  sent: Sent[];
  elevations: { items: unknown[]; pin: string }[];
  ack: boolean;
  online: boolean;
  hc: boolean;
};

const PIN = "2222";
const ONLINE = device(1, { status: "online" });
const sessions = (): SnapshotSession[] => [
  {
    id: "S2",
    device_id: device(2, {}).device_id,
    state: "active",
    started_at: Date.now() - 3_725_000,
    paused_total_ms: 0,
    rev: 1,
    player_id: "P1",
    username: "asha",
  },
];
const devices = (): SnapshotDevice[] => [
  ONLINE,
  device(2, { status: "in_session", floor_x: 1 }),
  device(3, { status: "offline", online: false, last_seen_at: Date.UTC(2026, 9, 25, 9, 30) }),
  device(4, { status: "maintenance", online: true, maintenance_note: "fan", meta_rev: 7 }),
  device(5, { group_id: null, zone: null, status: "offline", online: false, meta_rev: 3 }),
  device(6, { kind: "console", group_id: "G2", zone: null, tv_number: 2 }),
  device(7, { kind: "vr", group_id: null, zone: null }),
];

async function open(page: Page, token: string, over: Partial<Hub> = {}): Promise<Hub> {
  const hub: Hub = {
    devices: devices(),
    sessions: sessions(),
    sent: [],
    elevations: [],
    ack: true,
    online: true,
    hc: false,
    ...over,
  };
  await page.addInitScript(
    ([t]) => {
      sessionStorage.setItem("arena.hub.token", t);
      const sources: Array<{ dispatch: (name: string, data: unknown) => void }> = [];
      class FakeEventSource {
        static CLOSED = 2;
        readyState = 1;
        listeners: Record<string, Array<(e: { data: string }) => void>> = {};
        constructor() {
          sources.push(this);
        }
        addEventListener(name: string, fn: (e: { data: string }) => void) {
          (this.listeners[name] ??= []).push(fn);
        }
        close() {}
        dispatch(name: string, data: unknown) {
          for (const fn of this.listeners[name] ?? []) fn({ data: JSON.stringify(data) });
        }
      }
      (window as unknown as Record<string, unknown>).EventSource = FakeEventSource;
      (window as unknown as Record<string, unknown>).__emit = (name: string, data: unknown) =>
        sources.forEach((s) => s.dispatch(name, data));
    },
    [token],
  );
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot": {
        const snap = snapshot(hub.devices.map((d) => ({ ...d, online: hub.online && d.online })));
        snap.groups.push({
          id: "G2",
          name: "Consoles",
          kind: "console",
          colour: "orange",
          sort_order: 1,
          rev: 1,
          rate_minor: null,
        });
        snap.groups.push({
          id: "G3",
          name: "Studio",
          kind: "pc",
          colour: "lime",
          sort_order: 2,
          rev: 1,
          rate_minor: 5000,
        });
        snap.sessions = hub.sessions;
        snap.bills = hub.bills ?? [];
        snap.ui = { idle_lock_minutes: 5, hc: hub.hc };
        return json(route, snap);
      }
      case "/ui/me":
        return json(route, STAFF[token]);
      case "/ui/settings":
        return json(route, []);
      case "/ui/elevate": {
        const body = req.postDataJSON() as { items: unknown[]; pin: string };
        hub.elevations.push(body);
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
          intent: {
            payload: Sent["payload"];
            idempotency_key: string;
            base_rev?: number;
            reason?: string;
          };
          elevation_ticket?: string;
        };
        const sent: Sent = {
          payload: body.intent.payload,
          key: body.intent.idempotency_key,
          base_rev: body.intent.base_rev,
          reason: body.intent.reason,
          ticket: body.elevation_ticket,
        };
        hub.sent.push(sent);
        const kind = sent.payload.kind;
        const exit = kind === "device.admin_unlock";
        await json(route, {
          applied: {
            intent_id: "I",
            changes: [
              exit
                ? { epoch: "1:H", seq: 9, tbl: "device_admin_exits", row_id: "EXIT1", op: "insert" }
                : { epoch: "1:H", seq: 9, tbl: "device_events", row_id: "E1", op: "insert" },
            ],
          },
        });
        if (hub.ack && (kind === "device.screen" || kind === "device.power")) {
          await page.evaluate(
            ([key, id]) =>
              (window as unknown as { __emit: (n: string, d: unknown) => void }).__emit(
                "device.pc_ack",
                { cmd_id: key, device_id: id },
              ),
            [sent.key, String(sent.payload.device_id)],
          );
        }
        if (hub.ack && exit) {
          await page.evaluate(() =>
            (window as unknown as { __emit: (n: string, d: unknown) => void }).__emit(
              "device.admin_unlock_ack",
              { exit_id: "EXIT1", device_id: "x" },
            ),
          );
        }
        return;
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
const face = (page: Page, label: string) => tile(page, label).locator("[data-tile-face]");
const sheet = (page: Page) => page.getByRole("complementary");
const act = (page: Page, name: string) => sheet(page).getByRole("button", { name, exact: true });
const status = (page: Page) => page.getByTestId("unlock-status");
const clean = async (page: Page) =>
  expect(
    (await new AxeBuilder({ page }).analyze()).violations.filter(
      (v) => v.impact === "serious" || v.impact === "critical",
    ),
  ).toEqual([]);

test("a tap or Enter opens a non-modal sheet with focus inside, and Esc returns focus to the tile", async ({
  page,
}) => {
  await open(page, "counter-token");
  await expect(sheet(page)).toHaveCount(0);
  await tile(page, "PC 1").click({ position: { x: 20, y: 20 } });
  await expect(sheet(page)).toBeVisible();
  await expect(sheet(page)).toHaveAccessibleName("PC 1");
  await expect(sheet(page).getByRole("heading", { name: "PC 1" })).toBeFocused();
  await expect(page.locator("[aria-modal=true], dialog, [role=dialog]")).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(sheet(page)).toHaveCount(0);
  await expect(face(page, "PC 1")).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(sheet(page)).toBeVisible();
  await expect(sheet(page).getByRole("heading", { name: "PC 1" })).toBeFocused();
});

test("the floor stays live and operable beside the sheet: another tile swaps it", async ({
  page,
}, info: TestInfo) => {
  // On a phone the sheet covers an inert floor (touch.spec.ts); the swap is the docked sheet's.
  test.skip(info.project.name === "phone", "a phone sheet is full height over the floor");
  await open(page, "counter-token");
  await face(page, "PC 1").focus();
  await page.keyboard.press("Enter");
  await expect(sheet(page)).toHaveAccessibleName("PC 1");
  await tile(page, "PC 2").click({ position: { x: 20, y: 20 } });
  await expect(sheet(page)).toHaveAccessibleName("PC 2");
  await expect(tile(page, "PC 2")).toHaveAttribute("aria-current", "true");
  await expect(tile(page, "PC 1")).not.toHaveAttribute("aria-current", "true");
});

test("at 1920px the floor reflows to 4 columns while the sheet is open and docks right", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1920, height: 1000 });
  await open(page, "counter-token");
  const cols = (selector: string) =>
    page
      .locator(selector)
      .first()
      .evaluate((el) => getComputedStyle(el).gridTemplateColumns.split(" ").length);
  await expect(tile(page, "PC 1")).toBeVisible();
  expect(await cols("[data-testid=floor-zone] ul")).toBe(6);
  await face(page, "PC 1").focus();
  await page.keyboard.press("Enter");
  await expect(sheet(page)).toBeVisible();
  expect(await cols("[data-testid=floor-zone] ul")).toBe(4);
  const s = await sheet(page).boundingBox();
  const t1 = await tile(page, "PC 1").boundingBox();
  expect((s?.x ?? 0) > (t1?.x ?? 0) + (t1?.width ?? 0)).toBe(true);
  await page.keyboard.press("Escape");
  await expect(sheet(page)).toHaveCount(0);
  expect(await cols("[data-testid=floor-zone] ul")).toBe(6);
});

test("the sheet shows name, group, zone, rate and status", async ({ page }) => {
  await open(page, "counter-token");
  await face(page, "PC 2").focus();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("sheet-group")).toHaveText("Gaming");
  await expect(page.getByTestId("sheet-zone")).toHaveText("Main hall");
  await expect(page.getByTestId("sheet-rate")).toHaveText("₹100.00/hr");
  await expect(page.getByTestId("sheet-status")).toContainText(en["ui.status.in_session"]);
});

test("an in-session PC shows the clock and the player on its tile and offers no start", async ({
  page,
}) => {
  await open(page, "counter-token");
  await expect(tile(page, "PC 2").getByTestId("device-presence")).toHaveText(
    en["ui.status.in_session"],
  );
  await expect(tile(page, "PC 2")).toContainText(/1:02:\d\d/);
  await expect(tile(page, "PC 2").getByTestId("session-line")).toHaveText("asha");
  await face(page, "PC 2").focus();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("session-start")).toHaveCount(0);
  await page.keyboard.press("Escape");
  await face(page, "PC 1").focus();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("session-start")).toBeVisible();
});

test("a free or in-session PC offers every action", async ({ page }) => {
  await open(page, "counter-token");
  for (const label of ["PC 1", "PC 2"]) {
    await face(page, label).focus();
    await page.keyboard.press("Enter");
    for (const name of ["Unlock", "Message", "Lock", "Restart", "Shut down", "Admin unlock"]) {
      await expect(act(page, name)).toBeVisible();
    }
    await expect(act(page, "Return to service")).toHaveCount(0);
    await expect(act(page, "Assign to group")).toHaveCount(0);
  }
});

test("a maintenance PC offers Return to service and Admin unlock only", async ({ page }) => {
  await open(page, "manager-token");
  await face(page, "PC 4").focus();
  await page.keyboard.press("Enter");
  await expect(act(page, "Return to service")).toBeVisible();
  await expect(act(page, "Admin unlock")).toBeVisible();
  for (const name of ["Unlock", "Message", "Lock", "Restart", "Shut down"]) {
    await expect(act(page, name)).toHaveCount(0);
  }
});

test("an offline PC shows its last seen and no actions; a VR rig shows details only", async ({
  page,
}) => {
  await open(page, "manager-token");
  await face(page, "PC 3").focus();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("sheet-offline")).toBeVisible();
  await expect(sheet(page)).toContainText("3:00");
  await expect(sheet(page).getByRole("button")).toHaveCount(1);
  await page.keyboard.press("Escape");
  await face(page, "PC 7").focus();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("sheet-details-only")).toBeVisible();
  await expect(sheet(page).getByRole("button")).toHaveCount(1);
});

test("an unassigned PC offers Assign to group with only groups that fit", async ({ page }) => {
  const hub = await open(page, "manager-token");
  await face(page, "PC 5").focus();
  await page.keyboard.press("Enter");
  const picker = sheet(page).getByRole("combobox", { name: en["hub.sheet.assign_pick"] });
  await expect(picker.locator("option")).toHaveText([
    en["hub.sheet.assign_none"],
    "Gaming",
    "Studio",
  ]);
  await expect(act(page, "Assign to group")).toHaveAttribute("aria-disabled", "true");
  await picker.selectOption("G3");
  await act(page, "Assign to group").click();
  await expect(status(page)).toHaveText(en["hub.sheet.done.saved"]);
  expect(hub.sent).toHaveLength(1);
  expect(hub.sent[0].payload).toEqual({
    kind: "device.assign",
    device_id: hub.devices[4].device_id,
    group_id: "G3",
  });
  expect(hub.sent[0].base_rev).toBe(3);
});

test("a Counter locks and unlocks a PC and sees each ack inline", async ({ page }) => {
  const hub = await open(page, "counter-token");
  await face(page, "PC 1").focus();
  await page.keyboard.press("Enter");
  await act(page, "Lock").click();
  await expect(status(page)).toHaveText(en["hub.sheet.done.lock"]);
  await act(page, "Unlock").click();
  await expect(status(page)).toHaveText(en["hub.sheet.done.unlock"]);
  expect(hub.sent.map((s) => s.payload)).toEqual([
    { kind: "device.screen", device_id: ONLINE.device_id, action: "lock" },
    { kind: "device.screen", device_id: ONLINE.device_id, action: "unlock" },
  ]);
  expect(hub.sent.every((s) => s.ticket === undefined)).toBe(true);
  expect(hub.elevations).toHaveLength(0);
});

test("a message is 1 to 140 characters, in the sheet before it reaches the Hub", async ({
  page,
}) => {
  const hub = await open(page, "counter-token");
  await face(page, "PC 1").focus();
  await page.keyboard.press("Enter");
  const field = sheet(page).getByLabel(en["hub.sheet.message_label"]);
  await act(page, "Message").click();
  await expect(sheet(page)).toContainText(en["hub.sheet.message_invalid"]);
  await field.fill("é".repeat(141));
  await act(page, "Message").click();
  await expect(sheet(page)).toContainText(en["hub.sheet.message_invalid"]);
  expect(hub.sent).toHaveLength(0);
  await field.fill("é".repeat(140));
  await act(page, "Message").click();
  await expect(status(page)).toHaveText(en["hub.sheet.done.message"]);
  expect(hub.sent[0].payload).toEqual({
    kind: "device.screen",
    device_id: ONLINE.device_id,
    action: "message",
    text: "é".repeat(140),
  });
});

test("a Counter's restart opens the manager PIN in place; a wrong PIN clears it, the right one carries its ticket", async ({
  page,
}) => {
  const hub = await open(page, "counter-token");
  await face(page, "PC 1").focus();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("power-needs-approval")).toBeVisible();
  await act(page, "Restart").click();
  await page.keyboard.type("9999");
  await page.keyboard.press("Enter");
  await expect(page.getByText(en["ui.pin.rejected"])).toBeVisible();
  expect(hub.sent).toHaveLength(0);
  await page.keyboard.type(PIN);
  await page.keyboard.press("Enter");
  await expect(status(page)).toHaveText(en["hub.sheet.done.restart"]);
  expect(hub.elevations.at(-1)?.items).toEqual([{ intent_kind: "device.power" }]);
  expect(hub.sent[0].payload).toEqual({
    kind: "device.power",
    device_id: ONLINE.device_id,
    action: "restart",
  });
  expect(hub.sent[0].ticket).toBe("T1");
});

test("a manager shuts a PC down without a PIN", async ({ page }) => {
  const hub = await open(page, "manager-token");
  await face(page, "PC 1").focus();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("power-needs-approval")).toHaveCount(0);
  await act(page, "Shut down").click();
  await expect(status(page)).toHaveText(en["hub.sheet.done.shutdown"]);
  expect(hub.elevations).toHaveLength(0);
});

test("Admin unlock keeps its approval flow, now from the sheet", async ({ page }) => {
  const hub = await open(page, "counter-token");
  await face(page, "PC 4").focus();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("unlock-needs-approval")).toHaveText(
    en["hub.floor.remote_unlock_needs_approval"],
  );
  await act(page, "Admin unlock").click();
  await page.keyboard.type(PIN);
  await page.keyboard.press("Enter");
  await expect(status(page)).toHaveText(en["hub.floor.remote_unlock_done"]);
  expect(hub.elevations.at(-1)?.items).toEqual([{ intent_kind: "device.admin_unlock" }]);
  expect(hub.sent[0].ticket).toBe("T1");
});

test("Return to service needs device.manage: a Counter uses the inline PIN", async ({ page }) => {
  const hub = await open(page, "counter-token");
  await face(page, "PC 4").focus();
  await page.keyboard.press("Enter");
  await act(page, "Return to service").click();
  await page.keyboard.type(PIN);
  await page.keyboard.press("Enter");
  await expect(status(page)).toHaveText(en["hub.sheet.done.saved"]);
  expect(hub.elevations.at(-1)?.items).toEqual([{ intent_kind: "device.maintenance" }]);
  expect(hub.sent[0].payload).toEqual({
    kind: "device.maintenance",
    device_id: hub.devices[3].device_id,
    on: false,
  });
  expect(hub.sent[0].base_rev).toBe(7);
});

test("no ack in 5 s fails the tile, and Retry resends the same idempotency key", async ({
  page,
}) => {
  const hub = await open(page, "counter-token", { ack: false });
  await face(page, "PC 1").focus();
  await page.keyboard.press("Enter");
  await act(page, "Lock").click();
  await expect(status(page)).toHaveText(en["hub.sheet.waiting"]);
  await expect(tile(page, "PC 1").getByTestId("tile-failed")).toContainText(
    en["hub.floor.remote_unlock_failed"],
    { timeout: 8000 },
  );
  await expect(status(page)).toContainText(en["hub.floor.remote_unlock_failed"]);
  hub.ack = true;
  await act(page, en["hub.sheet.retry"]).click();
  await expect(status(page)).toHaveText(en["hub.sheet.done.lock"]);
  await expect(tile(page, "PC 1").getByTestId("tile-failed")).toHaveCount(0);
  expect(hub.sent).toHaveLength(2);
  expect(hub.sent[1].key).toBe(hub.sent[0].key);
});

test("an offline kiosk fails at once without asking the Hub", async ({ page }) => {
  const hub = await open(page, "manager-token");
  hub.online = false;
  await face(page, "PC 4").focus();
  await page.keyboard.press("Enter");
  hub.devices = hub.devices.map((d) => (d.status === "maintenance" ? { ...d, online: false } : d));
  await page.evaluate(() =>
    (window as unknown as { __emit: (n: string, d: unknown) => void }).__emit(
      "devices.changed",
      {},
    ),
  );
  await act(page, "Admin unlock").click();
  await expect(status(page)).toContainText(en["hub.floor.remote_unlock_failed"]);
  expect(hub.sent).toHaveLength(0);
});

test("destructive actions are secondary, 24px from the primary ones, and every target is 44px", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 1000 });
  await open(page, "counter-token");
  await face(page, "PC 1").focus();
  await page.keyboard.press("Enter");
  const box = async (name: string) => (await act(page, name).boundingBox())!;
  const message = await box("Message");
  const lock = await box("Lock");
  const restart = await box("Restart");
  const shutdown = await box("Shut down");
  expect(lock.y - (message.y + message.height)).toBeGreaterThanOrEqual(24);
  expect(restart.y - (lock.y + lock.height)).toBeGreaterThanOrEqual(24);
  expect(shutdown.y).toBeGreaterThan(restart.y);
  for (const b of [message, lock, restart, shutdown]) expect(b.height).toBeGreaterThanOrEqual(44);
  const secondary = (name: string) =>
    act(page, name).evaluate((el) => el.className.includes("button--secondary"));
  expect(await secondary("Lock")).toBe(true);
  expect(await secondary("Restart")).toBe(true);
  expect(await secondary("Shut down")).toBe(true);
  expect(await secondary("Message")).toBe(false);
});

test("keyboard alone opens the sheet, acts, and closes it", async ({ page }) => {
  const hub = await open(page, "counter-token");
  await page.keyboard.press("Tab");
  await page.keyboard.press("Tab");
  const first = page.locator("[data-tile-face][tabindex='0']");
  await first.focus();
  await page.keyboard.press("Enter");
  await expect(sheet(page).getByRole("heading")).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(act(page, "Close")).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(act(page, "Guest")).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(act(page, en["hub.session.start"])).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(act(page, "Unlock")).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(status(page)).toHaveText(en["hub.sheet.done.unlock"]);
  expect(hub.sent).toHaveLength(1);
  await page.keyboard.press("Escape");
  await expect(sheet(page)).toHaveCount(0);
  await expect(first).toBeFocused();
});

test("a live update never moves focus out of the sheet", async ({ page }) => {
  const hub = await open(page, "counter-token");
  await face(page, "PC 1").focus();
  await page.keyboard.press("Enter");
  await act(page, "Unlock").focus();
  hub.devices = hub.devices.map((d) => (d.label === "PC 1" ? { ...d, status: "in_session" } : d));
  await page.evaluate(() =>
    (window as unknown as { __emit: (n: string, d: unknown) => void }).__emit(
      "devices.changed",
      {},
    ),
  );
  await expect(page.getByTestId("sheet-status")).toContainText(en["ui.status.in_session"]);
  await expect(act(page, "Unlock")).toBeFocused();
});

for (const hc of [false, true]) {
  test(`axe: the open sheet has no serious violations (${hc ? "HC" : "dark"})`, async ({
    page,
  }) => {
    await open(page, "counter-token", { hc });
    await face(page, "PC 1").focus();
    await page.keyboard.press("Enter");
    await expect(sheet(page)).toBeVisible();
    await act(page, "Restart").click();
    await expect(page.getByRole("group", { name: en["ui.pin.prompt"] })).toBeVisible();
    await clean(page);
  });
}

test("an in-session PC offers Order to staff who can sell, and it opens the POS on that session", async ({
  page,
}) => {
  await open(page, "seller-token");
  await face(page, "PC 2").focus();
  await page.keyboard.press("Enter");
  await act(page, en["hub.session.order"]).click();
  await expect(page).toHaveURL(/\/pos\?session=S2$/);
});

test("Order is not offered without pos.sell, on a free PC, or on a session in a group bill", async ({
  page,
}) => {
  await open(page, "counter-token");
  await face(page, "PC 2").focus();
  await page.keyboard.press("Enter");
  await expect(act(page, en["hub.session.order"])).toHaveCount(0);
  await page.keyboard.press("Escape");
  await face(page, "PC 1").focus();
  await page.keyboard.press("Enter");
  await expect(act(page, en["hub.session.order"])).toHaveCount(0);

  const grouped = sessions().map((s) => ({ ...s, mode: "postpaid", group_bill_id: "G9" }));
  await open(page, "seller-token", { sessions: grouped });
  await face(page, "PC 2").focus();
  await page.keyboard.press("Enter");
  await expect(act(page, en["hub.session.order"])).toHaveCount(0);
});

test("the running bill lists the products placed on it after the session lines", async ({
  page,
}) => {
  const bill: SnapshotBill = {
    session_id: "S2",
    device_id: device(2, {}).device_id,
    session_rev: 1,
    lines: [
      {
        device_id: device(2, {}).device_id,
        from_at: Date.now() - 3_725_000,
        to_at: Date.now(),
        minutes: 62,
        rate_minor_per_hour: 10000,
        amount_minor: 10333,
      },
    ],
    items: [
      { line_id: "L-cola", served: true, label: "Cola", qty: 2, amount_minor: 8000 },
      { line_id: "L-chips", served: true, label: "Chips", qty: 1, amount_minor: 1000 },
    ],
    subtotal_minor: 19333,
    paid_minor: 0,
    remaining_minor: 19333,
  };
  await open(page, "seller-token", { bills: [bill] });
  await face(page, "PC 2").focus();
  await page.keyboard.press("Enter");
  const breakdown = page.getByTestId("session-bill").getByTestId("bill-breakdown");
  await expect(breakdown.getByTestId("bill-line")).toHaveCount(1);
  const items = breakdown.getByTestId("bill-item");
  await expect(items).toHaveCount(2);
  await expect(items.first()).toContainText("2 × Cola");
  await expect(items.first()).toContainText("80.00");
  await expect(breakdown.getByTestId("bill-subtotal")).toContainText("193.33");
  // The order is part of the running session's bill: there is no settle bar yet.
  await expect(page.getByTestId("settle-bar")).toHaveCount(0);
});

/** A running bill with a served Cola and an unserved Burger placed on it. */
const orderedBill = (): SnapshotBill => ({
  session_id: "S2",
  device_id: device(2, {}).device_id,
  session_rev: 1,
  lines: [],
  items: [
    { line_id: "L-cola", served: true, label: "Cola", qty: 1, amount_minor: 4000 },
    { line_id: "L-burger", served: false, label: "Burger", qty: 2, amount_minor: 18000 },
  ],
  subtotal_minor: 22000,
  paid_minor: 0,
  remaining_minor: 22000,
});

test("the order section marks a held line served and offers nothing on a served line", async ({
  page,
}) => {
  const hub = await open(page, "seller-token", { bills: [orderedBill()] });
  await face(page, "PC 2").focus();
  await page.keyboard.press("Enter");
  const lines = page.getByTestId("order-section").getByTestId("order-line");
  await expect(lines).toHaveCount(2);
  const [cola, burger] = [lines.nth(0), lines.nth(1)];
  await expect(cola).toHaveAttribute("data-served", "true");
  await expect(cola.getByTestId("order-line-state")).toContainText(en["hub.order.served"]);
  await expect(cola.getByRole("button")).toHaveCount(0);
  await expect(burger).toHaveAttribute("data-served", "false");
  await expect(burger.getByTestId("order-line-state")).toContainText(en["hub.order.unserved"]);
  await expect(page.getByTestId("order-pending")).toContainText(en["hub.order.unserved_note"]);

  await burger.getByRole("button", { name: en["hub.order.mark_served"] }).click();
  await expect
    .poll(() => hub.sent.map((s) => s.payload))
    .toContainEqual({
      kind: "order.line_served",
      line_id: "L-burger",
    });
});

test("cancelling an unserved line asks why, then whether it was already made", async ({ page }) => {
  const hub = await open(page, "seller-token", { bills: [orderedBill()] });
  await face(page, "PC 2").focus();
  await page.keyboard.press("Enter");
  const burger = page.getByTestId("order-line").nth(1);
  await burger.getByRole("button", { name: en["hub.order.cancel"] }).click();
  // Nothing is sent until both questions are answered.
  await page.getByRole("radio", { name: en["reason.changed_mind"] }).check();
  await page.getByRole("button", { name: en["ui.reason.confirm"] }).click();
  await expect(page.getByText(en["hub.order.made_legend"])).toBeVisible();
  expect(hub.sent).toHaveLength(0);
  await page.getByRole("button", { name: en["hub.order.made_yes"] }).click();
  await expect.poll(() => hub.sent.length).toBe(1);
  expect(hub.sent[0].payload).toEqual({
    kind: "order.line_cancel",
    line_id: "L-burger",
    made: true,
  });
  expect(hub.sent[0].reason).toBe("changed_mind");
});

test("cancelling can be backed out, and the reasons are the three on offer", async ({ page }) => {
  const hub = await open(page, "seller-token", { bills: [orderedBill()] });
  await face(page, "PC 2").focus();
  await page.keyboard.press("Enter");
  const burger = page.getByTestId("order-line").nth(1);
  await burger.getByRole("button", { name: en["hub.order.cancel"] }).click();
  const options = page.getByTestId("order-cancel-reason").getByRole("radio");
  await expect(options).toHaveCount(3);
  await expect(page.getByRole("radio", { name: en["reason.mistake"] })).toBeVisible();
  await expect(page.getByRole("radio", { name: en["reason.other"] })).toBeVisible();
  await page.getByRole("radio", { name: en["reason.mistake"] }).check();
  await page.getByRole("button", { name: en["ui.reason.confirm"] }).click();
  await page.getByRole("button", { name: en["hub.order.cancel_back"] }).click();
  await expect(page.getByText(en["hub.order.made_legend"])).toHaveCount(0);
  await expect(burger.getByRole("button", { name: en["hub.order.mark_served"] })).toBeVisible();
  expect(hub.sent).toHaveLength(0);
});

test("an ended session's bill cannot settle while a line is unserved", async ({ page }) => {
  const bill: SnapshotBill = {
    ...orderedBill(),
    order_id: "O1",
    order_rev: 4,
  };
  await open(page, "seller-token", { bills: [bill], sessions: [] });
  await face(page, "PC 2").focus();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("order-section")).toBeVisible();
  const submit = page.getByTestId("settle-region").getByRole("button", {
    name: en["hub.settle.submit"],
  });
  await expect(submit).toBeDisabled();
  await expect(page.getByTestId("settle-region")).toContainText(en["hub.order.unserved_note"]);
});
