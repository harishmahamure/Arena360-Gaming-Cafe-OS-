import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route, type TestInfo } from "@playwright/test";
import type { HubSnapshot, SessionStaff, SnapshotDevice, SnapshotZone } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { ORIGIN } from "./origin";

const HUB = "http://127.0.0.1:7409";
const CORS = {
  "access-control-allow-origin": ORIGIN,
  "access-control-allow-methods": "GET, POST",
  "access-control-allow-headers": "authorization, content-type",
};
const MANAGER: SessionStaff = {
  staff_id: "01JMANAGER0000000000000000",
  display_name: "Manager",
  role_name: "Manager",
  permissions: ["device.manage"],
};

type Payload = Record<string, unknown> & { kind: string };
type Hub = {
  zones: SnapshotZone[];
  devices: SnapshotDevice[];
  intents: { payload: Payload; base_rev?: number }[];
};

const pc = (n: number): SnapshotDevice => ({
  device_id: `01JPC${n}`.padEnd(26, "0"),
  label: `PC-0${n}`,
  class: "paired",
  zone: null,
  online: true,
  kind: "pc",
  group_id: null,
  meta_rev: 1,
});

const newHub = (): Hub => ({ zones: [], devices: [pc(1), pc(2)], intents: [] });

const snapshot = (hub: Hub): HubSnapshot => ({
  branch: { name: "Dev Branch", branch_code: "DEV01" },
  groups: [],
  zones: hub.zones,
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

function apply(hub: Hub, p: Payload, route: Route) {
  if (p.kind === "zone.upsert") {
    const row = { name: p.name as string, sort_order: p.sort_order as number };
    const old = hub.zones.find((z) => z.id === p.id);
    if (old) Object.assign(old, row, { rev: old.rev + 1 });
    else hub.zones.push({ id: `Z${hub.zones.length + 1}`, rev: 1, ...row });
    hub.zones.sort((a, b) => a.sort_order - b.sort_order);
  } else if (p.kind === "zone.delete") {
    if (hub.devices.some((d) => d.zone === p.id)) {
      return json(route, 409, { code: "ZONE_NOT_EMPTY", message: "not empty" });
    }
    hub.zones = hub.zones.filter((z) => z.id !== p.id);
  } else {
    const taken = hub.devices.some(
      (o) =>
        o.device_id !== p.device_id &&
        p.zone_id &&
        o.zone === p.zone_id &&
        o.floor_x === p.floor_x &&
        o.floor_y === p.floor_y,
    );
    if (taken) return json(route, 409, { code: "CELL_TAKEN", message: "taken", field: "floor_x" });
    const d = hub.devices.find((x) => x.device_id === p.device_id)!;
    d.zone = p.zone_id as string | null;
    d.floor_x = p.floor_x as number | null;
    d.floor_y = p.floor_y as number | null;
    d.meta_rev = (d.meta_rev ?? 0) + 1;
  }
  return json(route, 200, { applied: { intent_id: "I", changes: [] } });
}

async function open(page: Page, hub: Hub, path = "/settings/layout") {
  await page.addInitScript((t) => sessionStorage.setItem("arena.hub.token", t), "manager-token");
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot":
        return json(route, 200, snapshot(hub));
      case "/ui/me":
        return json(route, 200, MANAGER);
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: "retry: 300\n\n",
        });
      case "/ui/intent": {
        const body = req.postDataJSON() as { intent: { base_rev?: number; payload: Payload } };
        hub.intents.push({ payload: body.intent.payload, base_rev: body.intent.base_rev });
        return apply(hub, body.intent.payload, route);
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
  expect(bad.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual(
    [],
  );
}

const isPhone = (info: TestInfo) => info.project.name === "phone";

const addZone = async (page: Page, name: string, order?: string) => {
  await page.getByLabel(en["hub.zones.name"]).fill(name);
  if (order) await page.getByLabel(en["hub.zones.order"]).fill(order);
  await page.getByRole("button", { name: en["hub.zones.add"] }).click();
};

const cell = (page: Page, x: number, y: number) =>
  page.locator(`[data-testid^="zone-"][data-x="${x}"][data-y="${y}"]`).first();

test("a manager adds, renames and reorders zones and the list follows the sort order", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub);
  await expect(page.getByTestId("zone-form")).toBeVisible();
  await expectNoSeriousAxe(page);
  await addZone(page, "PC hall");
  await expect(page.getByLabel(en["hub.zones.name"])).toHaveValue("");
  await expect(page.getByTestId("zone-row")).toHaveCount(1);
  await addZone(page, "Console room");
  expect(hub.intents[0]).toEqual({
    payload: { kind: "zone.upsert", name: "PC hall", sort_order: 1 },
    base_rev: undefined,
  });
  expect(hub.intents[1].payload).toMatchObject({ name: "Console room", sort_order: 2 });
  await expect(page.getByTestId("zone-row")).toHaveText([/PC hall/, /Console room/]);
  await page
    .getByRole("button", { name: en["hub.zones.edit"].replace("{name}", "Console room") })
    .click();
  await page.getByLabel(en["hub.zones.name"]).fill("Consoles");
  await page.getByLabel(en["hub.zones.order"]).fill("0");
  await page.getByRole("button", { name: en["hub.zones.save"] }).click();
  expect(hub.intents.at(-1)).toEqual({
    payload: { kind: "zone.upsert", id: "Z2", name: "Consoles", sort_order: 0 },
    base_rev: 1,
  });
  await expect(page.getByTestId("zone-row")).toHaveText([/Consoles/, /PC hall/]);
});

