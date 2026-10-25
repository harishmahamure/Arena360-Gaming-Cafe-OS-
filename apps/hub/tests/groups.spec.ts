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
    permissions: ["device.manage"],
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
  intents: { payload: Payload; base_rev?: number; ticket?: string }[];
  elevations: { items: { intent_kind: string }[]; pin: string }[];
  queues: Set<string[]>;
  stale: boolean;
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

function newHub(): Hub {
  return {
    groups: [],
    devices: [pc(1), pc(2)],
    intents: [],
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
});

const json = (route: Route, status: number, body: unknown) =>
  route.fulfill({
    status,
    contentType: "application/json",
    headers: CORS,
    body: JSON.stringify(body),
  });

function apply(hub: Hub, p: Payload, route: Route) {
  if (p.kind === "group.upsert") {
    const row = {
      name: p.name as string,
      kind: p.group_kind as string,
      colour: p.colour as string,
      sort_order: p.sort_order as number,
    };
    const old = hub.groups.find((g) => g.id === p.id);
    if (old) Object.assign(old, row, { rev: old.rev + 1 });
    else hub.groups.push({ id: `G${hub.groups.length + 1}`, rev: 1, ...row });
  } else if (p.kind === "group.delete") {
    if (hub.devices.some((d) => d.group_id === p.id)) {
      return json(route, 409, { code: "GROUP_NOT_EMPTY", message: "not empty" });
    }
    hub.groups = hub.groups.filter((g) => g.id !== p.id);
  } else {
    const d = hub.devices.find((x) => x.device_id === p.device_id)!;
    d.group_id = p.group_id as string | null;
    d.meta_rev = (d.meta_rev ?? 0) + 1;
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
          intent: { base_rev?: number; payload: Payload };
          elevation_ticket?: string;
        };
        const { payload, base_rev } = body.intent;
        hub.intents.push({ payload, base_rev, ticket: body.elevation_ticket });
        if (!body.elevation_ticket && !STAFF[auth].permissions.includes("device.manage")) {
          return json(route, 403, { code: "PERMISSION_DENIED", message: "denied" });
        }
        if (hub.stale)
          return json(route, 409, { code: "STALE", message: "stale", field: "base_rev" });
        return apply(hub, payload, route);
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
}

async function open(page: Page, hub: Hub, token = "manager-token") {
  await stub(page, hub, token);
  await page.goto("/settings/layout");
  await expect(page.getByTestId("group-form")).toBeVisible();
}

async function expectNoSeriousAxe(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  expect(bad.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual(
    [],
  );
}

const addGroup = async (page: Page, name: string) => {
  await page.getByLabel(en["hub.groups.name"]).fill(name);
  await page.getByRole("button", { name: en["hub.groups.add"] }).click();
};

test("a manager creates a teal PCs group, moves both PCs in by keyboard and the floor shows the chip", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub);
  await expectNoSeriousAxe(page);
  await addGroup(page, "PCs");
  await expect(page.getByTestId("group-status")).toContainText(en["hub.settings.saved"]);
  expect(hub.intents[0]).toEqual({
    payload: { kind: "group.upsert", name: "PCs", group_kind: "pc", colour: "teal", sort_order: 1 },
    base_rev: undefined,
    ticket: undefined,
  });
  await expect(page.getByTestId("group-row")).toContainText("PCs");
  for (const label of ["PC-01", "PC-02"]) {
    const select = page
      .getByTestId("group-device")
      .filter({ hasText: label })
      .getByLabel(en["hub.groups.move"]);
    await select.focus();
    await select.selectOption({ label: "PCs" });
    await expect.poll(() => hub.devices.find((d) => d.label === label)?.group_id).toBe("G1");
  }
  expect(hub.intents.at(-1)?.base_rev).toBe(1);
  await page.goto("/floor");
  const tiles = page.getByTestId("group-device");
  await expect(tiles).toHaveCount(2);
  await expect(tiles.first().locator('[data-component="group-chip"]')).toHaveText("PCs");
  await expect(page.getByTestId("unassigned-device")).toHaveCount(0);
  await expectNoSeriousAxe(page);
});

