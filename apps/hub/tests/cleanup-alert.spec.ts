import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type { SessionStaff, SnapshotAlert } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { CORS, HUB, device, json, snapshot } from "./floor-fixture";

// The CHECK PC alert (Story 6.11): a kiosk whose session-end cleanup failed twice raises it on
// its tile. The tile says CHECK PC, opening the sheet does not acknowledge it, and the sheet
// offers "Retry cleanup" and "Checked": each pushes the pc command through `device.screen` and,
// once the PC has taken it, acknowledges the alert. The Hub's loopback API is stubbed.

const PC1 = device(1, {}).device_id;
const MIN = 60_000;

const STAFF: SessionStaff = {
  staff_id: "01JCOUNTER0000000000000000",
  display_name: "Counter",
  role_name: "Counter",
  permissions: ["device.screen"],
};

type Stub = {
  alerts: SnapshotAlert[];
  screens: Array<{ action: string; device_id: string }>;
  acks: string[];
  online: boolean;
};

const checkPc = (): SnapshotAlert => ({
  id: "A1",
  kind: "check_pc",
  device_id: PC1,
  raised_at: Date.now() - 2 * MIN,
});

async function open(page: Page, over: Partial<Stub> = {}): Promise<Stub> {
  const hub: Stub = { alerts: [checkPc()], screens: [], acks: [], online: true, ...over };
  await page.addInitScript(() => {
    sessionStorage.setItem("arena.hub.token", "counter-token");
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
    HTMLMediaElement.prototype.play = () => Promise.resolve();
  });
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot": {
        const snap = snapshot([device(1, { online: hub.online }), device(2, {})]);
        snap.alerts = hub.alerts;
        return json(route, snap);
      }
      case "/ui/me":
        return json(route, STAFF);
      case "/ui/settings":
        return json(route, []);
      case "/ui/intent": {
        const body = req.postDataJSON() as {
          intent: {
            payload: { kind: string; action?: string; device_id?: string; alert_id?: string };
            idempotency_key: string;
          };
        };
        const { payload, idempotency_key } = body.intent;
        if (payload.kind === "alert.ack") {
          hub.acks.push(payload.alert_id ?? "");
          hub.alerts = hub.alerts.filter((a) => a.id !== payload.alert_id);
          return json(route, {
            applied: {
              intent_id: "I",
              changes: [{ epoch: "1:H", seq: 9, tbl: "alerts", row_id: "A1", op: "update" }],
            },
          });
        }
        expect(payload.kind).toBe("device.screen");
        hub.screens.push({ action: payload.action ?? "", device_id: payload.device_id ?? "" });
        await json(route, {
          applied: {
            intent_id: "I",
            changes: [{ epoch: "1:H", seq: 9, tbl: "device_events", row_id: "E1", op: "insert" }],
          },
        });
        await page.evaluate(
          ([key, id]) =>
            (window as unknown as { __emit: (n: string, d: unknown) => void }).__emit(
              "device.pc_ack",
              { cmd_id: key, device_id: id },
            ),
          [idempotency_key, payload.device_id],
        );
        return;
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/floor");
  await expect(page.locator("[data-component=floor-tile]").first()).toBeVisible();
  return hub;
}

const tile = (page: Page, label: string) =>
  page.locator("[data-component=floor-tile]", { hasText: label });
const sheet = (page: Page) => page.getByRole("complementary");
const actions = (page: Page) => page.getByTestId("cleanup-actions");

async function clean(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  expect(
    results.violations.filter((v) => v.impact === "serious" || v.impact === "critical"),
  ).toEqual([]);
}

test("a CHECK PC alert tags the tile and the list names the failed cleanup", async ({ page }) => {
  await open(page);
  await expect(tile(page, "PC 1").locator("[data-component=alert-badge]")).toHaveText(
    en["ui.alert.tag.check_pc"],
  );
  await page.getByTestId("hub-alerts").click();
  await expect(page.getByTestId("alert-row")).toContainText(en["hub.alerts.kind.check_pc"]);
  await clean(page);
});

test("opening the sheet keeps the alert and offers Retry cleanup and Checked", async ({ page }) => {
  const hub = await open(page);
  await tile(page, "PC 1").locator("[data-tile-face]").click();
  await expect(sheet(page)).toBeVisible();
  const box = actions(page);
  await expect(box.getByRole("button", { name: en["hub.sheet.cleanup_retry"] })).toBeVisible();
  await expect(box.getByRole("button", { name: en["hub.sheet.cleanup_checked"] })).toBeVisible();
  await page.waitForTimeout(300);
  expect(hub.acks).toEqual([]);
  await expect(tile(page, "PC 1").locator("[data-component=alert-badge]")).toHaveText(
    en["ui.alert.tag.check_pc"],
  );
  await clean(page);
});

test("Retry cleanup pushes the command and acknowledges the alert", async ({ page }) => {
  const hub = await open(page);
  await tile(page, "PC 1").locator("[data-tile-face]").click();
  await actions(page).getByRole("button", { name: en["hub.sheet.cleanup_retry"] }).click();
  await expect.poll(() => hub.screens).toEqual([{ action: "cleanup_retry", device_id: PC1 }]);
  await expect.poll(() => hub.acks).toEqual(["A1"]);
  await expect(tile(page, "PC 1").locator("[data-component=alert-badge]")).toHaveCount(0);
  await expect(actions(page)).toHaveCount(0);
});

test("Checked pushes the command and acknowledges the alert", async ({ page }) => {
  const hub = await open(page);
  await tile(page, "PC 1").locator("[data-tile-face]").click();
  await actions(page).getByRole("button", { name: en["hub.sheet.cleanup_checked"] }).click();
  await expect.poll(() => hub.screens).toEqual([{ action: "cleanup_checked", device_id: PC1 }]);
  await expect.poll(() => hub.acks).toEqual(["A1"]);
  await expect(actions(page)).toHaveCount(0);
});

test("a PC that cannot be reached keeps the alert open for another try", async ({ page }) => {
  const hub = await open(page, { online: false });
  await tile(page, "PC 1").locator("[data-tile-face]").click();
  await actions(page).getByRole("button", { name: en["hub.sheet.cleanup_retry"] }).click();
  await page.waitForTimeout(300);
  expect(hub.screens).toEqual([]);
  expect(hub.acks).toEqual([]);
  await expect(actions(page)).toBeVisible();
});

test("a tile with no CHECK PC alert shows no cleanup actions", async ({ page }) => {
  await open(page, { alerts: [] });
  await tile(page, "PC 1").locator("[data-tile-face]").click();
  await expect(sheet(page)).toBeVisible();
  await expect(actions(page)).toHaveCount(0);
});