test("a zone with devices cannot be deleted and says why; an empty one goes", async ({ page }) => {
  const hub = newHub();
  hub.zones.push({ id: "Z1", name: "PC hall", sort_order: 1, rev: 2 });
  hub.devices[0].zone = "Z1";
  hub.devices[0].floor_x = 0;
  hub.devices[0].floor_y = 0;
  await open(page, hub);
  const del = () =>
    page.getByRole("button", { name: en["hub.zones.delete"].replace("{name}", "PC hall") });
  await del().click();
  await expect(page.getByTestId("group-status")).toContainText(en["hub.zones.not_empty"]);
  expect(hub.intents.at(-1)).toEqual({ payload: { kind: "zone.delete", id: "Z1" }, base_rev: 2 });
  hub.devices[0].zone = null;
  await page.reload();
  await del().click();
  await expect(page.getByTestId("zone-list")).toHaveCount(0);
});

test("a device is picked up with Enter, moved with the arrow keys and dropped with Enter", async ({
  page,
}, info) => {
  test.skip(isPhone(info), "the grid is desktop-only");
  const hub = newHub();
  hub.zones.push({ id: "Z1", name: "PC hall", sort_order: 1, rev: 1 });
  await open(page, hub);
  await expect(page.getByTestId("zone-pool-device")).toHaveCount(2);
  await expectNoSeriousAxe(page);
  const pick = page.getByRole("button", {
    name: en["hub.zones.pick"].replace("{device}", "PC-01"),
  });
  await pick.focus();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("zone-carrying")).toContainText("PC-01");
  await expect(cell(page, 0, 0)).toBeFocused();
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect.poll(() => hub.devices[0].floor_x).toBe(2);
  expect(hub.intents.at(-1)).toEqual({
    payload: {
      kind: "device.place",
      device_id: hub.devices[0].device_id,
      zone_id: "Z1",
      floor_x: 2,
      floor_y: 1,
    },
    base_rev: 1,
  });
  await expect(cell(page, 2, 1)).toHaveText("PC-01");
  await cell(page, 2, 1).focus();
  await page.keyboard.press("Enter");
  await page.keyboard.press("ArrowLeft");
  await page.keyboard.press("Enter");
  await expect.poll(() => hub.devices[0].floor_x).toBe(1);
  await expect(page.getByTestId("zone-pool-device")).toHaveCount(1);
});

