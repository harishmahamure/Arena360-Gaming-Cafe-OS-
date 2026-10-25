import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type { HubSnapshot, SessionStaff, SnapshotDevice, SnapshotGroup } from "@arena/ui/generated";
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
    permissions: ["device.manage", "device.remote"],
  },
  "counter-token": {
    staff_id: "01JCOUNTER0000000000000000",
    display_name: "Counter",
    role_name: "Counter",
    permissions: ["session.start"],
  },
};
const AUTH_FAILED = { code: "AUTH_FAILED", message: "sign-in failed" };

type Payload = Record<string, unknown> & { kind: string };
type Hub = {
  groups: SnapshotGroup[];
  devices: SnapshotDevice[];
  intents: { payload: Payload; base_rev?: number; ticket?: string; reason?: string }[];
  fail: { status: number; code: string; field?: string } | null;
  elevations: { items: { intent_kind: string }[]; pin: string }[];
  queues: Set<string[]>;
  stale: boolean;
  cap?: number;
};

function newHub(): Hub {
  return {
    groups: [
      { id: "G1", name: "Consoles", kind: "console", colour: "teal", sort_order: 1, rev: 1 },
    ],
    devices: [],
    intents: [],
    fail: null,
    elevations: [],
    queues: new Set(),
    stale: false,
  };
}

const snapshot = (hub: Hub): HubSnapshot => ({
  branch: { name: "Dev Branch", branch_code: "DEV01" },
  groups: hub.groups,
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
  device_cap: hub.cap,
});

const json = (route: Route, status: number, body: unknown) =>
  route.fulfill({
    status,
    contentType: "application/json",
    headers: CORS,
    body: JSON.stringify(body),
  });

function apply(hub: Hub, p: Payload, route: Route) {
  if (p.kind === "device.register") {
    const old = hub.devices.find((d) => d.device_id === p.device_id);
    const row = {
      label: p.label as string,
      kind: p.device_kind as SnapshotDevice["kind"],
      tv_number: (p.tv_number as number | undefined) ?? null,
    };
    if (old) Object.assign(old, row, { meta_rev: (old.meta_rev ?? 0) + 1 });
    else {
      hub.devices.push({
        device_id: `D${hub.devices.length + 1}`.padEnd(26, "0"),
        class: "paired",
        zone: null,
        online: false,
        status: "online",
        manual: true,
        group_id: (p.group_id as string | undefined) ?? null,
        meta_rev: 1,
        ...row,
      });
    }
  } else {
    hub.devices = hub.devices.filter((d) => d.device_id !== p.device_id);
  }
  return json(route, 200, { applied: { intent_id: "I", changes: [] } });
}

async function stub(page: Page, hub: Hub, token: string) {
  await page.addInitScript((t) => sessionStorage.setItem("arena.hub.token", t), token);
  const queue: string[] = [];
  hub.queues.add(queue);
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    const url = new URL(req.url());
    const auth = (req.headers()["authorization"] ?? "").replace(/^Bearer /, "");
    switch (url.pathname) {
      case "/ui/snapshot":
        return json(route, 200, snapshot(hub));
      case "/ui/me":
        return STAFF[auth] ? json(route, 200, STAFF[auth]) : json(route, 401, AUTH_FAILED);
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: `retry: 300\n\n` + queue.splice(0).join(""),
        });
      case "/ui/elevate": {
        const body = req.postDataJSON() as Hub["elevations"][number];
        hub.elevations.push(body);
        if (body.pin !== "2222") return json(route, 403, { code: "PIN_REJECTED", message: "no" });
        return json(route, 200, { ticket: "T1", items: body.items, expires_in_s: 60 });
      }
      case "/ui/elevate/drop":
        return route.fulfill({ status: 204, headers: CORS });
      case "/ui/intent": {
        const body = req.postDataJSON() as {
          intent: { base_rev?: number; reason?: string; payload: Payload };
          elevation_ticket?: string;
        };
        const { payload, base_rev, reason } = body.intent;
        hub.intents.push({ payload, base_rev, ticket: body.elevation_ticket, reason });
        if (!body.elevation_ticket && !STAFF[auth].permissions.includes("device.manage")) {
          return json(route, 403, { code: "PERMISSION_DENIED", message: "denied" });
        }
        if (hub.fail) return json(route, hub.fail.status, { message: "no", ...hub.fail });
        return apply(hub, payload, route);
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
}

async function open(page: Page, hub: Hub, token = "manager-token") {
  await stub(page, hub, token);
  await page.goto("/settings/devices");
  await expect(page.getByTestId("device-form")).toBeVisible();
}

