import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type {
  SessionStaff,
  SnapshotBill,
  SnapshotDevice,
  SnapshotSession,
} from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { CORS, HUB, device, json, snapshot } from "./floor-fixture";

const STAFF: Record<string, SessionStaff> = {
  ender: {
    staff_id: "01JENDER000000000000000000",
    display_name: "Counter",
    role_name: "Counter",
    permissions: ["session.start", "session.end", "device.screen"],
  },
  plain: {
    staff_id: "01JPLAIN000000000000000000",
    display_name: "Plain",
    role_name: "Counter",
    permissions: ["session.start", "device.screen"],
  },
};

type Sent = {
  payload: Record<string, unknown> & { kind: string };
  key: string;
  rev?: number;
  ticket?: string;
};
type Hub = {
  devices: SnapshotDevice[];
  sessions: SnapshotSession[];
  bills: SnapshotBill[];
  sent: Sent[];
  snapshots: number;
  fail?: { status: number; code: string };
};

const PIN = "2222";
const BUSY = device(1, { status: "in_session" });
const T0 = Date.UTC(2027, 0, 15, 4, 0);
const LINE = {
  device_id: BUSY.device_id,
  from_at: T0,
  to_at: T0 + 41 * 60_000,
  minutes: 45,
  rate_minor_per_hour: 10000,
  amount_minor: 7500,
};
const live = (): Pick<Hub, "devices" | "sessions" | "bills"> => ({
  devices: [BUSY, device(2, { status: "online" })],
  sessions: [
    { id: "S1", device_id: BUSY.device_id, state: "active", started_at: T0, rev: 3, guest_no: 1 },
  ],
  bills: [
    {
      session_id: "S1",
      device_id: BUSY.device_id,
      lines: [LINE],
      subtotal_minor: 7500,
      paid_minor: 0,
      remaining_minor: 7500,
    },
  ],
});

async function open(page: Page, who: keyof typeof STAFF, over: Partial<Hub> = {}): Promise<Hub> {
  const hub: Hub = { ...live(), sent: [], snapshots: 0, ...over };
  await page.addInitScript(() => {
    sessionStorage.setItem("arena.hub.token", "tok");
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
  });
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot": {
        hub.snapshots += 1;
        const snap = snapshot(hub.devices);
        snap.sessions = hub.sessions;
        snap.bills = hub.bills;
        return json(route, snap);
      }
      case "/ui/me":
        return json(route, STAFF[who]);
      case "/ui/settings":
        return json(route, []);
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
          intent: { payload: Sent["payload"]; idempotency_key: string; base_rev?: number };
          elevation_ticket?: string;
        };
        hub.sent.push({
          payload: body.intent.payload,
          key: body.intent.idempotency_key,
          rev: body.intent.base_rev,
          ticket: body.elevation_ticket,
        });
        if (hub.fail) {
          return route.fulfill({
            status: hub.fail.status,
            contentType: "application/json",
            headers: CORS,
            body: JSON.stringify({ message: "no", ...hub.fail }),
          });
        }
        hub.devices = hub.devices.map((d) =>
          d.device_id === BUSY.device_id ? { ...d, status: "online" } : d,
        );
        hub.sessions = [];
        hub.bills = [{ ...hub.bills[0], order_id: "O1" }];
        await json(route, {
          applied: {
            intent_id: "I",
            changes: [{ epoch: "1:H", seq: 9, tbl: "orders", row_id: "O1", op: "insert" }],
          },
        });
        await page.evaluate(() =>
          (window as unknown as { __emit: (n: string, d: unknown) => void }).__emit(
            "devices.changed",
            {},
          ),
        );
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
const sheet = (page: Page) => page.getByRole("complementary");
const endButton = (page: Page) =>
  sheet(page).getByRole("button", { name: en["hub.session.end"], exact: true });
const status = (page: Page) => page.getByTestId("unlock-status");
async function openSheet(page: Page, label = "PC 1") {
  await tile(page, label).locator("[data-tile-face]").focus();
  await page.keyboard.press("Enter");
  await expect(sheet(page)).toHaveAccessibleName(label);
}

test("an in-session sheet shows the live bill lines, Subtotal, Paid so far and Remaining", async ({
  page,
}) => {
  await open(page, "ender");
  await openSheet(page);
  const bill = page.getByTestId("bill-breakdown");
  await expect(bill.getByTestId("bill-line")).toHaveCount(1);
  await expect(bill.getByTestId("bill-line")).toContainText(/PC 1 · .+–.+ · 45 min × ₹100\.00\/hr/);
  await expect(bill.getByTestId("bill-line")).toContainText("₹75.00");
  await expect(bill.getByTestId("bill-subtotal")).toContainText("₹75.00");
  await expect(bill.getByTestId("bill-paid")).toContainText("₹0.00");
  await expect(bill.getByTestId("bill-remaining")).toContainText("₹75.00");
  await expect(bill.getByTestId("bill-subtotal").locator("dd")).toHaveClass(/tabular-nums/);
  const box = (await endButton(page).boundingBox())!;
  expect(box.height).toBeGreaterThanOrEqual(44);
  expect(
    (await new AxeBuilder({ page }).analyze()).violations.filter(
      (v) => v.impact === "serious" || v.impact === "critical",
    ),
  ).toEqual([]);
});

test("End sends session.end with the session rev, frees the tile and shows UNPAID", async ({
  page,
}) => {
  const hub = await open(page, "ender");
  await openSheet(page);
  await endButton(page).click();
  await expect(status(page)).toHaveText(en["hub.sheet.done.ended"]);
  expect(hub.sent).toEqual([
    {
      payload: { kind: "session.end", session_id: "S1" },
      key: hub.sent[0].key,
      rev: 3,
      ticket: undefined,
    },
  ]);
  await expect(tile(page, "PC 1").getByTestId("tile-unpaid")).toHaveText("UNPAID ₹75.00");
  await expect(page.getByTestId("session-start")).toBeVisible();
  await expect(page.getByTestId("bill-breakdown")).toHaveCount(0);
});

test("End without session.end asks for a manager PIN and then sends the ticket", async ({
  page,
}) => {
  const hub = await open(page, "plain");
  await openSheet(page);
  await endButton(page).click();
  await page.keyboard.type(PIN);
  await page.keyboard.press("Enter");
  await expect(status(page)).toHaveText(en["hub.sheet.done.ended"]);
  expect(hub.sent).toHaveLength(1);
  expect(hub.sent[0].ticket).toBe("T1");
});

test("a stale End is refused inline and the tile stays in session", async ({ page }) => {
  await open(page, "ender", { fail: { status: 409, code: "STALE" } });
  await openSheet(page);
  await endButton(page).click();
  await expect(status(page)).toContainText(en["hub.session.end_stale"]);
  await expect(tile(page, "PC 1").getByTestId("session-line")).toBeVisible();
});

test("an open sheet on an in-session PC refetches the snapshot every 10 s", async ({ page }) => {
  await page.clock.install();
  const hub = await open(page, "ender");
  await openSheet(page);
  const before = hub.snapshots;
  await page.clock.runFor(10_500);
  await expect.poll(() => hub.snapshots).toBeGreaterThan(before);
});

test("End works by keyboard", async ({ page }) => {
  const hub = await open(page, "ender");
  await openSheet(page);
  await endButton(page).focus();
  await page.keyboard.press("Enter");
  await expect(status(page)).toHaveText(en["hub.sheet.done.ended"]);
  expect(hub.sent).toHaveLength(1);
});