test("two devices cannot take one cell and the screen says so", async ({ page }, info) => {
  test.skip(isPhone(info), "the grid is desktop-only");
  const hub = newHub();
  hub.zones.push({ id: "Z1", name: "PC hall", sort_order: 1, rev: 1 });
  Object.assign(hub.devices[0], { zone: "Z1", floor_x: 3, floor_y: 3 });
  await open(page, hub);
  await page
    .getByRole("button", { name: en["hub.zones.pick"].replace("{device}", "PC-02") })
    .click();
  await cell(page, 3, 3).click();
  await expect(page.getByTestId("group-status")).toContainText(en["hub.zones.cell_taken"]);
  expect(hub.devices[1].zone).toBeNull();
});

test("the floor shows zones in sort order with devices in their cells and Unassigned last", async ({
  page,
}, info) => {
  test.skip(isPhone(info), "the grid is desktop-only");
  const hub = newHub();
  hub.zones.push(
    { id: "Z1", name: "Console room", sort_order: 1, rev: 1 },
    { id: "Z2", name: "PC hall", sort_order: 2, rev: 1 },
  );
  Object.assign(hub.devices[0], { zone: "Z2", floor_x: 2, floor_y: 1 });
  await open(page, hub, "/floor");
  const titles = page.locator("main h2, h2");
  await expect(titles).toHaveText(["Console room", "PC hall", en["hub.floor.unassigned"]]);
  const tile = page.locator('[data-device-id="' + hub.devices[0].device_id + '"]');
  await expect(tile).toHaveCSS("grid-column-start", "3");
  await expect(tile).toHaveCSS("grid-row-start", "2");
  await expect(page.getByTestId("unassigned-device")).toHaveCount(1);
  await expect(page.getByTestId("zone-device")).toHaveCount(1);
  await expectNoSeriousAxe(page);
});

test("a phone-width editor lists each device with Zone and Position fields at 44 px", async ({
  page,
}, info) => {
  test.skip(!isPhone(info), "the list fallback is phone-only");
  const hub = newHub();
  hub.zones.push({ id: "Z1", name: "PC hall", sort_order: 1, rev: 1 });
  await open(page, hub);
  const row = page.getByTestId("zone-list-device").filter({ hasText: "PC-01" });
  await expect(row).toBeVisible();
  await expect(page.getByTestId("zone-section").first()).toBeHidden();
  const zone = row.getByLabel(en["hub.zones.zone"]);
  const position = row.getByLabel(en["hub.zones.position"]);
  for (const el of [zone, position]) {
    expect((await el.boundingBox())!.height).toBeGreaterThanOrEqual(44);
  }
  await expect(position).toBeDisabled();
  await zone.selectOption({ label: "PC hall" });
  await expect.poll(() => hub.devices[0].zone).toBe("Z1");
  expect([hub.devices[0].floor_x, hub.devices[0].floor_y]).toEqual([0, 0]);
  await expect(position).toBeEnabled();
  await position.selectOption({ label: "Column 5, row 2" });
  await expect.poll(() => hub.devices[0].floor_x).toBe(4);
  expect(hub.devices[0].floor_y).toBe(1);
  await expectNoSeriousAxe(page);
});

test("Escape cancels a pick-up and a device can be dragged onto a cell", async ({ page }, info) => {
  test.skip(isPhone(info), "the grid is desktop-only");
  const hub = newHub();
  hub.zones.push({ id: "Z1", name: "PC hall", sort_order: 1, rev: 1 });
  await open(page, hub);
  const pick = page.getByRole("button", {
    name: en["hub.zones.pick"].replace("{device}", "PC-01"),
  });
  await pick.click();
  await expect(page.getByTestId("zone-carrying")).toContainText("PC-01");
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("zone-carrying")).toHaveText("");
  expect(hub.intents).toEqual([]);
  await pick.dragTo(cell(page, 4, 4));
  await expect.poll(() => hub.devices[0].floor_x).toBe(4);
  expect(hub.devices[0].floor_y).toBe(4);
  expect(hub.intents).toHaveLength(1);
});