async function expectNoSeriousAxe(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  expect(bad.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual(
    [],
  );
}

const addConsole = async (page: Page, label: string, tv: string) => {
  await page.getByLabel(en["hub.devices.label"]).fill(label);
  await page.getByLabel(en["hub.devices.tv"]).fill(tv);
  await page.getByRole("button", { name: en["hub.devices.add"] }).click();
};

test("a manager adds a console with a group and TV number and the floor shows it", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub);
  await expectNoSeriousAxe(page);
  await page.getByLabel(en["hub.devices.group"]).selectOption({ label: "Consoles" });
  await addConsole(page, "PS5 One", "3");
  await expect(page.getByTestId("device-status")).toContainText(en["hub.settings.saved"]);
  expect(hub.intents[0]).toEqual({
    payload: {
      kind: "device.register",
      class: "paired",
      device_kind: "console",
      label: "PS5 One",
      token_hash: "",
      tv_number: 3,
      group_id: "G1",
    },
    base_rev: undefined,
    ticket: undefined,
    reason: undefined,
  });
  await expect(page.getByTestId("device-row")).toContainText("PS5 One");
  await page.goto("/floor");
  const tile = page.getByTestId("group-device");
  await expect(tile).toContainText("PS5 One");
  await expect(tile.getByTestId("tv-number")).toHaveText(en["hub.floor.tv"].replace("{n}", "3"));
  await expect(tile.locator('[data-component="group-chip"]')).toHaveText("Consoles");
  await expect(tile.getByTestId("device-presence")).toHaveText(en["hub.floor.online"]);
  await expect(tile.getByRole("button", { name: en["hub.floor.remote_unlock"] })).toHaveCount(0);
  await expectNoSeriousAxe(page);
  await page.goto("/settings/layout");
  await expect(page.getByTestId("group-device")).toContainText("PS5 One");
});

test("a VR rig needs no TV number and only offers VR groups", async ({ page }) => {
  const hub = newHub();
  await open(page, hub);
  await page.getByLabel(en["hub.devices.group"]).selectOption({ label: "Consoles" });
  await page.getByLabel(en["hub.devices.kind"]).selectOption({ label: en["hub.devices.kind.vr"] });
  await expect(page.getByLabel(en["hub.devices.tv"])).toHaveCount(0);
  await expect(page.getByLabel(en["hub.devices.group"]).locator("option")).toHaveCount(1);
  await page.getByLabel(en["hub.devices.label"]).fill("VR 1");
  await page.getByRole("button", { name: en["hub.devices.add"] }).click();
  await expect(page.getByTestId("device-row")).toContainText("VR 1");
  expect(hub.intents[0].payload).toMatchObject({ device_kind: "vr", label: "VR 1" });
  expect(hub.intents[0].payload.group_id).toBeUndefined();
  expect(hub.intents[0].payload.tv_number).toBeUndefined();
});

test("a used TV number is a field error", async ({ page }) => {
  const hub = newHub();
  hub.fail = { status: 422, code: "VALIDATION_FAILED", field: "tv_number" };
  await open(page, hub);
  await addConsole(page, "PS5 Two", "3");
  await expect(page.getByText(en["hub.devices.tv_invalid"])).toBeVisible();
});

test("the device limit shows the tier lock", async ({ page }) => {
  const hub = newHub();
  hub.fail = { status: 403, code: "DEVICE_LIMIT" };
  await open(page, hub);
  await addConsole(page, "PS5 Two", "4");
  const limit = page.getByTestId("device-limit");
  await expect(limit).toContainText(en["hub.devices.limit"]);
  await expect(limit.locator('[data-component="tier-lock"]')).toBeVisible();
  await expectNoSeriousAxe(page);
});

test("a branch at its device cap shows the tier lock and below it does not", async ({ page }) => {
  const hub = newHub();
  hub.cap = 1;
  await open(page, hub);
  await expect(page.getByTestId("device-limit")).toHaveCount(0);
  await addConsole(page, "PS5 Two", "4");
  const limit = page.getByTestId("device-limit");
  await expect(limit).toContainText(en["hub.devices.limit"]);
  await expect(limit).toContainText(en["staff.tier.available_on"].replace("{tier}", "Basic"));
  await expectNoSeriousAxe(page);
});

test("editing sends base_rev and removing sends a revoke with a reason", async ({ page }) => {
  const hub = newHub();
  hub.devices.push({
    device_id: "D1".padEnd(26, "0"),
    label: "PS5 One",
    class: "paired",
    zone: null,
    online: false,
    kind: "console",
    group_id: null,
    meta_rev: 4,
    tv_number: 3,
    manual: true,
  });
  await open(page, hub);
  await page
    .getByRole("button", { name: en["hub.devices.edit"].replace("{name}", "PS5 One") })
    .click();
  await page.getByLabel(en["hub.devices.label"]).fill("PS5 Two");
  await page.getByRole("button", { name: en["hub.devices.save"] }).click();
  await expect(page.getByTestId("device-row")).toContainText("PS5 Two");
  expect(hub.intents.at(-1)).toMatchObject({
    payload: { kind: "device.register", device_id: "D1".padEnd(26, "0"), tv_number: 3 },
    base_rev: 4,
  });
  await page
    .getByRole("button", { name: en["hub.devices.remove"].replace("{name}", "PS5 Two") })
    .click();
  await expect(page.getByTestId("device-list")).toHaveCount(0);
  expect(hub.intents.at(-1)).toMatchObject({
    payload: { kind: "device.revoke" },
    reason: "other",
  });
});

test("a Counter's change asks for a manager's PIN in place", async ({ page }) => {
  const hub = newHub();
  await open(page, hub, "counter-token");
  await addConsole(page, "PS5 One", "1");
  const approval = page.getByTestId("device-approval");
  await expect(approval).toBeVisible();
  expect(hub.intents).toEqual([]);
  await page.keyboard.type("2222");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("device-status")).toContainText(en["hub.settings.saved"]);
  expect(hub.elevations.at(-1)).toEqual({
    items: [{ intent_kind: "device.register" }],
    pin: "2222",
  });
  expect(hub.intents[0].ticket).toBe("T1");
});
