import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type { HubSnapshot, SessionStaff, SnapshotDevice } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { ORIGIN } from "./origin";

const HUB = "http://127.0.0.1:7409";
const CORS = {
  "access-control-allow-origin": ORIGIN,
  "access-control-allow-methods": "GET, POST",
  "access-control-allow-headers": "authorization, content-type",
};
const STAFF: Record<string, SessionStaff> = {
  "manager-token": {
    staff_id: "01JMANAGER0000000000000000",
    display_name: "Manager",
    role_name: "Manager",
    permissions: ["device.manage"],
  },
  "counter-token": {
    staff_id: "01JCOUNTER0000000000000000",
    display_name: "Counter",
    role_name: "Counter",
    permissions: ["session.start"],
  },
};

type Payload = Record<string, unknown> & { kind: string };
type Hub = {
  devices: SnapshotDevice[];
  prior: Map<string, SnapshotDevice["status"]>;
  intents: { payload: Payload; base_rev?: number; ticket?: string }[];
  fail: { status: number; code: string; field?: string } | null;
};

const device = (id: string, label: string, extra: Partial<SnapshotDevice>): SnapshotDevice => ({
  device_id: id.padEnd(26, "0"),
  label,
  class: "paired",
  zone: null,
  online: false,
  kind: "pc",
  group_id: "G1",
  meta_rev: 1,
  status: "offline",
  ...extra,
});

const newHub = (): Hub => ({
  devices: [
    device("P1", "PC 1", { online: true, status: "online" }),
    device("P2", "PC 2", { last_seen_at: Date.UTC(2026, 9, 25, 9, 30) }),
    device("M1", "TV 1", { kind: "console", manual: true, status: "online", tv_number: 1 }),
  ],
  prior: new Map(),
  intents: [],
  fail: null,
});

const snapshot = (hub: Hub): HubSnapshot => ({
  branch: { name: "Dev Branch", branch_code: "DEV01" },
  groups: [{ id: "G1", name: "Hall", kind: "pc", colour: "teal", sort_order: 1, rev: 1 }],
  zones: [],
  devices: hub.devices,
  locale: {
    currency: "INR",
    exponent: 2,
    symbol: "₹",
    symbol_position: "before",
    number_format: "indian",
    tz: "Asia/Kolkata",
    day_start_min: 360,
    fy_start_month: 4,
    pending: null,
  },
  ui: { idle_lock_minutes: 5, hc: false },
  connectivity: { state: "online", checked_at: null },
  lan: null,
  paired: true,
});

const json = (route: Route, status: number, body: unknown) =>
  route.fulfill({
    status,
    contentType: "application/json",
    headers: CORS,
    body: JSON.stringify(body),
  });

function apply(hub: Hub, p: Payload) {
  const d = hub.devices.find((x) => x.device_id === p.device_id);
  if (!d) return;
  d.meta_rev = (d.meta_rev ?? 0) + 1;
  if (p.on) {
    d.maintenance_note = ((p.note as string | undefined) ?? "").trim() || null;
    if (!hub.prior.has(d.device_id)) hub.prior.set(d.device_id, d.status);
    d.status = "maintenance";
  } else {
    d.status = hub.prior.get(d.device_id) ?? "online";
    d.maintenance_note = null;
    hub.prior.delete(d.device_id);
  }
}

async function open(page: Page, hub: Hub, path: string, token = "manager-token") {
  await page.addInitScript((t) => sessionStorage.setItem("arena.hub.token", t), token);
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    const url = new URL(req.url());
    const auth = (req.headers()["authorization"] ?? "").replace(/^Bearer /, "");
    switch (url.pathname) {
      case "/ui/snapshot":
        return json(route, 200, snapshot(hub));
      case "/ui/me":
        return json(route, 200, STAFF[auth]);
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: `retry: 300000\n\n`,
        });
      case "/ui/elevate": {
        const body = req.postDataJSON() as { pin: string; items: { intent_kind: string }[] };
        if (body.pin !== "2222") return json(route, 403, { code: "PIN_REJECTED", message: "no" });
        return json(route, 200, { ticket: "T1", items: body.items, expires_in_s: 60 });
      }
      case "/ui/elevate/drop":
        return route.fulfill({ status: 204, headers: CORS });
      case "/ui/intent": {
        const body = req.postDataJSON() as {
          intent: { base_rev?: number; payload: Payload };
          elevation_ticket?: string;
        };
        const { payload, base_rev } = body.intent;
        hub.intents.push({ payload, base_rev, ticket: body.elevation_ticket });
        if (!body.elevation_ticket && !STAFF[auth].permissions.includes("device.manage")) {
          return json(route, 403, { code: "PIN_REQUIRED", message: "pin" });
        }
        if (hub.fail) return json(route, hub.fail.status, { message: "no", ...hub.fail });
        apply(hub, payload);
        return json(route, 200, { applied: { intent_id: "I", changes: [] } });
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto(path);
}

async function expectNoSeriousAxe(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  expect(bad.map((v) => v.id)).toEqual([]);
}