test("a group with devices cannot be deleted and says why; an empty one goes", async ({ page }) => {
  const hub = newHub();
  hub.groups.push({ id: "G1", name: "PCs", kind: "pc", colour: "lime", sort_order: 1, rev: 1 });
  hub.devices[0].group_id = "G1";
  await open(page, hub);
  const del = page.getByRole("button", { name: en["hub.groups.delete"].replace("{name}", "PCs") });
  await del.click();
  await expect(page.getByTestId("group-status")).toContainText(en["hub.groups.not_empty"]);
  expect(hub.intents.at(-1)).toMatchObject({
    payload: { kind: "group.delete", id: "G1" },
    base_rev: 1,
  });
  hub.devices[0].group_id = null;
  await page
    .getByRole("button", { name: en["hub.groups.delete"].replace("{name}", "PCs") })
    .click();
  await expect(page.getByTestId("group-list")).toHaveCount(0);
});

test("editing sends base_rev and a stale edit says the group changed elsewhere", async ({
  page,
}) => {
  const hub = newHub();
  hub.groups.push({ id: "G1", name: "PCs", kind: "pc", colour: "lime", sort_order: 1, rev: 4 });
  await open(page, hub);
  await page.getByRole("button", { name: en["hub.groups.edit"].replace("{name}", "PCs") }).click();
  await page.getByLabel(en["hub.groups.name"]).fill("Gaming");
  await page.getByRole("radio", { name: en["hub.groups.colour.coral"] }).check();
  await page.getByRole("button", { name: en["hub.groups.save"] }).click();
  await expect(page.getByTestId("group-row")).toContainText("Gaming");
  expect(hub.intents.at(-1)).toMatchObject({
    payload: { kind: "group.upsert", id: "G1", name: "Gaming", colour: "coral" },
    base_rev: 4,
  });
  hub.stale = true;
  await page
    .getByRole("button", { name: en["hub.groups.edit"].replace("{name}", "Gaming") })
    .click();
  await page.getByRole("button", { name: en["hub.groups.save"] }).click();
  await expect(page.getByTestId("group-status")).toContainText(en["hub.settings.branch.stale"]);
});

test("a Counter's change asks for a manager's PIN in place", async ({ page }) => {
  const hub = newHub();
  await open(page, hub, "counter-token");
  await addGroup(page, "PCs");
  const approval = page.getByTestId("group-approval");
  await expect(approval).toBeVisible();
  expect(hub.intents).toEqual([]);
  await expectNoSeriousAxe(page);
  await page.keyboard.type("1111");
  await page.keyboard.press("Enter");
  await expect(approval).toContainText(en["ui.pin.rejected"]);
  await page.keyboard.type("2222");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("group-status")).toContainText(en["hub.settings.saved"]);
  expect(hub.elevations.at(-1)).toEqual({ items: [{ intent_kind: "group.upsert" }], pin: "2222" });
  expect(hub.intents[0].ticket).toBe("T1");
});

test("an owner's change toasts and refreshes the groups", async ({ page }) => {
  const hub = newHub();
  await open(page, hub);
  hub.groups.push({
    id: "G1",
    name: "Owner PCs",
    kind: "pc",
    colour: "sand",
    sort_order: 1,
    rev: 1,
  });
  for (const q of hub.queues) q.push(`event: groups.changed\ndata: {"by_owner":false}\n\n`);
  await page.waitForTimeout(1200);
  await expect(page.getByTestId("hub-toast")).toHaveCount(0);
  for (const q of hub.queues) q.push(`event: groups.changed\ndata: {"by_owner":true}\n\n`);
  await expect(page.getByTestId("hub-toast")).toHaveText(en["hub.groups.owner_toast"], {
    timeout: 5000,
  });
});
