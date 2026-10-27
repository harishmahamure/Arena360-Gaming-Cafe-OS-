import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type { SessionStaff, SnapshotDevice, SnapshotSession } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { CORS, HUB, device, json, snapshot } from "./floor-fixture";

const COUNTER: SessionStaff = {
  staff_id: "01JCOUNTER0000000000000000",
  display_name: "Counter",
  role_name: "Counter",
  permissions: [
    "session.start",
    "session.end",
    "session.pause",
    "session.transfer",
    "device.screen",
  ],
};

type Sent = { payload: Record<string, unknown> & { kind: string }; key: string; rev?: number };
type Hub = {
  devices: SnapshotDevice[];
  sessions: SnapshotSession[];
  sent: Sent[];
  fail?: { status: number; code: string };
  ack: boolean;
};

const SOURCE = device(1, { status: "in_session" });
const SESSION: SnapshotSession = {
  id: "S1",
  device_id: SOURCE.device_id,
  state: "active",
  started_at: Date.now() - 600_000,
  paused_total_ms: 0,
  rev: 4,
  player_id: null,
  username: null,
  guest_no: 1,
};

async function open(page: Page, over: Partial<Hub> = {}, state = "active"): Promise<Hub> {
  const hub: Hub = {
    devices: [
      SOURCE,
      device(2, { status: "online" }),
      device(3, { status: "offline", online: false }),
      device(4, { status: "in_session" }),
      device(5, { status: "online", kind: "console", tv_number: 1 }),
      device(6, { status: "online", group_id: null }),
    ],
    sessions: [{ ...SESSION, state: state as SnapshotSession["state"] }],
    sent: [],
    ack: true,
    ...over,
  };
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
    const url = new URL(req.url());
    switch (url.pathname) {
      case "/ui/snapshot": {
        const snap = snapshot(hub.devices);
        snap.sessions = hub.sessions;
        return json(route, snap);
      }
      case "/ui/me":
        return json(route, COUNTER);
      case "/ui/settings":
        return json(route, []);
      case "/ui/intent": {
        const body = req.postDataJSON() as {
          intent: { payload: Sent["payload"]; idempotency_key: string; base_rev?: number };
        };
        const sent: Sent = {
          payload: body.intent.payload,
          key: body.intent.idempotency_key,
          rev: body.intent.base_rev,
        };
        hub.sent.push(sent);
        if (hub.fail) {
          return route.fulfill({
            status: hub.fail.status,
            contentType: "application/json",
            headers: CORS,
            body: JSON.stringify({ message: "no", ...hub.fail }),
          });
        }
        await json(route, {
          applied: {
            intent_id: "I",
            changes: [{ epoch: "1:H", seq: 9, tbl: "sessions", row_id: "S1", op: "update" }],
          },
        });
        if (sent.payload.kind === "session.transfer") {
          const to = String(sent.payload.device_id);
          hub.devices = hub.devices.map((d) =>
            d.device_id === to
              ? { ...d, status: "in_session" }
              : d.device_id === SOURCE.device_id
                ? { ...d, status: "online" }
                : d,
          );
          hub.sessions = hub.sessions.map((s) => ({ ...s, device_id: to, rev: s.rev + 1 }));
          await page.evaluate(
            ([cmd_id, device_id, ack]) => {
              const emit = (window as unknown as { __emit: (n: string, d: unknown) => void })
                .__emit;
              emit("devices.changed", {});
              if (ack) emit("device.pc_ack", { cmd_id, device_id });
            },
            [sent.key, to, hub.ack] as const,
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
const sheet = (page: Page) => page.getByRole("complementary");
const act = (page: Page, name: string) => sheet(page).getByRole("button", { name, exact: true });
const status = (page: Page) => page.getByTestId("unlock-status");
async function openSheet(page: Page, label = "PC 1") {
  await tile(page, label).locator("[data-tile-face]").focus();
  await page.keyboard.press("Enter");
  await expect(sheet(page)).toHaveAccessibleName(label);
}
const clean = async (page: Page) =>
  expect(
    (await new AxeBuilder({ page }).analyze()).violations.filter(
      (v) => v.impact === "serious" || v.impact === "critical",
    ),
  ).toEqual([]);

test("Transfer highlights only the free, online, assigned PCs of the same kind", async ({
  page,
}) => {
  await open(page);
  await openSheet(page);
  await expect(page.locator("[data-transfer-target]")).toHaveCount(0);
  await act(page, en["hub.session.transfer"]).click();
  await expect(page.getByTestId("transfer-hint")).toHaveText(en["hub.transfer.hint"]);
  const targets = page.locator("[data-transfer-target]");
  await expect(targets).toHaveCount(1);
  await expect(targets.first()).toContainText("PC 2");
  await expect(targets.first().getByTestId("transfer-move-here")).toHaveText(
    en["hub.transfer.move_here"],
  );
  await expect(targets.first().locator("[data-tile-face]")).toHaveAccessibleName(
    new RegExp(en["hub.transfer.move_here"]),
  );
  await clean(page);
});

test("tapping a target sends session.transfer with the session revision and opens the new sheet", async ({
  page,
}) => {
  const hub = await open(page);
  await openSheet(page);
  await act(page, en["hub.session.transfer"]).click();
  await tile(page, "PC 2").locator("[data-tile-face]").click();
  await expect(sheet(page)).toHaveAccessibleName("PC 2");
  await expect(status(page)).toHaveText(en["hub.sheet.done.transferred"]);
  expect(hub.sent).toHaveLength(1);
  expect(hub.sent[0].payload).toEqual({
    kind: "session.transfer",
    session_id: "S1",
    device_id: hub.devices[1].device_id,
  });
  expect(hub.sent[0].rev).toBe(4);
  await expect(tile(page, "PC 2").getByTestId("device-presence")).toHaveText(
    en["ui.status.in_session"],
  );
  await expect(tile(page, "PC 1").getByTestId("device-presence")).toHaveText(
    en["hub.floor.online"],
  );
  await expect(page.locator("[data-transfer-target]")).toHaveCount(0);
});

test("arrows then Enter pick a target by keyboard, and Escape cancels the mode", async ({
  page,
}) => {
  const hub = await open(page);
  await openSheet(page);
  await act(page, en["hub.session.transfer"]).click();
  await expect(tile(page, "PC 2").locator("[data-tile-face]")).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(page.locator("[data-transfer-target]")).toHaveCount(0);
  expect(hub.sent).toHaveLength(0);
  await expect(sheet(page)).toHaveAccessibleName("PC 1");
  await act(page, en["hub.session.transfer"]).click();
  await tile(page, "PC 1").locator("[data-tile-face]").focus();
  await page.keyboard.press("ArrowRight");
  await expect(tile(page, "PC 2").locator("[data-tile-face]")).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(status(page)).toHaveText(en["hub.sheet.done.transferred"]);
  expect(hub.sent.map((s) => s.payload.kind)).toEqual(["session.transfer"]);
});

test("Cancel leaves the mode without sending anything", async ({ page }) => {
  const hub = await open(page);
  await openSheet(page);
  await act(page, en["hub.session.transfer"]).click();
  await act(page, en["hub.transfer.cancel"]).click();
  await expect(page.locator("[data-transfer-target]")).toHaveCount(0);
  await expect(act(page, en["hub.session.transfer"])).toBeVisible();
  expect(hub.sent).toHaveLength(0);
});

for (const [code, text] of [
  ["PC_BUSY", en["hub.session.busy"]],
  ["PC_UNAVAILABLE", en["hub.transfer.unavailable"]],
  ["GROUP_KIND_MISMATCH", en["hub.transfer.kind_mismatch"]],
  ["PLAN_NOT_FOR_GROUP", en["hub.transfer.plan_group"]],
  ["DEVICE_UNASSIGNED", en["hub.transfer.unassigned"]],
] as const) {
  test(`${code} is refused inline in the source sheet and nothing changes`, async ({ page }) => {
    const hub = await open(page, { fail: { status: 409, code } });
    await openSheet(page);
    await act(page, en["hub.session.transfer"]).click();
    await tile(page, "PC 4").locator("[data-tile-face]").focus();
    await page.keyboard.press("Enter");
    await expect(status(page)).toContainText(text);
    await expect(sheet(page)).toHaveAccessibleName("PC 1");
    await expect(tile(page, "PC 1").getByTestId("device-presence")).toHaveText(
      en["ui.status.in_session"],
    );
    await expect(page.getByTestId("transfer-hint")).toBeVisible();
    expect(hub.sent).toHaveLength(1);
  });
}

test("a silent new kiosk shows Failed - retry on the new tile and Retry resends the same key", async ({
  page,
}) => {
  const hub = await open(page, { ack: false });
  await openSheet(page);
  await act(page, en["hub.session.transfer"]).click();
  await tile(page, "PC 2").locator("[data-tile-face]").click();
  await expect(sheet(page)).toHaveAccessibleName("PC 2");
  await expect(status(page)).toContainText(en["hub.floor.remote_unlock_failed"], {
    timeout: 8000,
  });
  await expect(tile(page, "PC 2").getByTestId("tile-failed")).toBeVisible();
  hub.ack = true;
  await act(page, en["hub.sheet.retry"]).click();
  await expect(status(page)).toHaveText(en["hub.sheet.done.transferred"]);
  expect(hub.sent).toHaveLength(2);
  expect(hub.sent[1].key).toBe(hub.sent[0].key);
  expect(hub.sent[1].payload).toEqual(hub.sent[0].payload);
});

test("a paused session can be transferred and a free PC cannot", async ({ page }) => {
  await open(page, {}, "paused");
  await openSheet(page);
  await expect(act(page, en["hub.session.transfer"])).toBeVisible();
  await tile(page, "PC 2").locator("[data-tile-face]").focus();
  await page.keyboard.press("Enter");
  await expect(sheet(page)).toHaveAccessibleName("PC 2");
  await expect(act(page, en["hub.session.transfer"])).toHaveCount(0);
});