const row = (page: Page, label: string) =>
  page.getByTestId("status-row").filter({ hasText: label });

test("every device shows one status in words and a manual device shows online", async ({
  page,
}) => {
  await open(page, newHub(), "/floor");
  const tiles = page.getByTestId("group-device");
  await expect(tiles).toHaveCount(3);
  const tile = (label: string) => tiles.filter({ hasText: label });
  await expect(tile("PC 1").getByTestId("device-presence")).toHaveText(en["hub.floor.online"]);
  await expect(tile("PC 2").getByTestId("device-presence")).toHaveText(en["hub.floor.offline"]);
  await expect(tile("PC 2").getByTestId("device-last-seen")).toContainText("3:00");
  await expect(tile("TV 1").getByTestId("device-presence")).toHaveText(en["hub.floor.online"]);
  await expectNoSeriousAxe(page);
});

test("a manager sets maintenance with a note and returns the device to service", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub, "/settings/devices");
  await row(page, "PC 1")
    .getByRole("button", { name: en["hub.devices.maintenance"].replace("{name}", "PC 1") })
    .click();
  await page.getByLabel(en["hub.devices.note"]).fill("  fan is dead ");
  await expectNoSeriousAxe(page);
  await page.getByRole("button", { name: en["hub.devices.maintenance_confirm"] }).click();
  await expect(row(page, "PC 1")).toContainText(en["ui.status.maintenance"]);
  await expect(row(page, "PC 1").getByTestId("maintenance-note")).toHaveText("fan is dead");
  expect(hub.intents[0]).toEqual({
    payload: {
      kind: "device.maintenance",
      device_id: "P1".padEnd(26, "0"),
      on: true,
      note: "  fan is dead ",
    },
    base_rev: 1,
    ticket: undefined,
  });
  await page.goto("/floor");
  const tile = page.getByTestId("group-device").filter({ hasText: "PC 1" });
  await expect(tile.getByTestId("device-presence")).toHaveText(en["ui.status.maintenance"]);
  await expect(tile.getByTestId("device-unavailable")).toContainText(en["hub.floor.unavailable"]);
  await expect(tile.getByTestId("device-unavailable")).toContainText("fan is dead");
  await expect(tile.locator('[data-component="group-chip"]')).toHaveText("Hall");
  await expectNoSeriousAxe(page);
  await page.goto("/settings/devices");
  await row(page, "PC 1")
    .getByRole("button", { name: en["hub.devices.return_to_service"].replace("{name}", "PC 1") })
    .click();
  await expect(row(page, "PC 1")).toContainText(en["hub.floor.online"]);
  expect(hub.intents[1].payload).toEqual({
    kind: "device.maintenance",
    device_id: "P1".padEnd(26, "0"),
    on: false,
  });
  expect(hub.intents[1].base_rev).toBe(2);
});

test("a counter needs a manager PIN to set maintenance", async ({ page }) => {
  const hub = newHub();
  await open(page, hub, "/settings/devices", "counter-token");
  await row(page, "PC 2")
    .getByRole("button", { name: en["hub.devices.maintenance"].replace("{name}", "PC 2") })
    .click();
  await page.getByRole("button", { name: en["hub.devices.maintenance_confirm"] }).click();
  await expect(page.getByTestId("device-approval")).toBeVisible();
  await page.keyboard.type("2222");
  await page.keyboard.press("Enter");
  await expect(row(page, "PC 2")).toContainText(en["ui.status.maintenance"]);
  expect(hub.intents.at(-1)?.ticket).toBe("T1");
});

test("a too-long note is a field error", async ({ page }) => {
  const hub = newHub();
  hub.fail = { status: 422, code: "VALIDATION_FAILED", field: "note" };
  await open(page, hub, "/settings/devices");
  await row(page, "PC 1")
    .getByRole("button", { name: en["hub.devices.maintenance"].replace("{name}", "PC 1") })
    .click();
  await page.getByRole("button", { name: en["hub.devices.maintenance_confirm"] }).click();
  await expect(page.getByText(en["hub.devices.note_invalid"])).toBeVisible();
});

test("an over-cap device keeps its tile, says it is over the plan limit and offers Upgrade (Story 10.12)", async ({
  page,
}) => {
  const hub = newHub();
  hub.devices[0].over_cap = true;
  await open(page, hub, "/floor");
  const tiles = page.getByTestId("group-device");
  await expect(tiles).toHaveCount(3);
  const tile = tiles.filter({ hasText: "PC 1" });
  await expect(tile.getByTestId("device-over-cap")).toContainText(en["hub.floor.over_cap"]);
  await expect(tiles.filter({ hasText: "TV 1" }).getByTestId("device-over-cap")).toHaveCount(0);
  await tile.click();
  const lock = page.getByTestId("sheet-over-cap");
  await expect(lock).toContainText(en["hub.floor.over_cap"]);
  await expect(lock.getByRole("link", { name: en["staff.tier.upgrade"] })).toBeVisible();
  await expect(page.getByTestId("session-start")).toHaveCount(0);
  await expectNoSeriousAxe(page);
});
