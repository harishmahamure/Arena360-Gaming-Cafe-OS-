import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type { SessionStaff, SnapshotBill } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { CORS, HUB, device, json, snapshot } from "./floor-fixture";

const STAFF: Record<string, SessionStaff> = {
  counter: {
    staff_id: "01JCOUNTER00000000000000000",
    display_name: "Counter",
    role_name: "Counter",
    permissions: ["session.start", "session.end", "pos.sell", "device.screen"],
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
type Phase = "live" | "ended" | "settled";
type Hub = {
  phase: Phase;
  sent: Sent[];
  irreversible: boolean;
  fail?: { status: number; code: string };
  offline: boolean;
};

const PIN = "2222";
const BUSY = device(1, { status: "in_session" });
const FREE = device(1, { status: "online" });
const T0 = Date.UTC(2027, 0, 15, 4, 0);
const LINE = {
  device_id: BUSY.device_id,
  from_at: T0,
  to_at: T0 + 41 * 60_000,
  minutes: 45,
  rate_minor_per_hour: 10000,
  amount_minor: 7500,
};
const BILL: SnapshotBill = {
  session_id: "S1",
  device_id: BUSY.device_id,
  lines: [LINE],
  subtotal_minor: 7500,
  paid_minor: 0,
  remaining_minor: 7500,
};

const ended = (irreversible: boolean) => ({
  session_id: "S1",
  order_id: "O1",
  ended_at: 1000,
  undo_until: 9000,
  session_rev: 4,
  total_minor: 7500,
  irreversible,
});
const settled = (irreversible: boolean) => ({
  order_id: "O1",
  settled_at: 1000,
  undo_until: 9000,
  order_rev: 2,
  total_minor: 7500,
  irreversible,
});

async function open(page: Page, who: keyof typeof STAFF, over: Partial<Hub> = {}): Promise<Hub> {
  const hub: Hub = { phase: "live", sent: [], irreversible: false, offline: false, ...over };
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
        const snap = snapshot([
          hub.phase === "live" ? BUSY : FREE,
          device(2, { status: "online" }),
        ]);
        snap.sessions =
          hub.phase === "live"
            ? [
                {
                  id: "S1",
                  device_id: BUSY.device_id,
                  state: "active",
                  started_at: T0,
                  paused_total_ms: 0,
                  rev: 3,
                  guest_no: 1,
                },
              ]
            : [];
        snap.bills =
          hub.phase === "live"
            ? [BILL]
            : hub.phase === "ended"
              ? [{ ...BILL, order_id: "O1", order_rev: 1 }]
              : [];
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
        const kind = body.intent.payload.kind;
        hub.sent.push({
          payload: body.intent.payload,
          key: body.intent.idempotency_key,
          rev: body.intent.base_rev,
          ticket: body.elevation_ticket,
        });
        if (hub.offline && kind.endsWith(".reverted")) return route.abort("failed");
        if (hub.fail && kind.endsWith(".reverted")) {
          return route.fulfill({
            status: hub.fail.status,
            contentType: "application/json",
            headers: CORS,
            body: JSON.stringify({ message: "no", ...hub.fail }),
          });
        }
        const applied = { intent_id: "I", changes: [] };
        if (kind === "session.end") {
          hub.phase = "ended";
          return json(route, { applied, ended: ended(hub.irreversible) });
        }
        if (kind === "session.settle") {
          hub.phase = "settled";
          return json(route, { applied, settled: settled(hub.irreversible) });
        }
        hub.phase = kind === "session.end.reverted" ? "live" : "ended";
        return json(route, { applied });
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
const toast = (page: Page) => page.getByTestId("undo-toast");
const undoButton = (page: Page) => page.getByTestId("undo-action");
const endButton = (page: Page) =>
  sheet(page).getByRole("button", { name: en["hub.session.end"], exact: true });
const bar = (page: Page) => page.getByTestId("settle-bar");
const settleButton = (page: Page) =>
  bar(page).getByRole("button", { name: en["hub.settle.submit"], exact: true });

async function openSheet(page: Page, label = "PC 1") {
  await tile(page, label).locator("[data-tile-face]").focus();
  await page.keyboard.press("Enter");
  await expect(sheet(page)).toHaveAccessibleName(label);
}

async function endIt(page: Page) {
  await openSheet(page);
  await endButton(page).click();
  await expect(toast(page)).toBeVisible();
}

async function settleIt(page: Page) {
  await bar(page).locator("[data-method=cash]").click();
  await settleButton(page).click();
}

test("End shows the undo toast with the amount and an 8 second countdown", async ({ page }) => {
  await page.clock.install();
  await open(page, "counter");
  await endIt(page);
  await expect(page.getByTestId("undo-message")).toHaveText("Session ended · ₹75.00");
  await expect(page.getByTestId("undo-message")).toHaveClass(/tabular-nums/);
  await expect(page.getByTestId("undo-seconds")).toHaveText("8 s left");
  const box = (await undoButton(page).boundingBox())!;
  expect(box.height).toBeGreaterThanOrEqual(44);
  expect(box.width).toBeGreaterThanOrEqual(44);
  await page.clock.runFor(3_100);
  await expect(page.getByTestId("undo-seconds")).toHaveText("4 s left");
  await expect(tile(page, "PC 1").getByTestId("tile-unpaid")).toBeVisible();
  await page.clock.runFor(5_000);
  await expect(toast(page)).toHaveCount(0);
  await expect(tile(page, "PC 1").getByTestId("tile-unpaid")).toHaveText("UNPAID ₹75.00");
});

test("the toast never blocks the floor and passes axe", async ({ page }) => {
  await open(page, "counter");
  await endIt(page);
  const dock = toast(page).locator("xpath=..");
  await expect(dock).toHaveCSS("pointer-events", "none");
  await tile(page, "PC 2").locator("[data-tile-face]").click();
  await expect(sheet(page)).toHaveAccessibleName("PC 2");
  await expect(toast(page)).toBeVisible();
  expect(
    (await new AxeBuilder({ page }).analyze()).violations.filter(
      (v) => v.impact === "serious" || v.impact === "critical",
    ),
  ).toEqual([]);
});

test("Undo of an End sends session.end.reverted with the session rev and the session is back", async ({
  page,
}) => {
  const hub = await open(page, "counter");
  await endIt(page);
  await undoButton(page).click();
  await expect(page.getByTestId("undo-notice")).toHaveText(en["hub.undo.done.end"]);
  expect(hub.sent[1]).toEqual({
    payload: { kind: "session.end.reverted", session_id: "S1" },
    key: hub.sent[1].key,
    rev: 4,
    ticket: undefined,
  });
  await expect(tile(page, "PC 1").getByTestId("session-line")).toBeVisible();
  await expect(tile(page, "PC 1").getByTestId("tile-unpaid")).toHaveCount(0);
  await expect(page.getByTestId("undo-action")).toHaveCount(0);
});

test("a settled bill can be undone and the UNPAID line returns", async ({ page }) => {
  const hub = await open(page, "counter");
  await endIt(page);
  await settleIt(page);
  await expect(page.getByTestId("undo-message")).toHaveText("Bill settled · ₹75.00");
  await expect(tile(page, "PC 1").getByTestId("tile-unpaid")).toHaveCount(0);
  await undoButton(page).click();
  await expect(page.getByTestId("undo-notice")).toHaveText(en["hub.undo.done.settle"]);
  const undone = hub.sent.at(-1)!;
  expect(undone.payload).toEqual({ kind: "session.settle.reverted", order_id: "O1" });
  expect(undone.rev).toBe(2);
  await expect(tile(page, "PC 1").getByTestId("tile-unpaid")).toHaveText("UNPAID ₹75.00");
});

test("a new money action replaces the toast and the earlier one is final in the UI", async ({
  page,
}) => {
  await open(page, "counter");
  await endIt(page);
  await expect(page.getByTestId("undo-message")).toHaveText("Session ended · ₹75.00");
  await settleIt(page);
  await expect(page.getByTestId("undo-message")).toHaveText("Bill settled · ₹75.00");
  await expect(toast(page)).toHaveCount(1);
  await expect(page.getByTestId("undo-seconds")).toHaveText("8 s left");
});

test("an effect event closes the toast, and a bill with an effect already shows none", async ({
  page,
}) => {
  const hub = await open(page, "counter");
  await endIt(page);
  await page.evaluate(() =>
    (window as unknown as { __emit: (n: string, d: unknown) => void }).__emit("effect.recorded", {
      kind: "receipt_printed",
      order_id: "other",
    }),
  );
  await expect(toast(page)).toBeVisible();
  await page.evaluate(() =>
    (window as unknown as { __emit: (n: string, d: unknown) => void }).__emit("effect.recorded", {
      kind: "receipt_printed",
      order_id: "O1",
    }),
  );
  await expect(toast(page)).toHaveCount(0);
  expect(hub.sent).toHaveLength(1);

  hub.irreversible = true;
  await settleIt(page);
  await expect(page.getByTestId("settle-done")).toBeVisible();
  await expect(toast(page)).toHaveCount(0);
});

test("a late or refused Undo says so and the toast closes", async ({ page }) => {
  await page.clock.install();
  await open(page, "counter", { fail: { status: 409, code: "UNDO_NOT_ALLOWED" } });
  await endIt(page);
  await undoButton(page).click();
  await expect(page.getByTestId("undo-notice")).toHaveText(`⚠ ${en["hub.undo.not_allowed"]}`);
  await expect(page.getByTestId("undo-action")).toHaveCount(0);
  await page.clock.runFor(3_500);
  await expect(toast(page)).toHaveCount(0);
});

test("a stale Undo says the bill changed", async ({ page }) => {
  await open(page, "counter", { fail: { status: 409, code: "STALE" } });
  await endIt(page);
  await undoButton(page).click();
  await expect(page.getByTestId("undo-notice")).toHaveText(`⚠ ${en["hub.undo.stale"]}`);
});

test("an unreachable Hub keeps the Undo so it can be tried again with the same key", async ({
  page,
}) => {
  const hub = await open(page, "counter", { offline: true });
  await endIt(page);
  await undoButton(page).click();
  await expect(page.getByTestId("undo-error")).toHaveText(`⚠ ${en["hub.undo.failed"]}`);
  hub.offline = false;
  await undoButton(page).click();
  await expect(page.getByTestId("undo-notice")).toHaveText(en["hub.undo.done.end"]);
  const attempts = hub.sent.filter((s) => s.payload.kind === "session.end.reverted");
  expect(attempts).toHaveLength(2);
  expect(attempts[1].key).toBe(attempts[0].key);
});

test("Undo without the permission asks for a manager PIN and sends the ticket", async ({
  page,
}) => {
  const hub = await open(page, "plain");
  await openSheet(page);
  await endButton(page).click();
  await page.keyboard.type(PIN);
  await page.keyboard.press("Enter");
  await expect(toast(page)).toBeVisible();
  await undoButton(page).click();
  await expect(toast(page).locator("[data-component=inline-pin]")).toBeVisible();
  expect(hub.sent).toHaveLength(1);
  await page.keyboard.type(PIN);
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("undo-notice")).toHaveText(en["hub.undo.done.end"]);
  expect(hub.sent[1].payload.kind).toBe("session.end.reverted");
  expect(hub.sent[1].ticket).toBe("T1");
});

test("the toast works by keyboard", async ({ page }) => {
  const hub = await open(page, "counter");
  await endIt(page);
  await undoButton(page).focus();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("undo-notice")).toBeVisible();
  expect(hub.sent).toHaveLength(2);
});
