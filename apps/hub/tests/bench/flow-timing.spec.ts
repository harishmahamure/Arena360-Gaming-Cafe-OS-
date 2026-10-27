import { expect, test, type Page } from "@playwright/test";
import type {
  SessionStaff,
  SnapshotBill,
  SnapshotDevice,
  SnapshotSession,
} from "@arena/ui/generated";
import en from "../../../../packages/ui/src/messages/en.json" with { type: "json" };
import { CORS, HUB, device, json, snapshot } from "../floor-fixture";

// Counter timings on the reference hardware (Story 3.37): from the first tap, a session start is
// confirmed under 10 s and a settle shows its toast under 10 s. Profile creation (under 15 s) is
// `players.spec.ts` > "the required fields create a profile in under 15 seconds", which the
// release benchmark runs on the same runner. The Hub is stubbed, so this times the screen and
// the intent round trip, not the kiosk: the kiosk unlock is proven by the Story 2.29 test session.
// `FLOW_BENCH_*_MS` override the budgets.

const env = (globalThis as { process?: { env: Record<string, string | undefined> } }).process?.env;
const START_MS = Number(env?.FLOW_BENCH_START_MS ?? 10_000);
const SETTLE_MS = Number(env?.FLOW_BENCH_SETTLE_MS ?? 10_000);

const COUNTER: SessionStaff = {
  staff_id: "01JCOUNTER0000000000000000",
  display_name: "Counter",
  role_name: "Counter",
  permissions: ["pos.sell", "session.start", "player.view", "device.screen"],
};
const FREE = device(1, { status: "online" });
const BILL: SnapshotBill = {
  session_id: "S1",
  device_id: FREE.device_id,
  order_id: "O1",
  order_rev: 4,
  lines: [],
  subtotal_minor: 7500,
  paid_minor: 0,
  remaining_minor: 7500,
};

type Hub = { devices: SnapshotDevice[]; sessions: SnapshotSession[]; bills: SnapshotBill[] };

async function open(page: Page, hub: Hub) {
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
        const snap = snapshot(hub.devices);
        snap.sessions = hub.sessions;
        snap.bills = hub.bills;
        return json(route, snap);
      }
      case "/ui/me":
        return json(route, COUNTER);
      case "/ui/settings":
        return json(route, []);
      case "/ui/intent": {
        const body = req.postDataJSON() as {
          intent: { payload: { kind: string; device_id?: string }; idempotency_key: string };
        };
        const { payload, idempotency_key } = body.intent;
        if (payload.kind === "session.settle") {
          hub.bills = [];
          return json(route, {
            applied: { intent_id: "I", changes: [] },
            settled: { order_id: "O1", settled_at: 1, undo_until: 8001 },
          });
        }
        await json(route, {
          applied: {
            intent_id: "I",
            changes: [{ epoch: "1:H", seq: 9, tbl: "sessions", row_id: "S9", op: "insert" }],
          },
        });
        const id = String(payload.device_id);
        hub.devices = hub.devices.map((d) =>
          d.device_id === id ? { ...d, status: "in_session" } : d,
        );
        hub.sessions.push({
          id: "S9",
          device_id: id,
          state: "active",
          started_at: Date.now(),
          paused_total_ms: 0,
          rev: 1,
          player_id: null,
          username: null,
          guest_no: 1,
        });
        await page.evaluate(
          ([cmd_id, device_id]) => {
            const emit = (window as unknown as { __emit: (n: string, d: unknown) => void }).__emit;
            emit("session.started", { session_id: "S9", device_id });
            emit("devices.changed", {});
            emit("device.pc_ack", { cmd_id, device_id });
          },
          [idempotency_key, id],
        );
        return;
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/floor");
}

test("a session start is confirmed within the budget of the first tap", async ({ page }) => {
  await open(page, { devices: [FREE], sessions: [], bills: [] });
  const tile = page.locator("[data-component=floor-tile]", { hasText: "PC 1" });
  await tile.locator("[data-tile-face]").focus();
  await page.keyboard.press("Enter");
  const sheet = page.getByRole("complementary");
  await expect(sheet).toHaveAccessibleName("PC 1");

  const began = Date.now();
  await sheet.getByRole("button", { name: "Guest", exact: true }).click();
  await sheet.getByRole("button", { name: en["hub.session.start"], exact: true }).click();
  await expect(page.getByTestId("unlock-status")).toHaveText(en["hub.sheet.done.started"], {
    timeout: START_MS,
  });
  const took = Date.now() - began;
  console.log(`session start confirmed in ${took} ms`);
  expect(took, "session start (tap to confirmed)").toBeLessThan(START_MS);
});

test("a settle shows its toast within the budget of the tap", async ({ page }) => {
  await open(page, { devices: [FREE], sessions: [], bills: [BILL] });
  await page.locator("[data-component=floor-tile] [data-tile-face]").first().focus();
  await page.keyboard.press("Enter");
  const bar = page.getByTestId("settle-bar");
  await expect(bar).toBeVisible();

  const began = Date.now();
  await bar.locator("[data-method=cash]").click();
  await bar.getByRole("button", { name: en["hub.settle.submit"], exact: true }).click();
  await expect(page.getByTestId("settle-done")).toBeVisible({ timeout: SETTLE_MS });
  const took = Date.now() - began;
  console.log(`settle toast in ${took} ms`);
  expect(took, "settle (tap to toast)").toBeLessThan(SETTLE_MS);
});
