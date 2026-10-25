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
const T0 = new Date("2026-10-25T10:00:00Z").getTime();
const TEN_MIN = 10 * 60_000;
const STAFF: Record<string, SessionStaff> = {
  "manager-token": {
    staff_id: "01JMANAGER0000000000000000",
    display_name: "Manager",
    role_name: "Manager",
    permissions: ["settings.edit", "device.manage", "device.remote"],
  },
  "counter-token": {
    staff_id: "01JCOUNTER0000000000000000",
    display_name: "Counter",
    role_name: "Counter",
    permissions: ["session.start", "pos.sell"],
  },
};
const FINGERPRINT = "ab12cd34ef560789" + "0".repeat(48);
const AUTH_FAILED = { code: "AUTH_FAILED", message: "sign-in failed" };

type Hub = {
  devices: SnapshotDevice[];
  codes: string[];
  issued: number;
  discarded: number;
  queues: Set<string[]>;
  now: () => number;
  fail: number;
  hold: Promise<void> | null;
};

function newHub(): Hub {
  return {
    devices: [
      {
        device_id: "01JPC07000000000000000000A",
        label: "PC-07",
        class: "paired",
        zone: null,
        online: true,
        kind: "pc",
        group_id: null,
        meta_rev: 1,
      },
      {
        device_id: "01JPC08000000000000000000B",
        label: "PC-08",
        class: "paired",
        zone: null,
        online: false,
        kind: "pc",
        group_id: null,
        meta_rev: 1,
      },
      {
        device_id: "01JPC09000000000000000000C",
        label: "Grouped PC",
        class: "paired",
        zone: null,
        online: true,
        kind: "pc",
        group_id: "01JGROUP00000000000000000A",
        meta_rev: 2,
      },
    ],
    codes: ["123456", "654321", "777000"],
    issued: 0,
    discarded: 0,
    queues: new Set(),
    now: () => T0,
    fail: 0,
    hold: null,
  };
}

function snapshot(hub: Hub): HubSnapshot {
  return {
    paired: true,
    branch: { name: "Koramangala", branch_code: "KOR01" },
    groups: [
      {
        id: "01JGROUP00000000000000000A",
        name: "PCs",
        kind: "pc",
        colour: "teal",
        sort_order: 1,
        rev: 1,
      },
    ],
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
    ui: { idle_lock_minutes: 30, hc: false },
    connectivity: { state: "online", checked_at: null },
    lan: {
      addresses: ["192.168.1.20"],
      lan_port: 7400,
      web_port: 7401,
      fingerprint: FINGERPRINT,
    },
  };
}

function push(hub: Hub, text: string) {
  for (const q of hub.queues) q.push(text);
}

function json(route: Route, status: number, body: unknown) {
  return route.fulfill({
    status,
    contentType: "application/json",
    headers: CORS,
    body: JSON.stringify(body),
  });
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
      case "/ui/settings":
        if (!STAFF[auth]) return json(route, 401, AUTH_FAILED);
        return json(route, 200, [
          { key: "idle_lock_minutes", value: "30", rev: 1 },
          { key: "display.hc.hub", value: "false", rev: 1 },
        ]);
      case "/ui/pairing-code": {
        if (!STAFF[auth]) return json(route, 401, AUTH_FAILED);
        if (!STAFF[auth].permissions.includes("device.manage")) {
          return json(route, 403, { code: "PERMISSION_DENIED", message: "denied" });
        }
        if (hub.hold) await hub.hold;
        if (hub.fail > 0) {
          hub.fail--;
          return json(route, 500, { code: "INTERNAL", message: "x" });
        }
        const code = hub.codes[hub.issued++ % hub.codes.length];
        return json(route, 200, { code, expires_at: hub.now() + TEN_MIN });
      }
      case "/ui/pairing-code/discard":
        hub.discarded++;
        return route.fulfill({ status: 204, headers: CORS });
      case "/ui/events": {
        const t = url.searchParams.get("token") ?? "";
        if (!STAFF[t]) return json(route, 401, AUTH_FAILED);
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: `retry: 300\n\n` + queue.splice(0).join(""),
        });
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
}

async function expectNoSeriousAxe(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  expect(bad.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual(
    [],
  );
}

const pairButton = (page: Page) =>
  page.getByRole("button", { name: en["hub.settings.devices_display.pair"] });

test("a manager's pair panel shows the code 3 + 3, counts down and is renewed by the Hub", async ({
  page,
}) => {
  await page.clock.install({ time: T0 });
  const hub = newHub();
  await stub(page, hub, "manager-token");
  await page.goto("/settings/devices-display");
  await expect(pairButton(page)).toBeVisible();
  const t1 = (await page.evaluate(() => Date.now())) + 500;
  await page.clock.pauseAt(t1);
  hub.now = () => t1;
  await pairButton(page).click();
  const panel = page.getByTestId("pair-panel");
  await expect(panel.getByRole("heading", { level: 3 })).toHaveText(
    en["hub.settings.devices_display.pair_title"],
  );
  await expect(page.getByTestId("pair-code")).toHaveText("123 456");
  await expect(page.getByTestId("pair-fingerprint")).toHaveText(
    en["hub.settings.devices_display.pair_fingerprint"].replace(
      "{fingerprint}",
      "ab12 cd34 ef56 0789",
    ),
  );
  const fontVariant = await page
    .getByTestId("pair-code")
    .evaluate((el) => getComputedStyle(el).fontVariantNumeric);
  expect(fontVariant).toContain("tabular-nums");
  await expect(page.getByTestId("pair-expires")).toHaveText(
    en["hub.settings.devices_display.pair_expires"].replace("{countdown}", "10:00"),
  );
  const announce = page.getByTestId("pair-announce");
  await expect(announce).toHaveAttribute("aria-live", "polite");
  await page.clock.setSystemTime(t1 + 5 * 60_000 - 1_000);
  await page.clock.runFor(1_000);
  await expect(page.getByTestId("pair-expires")).toHaveText(
    en["hub.settings.devices_display.pair_expires"].replace("{countdown}", "5:00"),
  );
  await expect(announce).toHaveText(
    en["hub.settings.devices_display.pair_minutes_left"].replace("{minutes}", "5"),
  );
  await page.clock.setSystemTime(t1 + 9 * 60_000 - 1_000);
  await page.clock.runFor(1_000);
  await expect(page.getByTestId("pair-expires")).toHaveText(
    en["hub.settings.devices_display.pair_expires"].replace("{countdown}", "1:00"),
  );
  await expect(announce).toHaveText(en["hub.settings.devices_display.pair_one_minute_left"]);

  const renewedAt = t1 + 9 * 60_000;
  push(
    hub,
    `event: pairing.code_changed\ndata: ${JSON.stringify({ code: "654321", expires_at: renewedAt + TEN_MIN })}\n\n`,
  );
  await expect(page.getByTestId("pair-code")).toHaveText("654 321");
  await expect(announce).toHaveText(
    en["hub.settings.devices_display.pair_renewed"].replace("{code}", "654 321"),
  );
  await page.clock.runFor(1_000);
  await expect(page.getByTestId("pair-expires")).toHaveText(
    en["hub.settings.devices_display.pair_expires"].replace("{countdown}", "9:59"),
  );
  await page.clock.resume();
  await expectNoSeriousAxe(page);

  await panel.getByRole("button", { name: en["hub.settings.devices_display.pair_close"] }).click();
  await expect(page.getByTestId("pair-panel")).toHaveCount(0);
  await expect(pairButton(page)).toBeFocused();
  await expect.poll(() => hub.discarded).toBe(1);
});

test("a Counter gets no pair button", async ({ page }) => {
  const hub = newHub();
  await stub(page, hub, "counter-token");
  await page.goto("/settings/devices-display");
  await expect(page.getByTestId("hc-row-hub")).toBeVisible();
  await expect(pairButton(page)).toHaveCount(0);
  expect(hub.issued).toBe(0);
});

test("the floor lists ungrouped devices as dashed Unassigned entries and follows devices.changed", async ({
  page,
}) => {
  const hub = newHub();
  await stub(page, hub, "manager-token");
  await page.goto("/floor");
  const section = page.getByRole("region", { name: en["hub.floor.unassigned"] });
  const entries = section.getByTestId("unassigned-device");
  await expect(entries).toHaveCount(2);
  await expect(entries.nth(0)).toContainText("PC-07");
  await expect(entries.nth(0)).toContainText(en["hub.floor.online"]);
  await expect(entries.nth(1)).toContainText("PC-08");
  await expect(entries.nth(1)).toContainText(en["hub.floor.offline"]);
  await expect(section).not.toContainText("Grouped PC");
  const border = await entries.nth(0).evaluate((el) => getComputedStyle(el).borderStyle);
  expect(border).toBe("dashed");
  await expectNoSeriousAxe(page);

  hub.devices = [
    ...hub.devices,
    {
      device_id: "01JPC10000000000000000000D",
      label: "PC-10",
      class: "paired",
      zone: null,
      online: true,
      kind: "pc",
      group_id: null,
      meta_rev: 1,
    },
  ];
  push(hub, `event: devices.changed\ndata: {}\n\n`);
  await expect(entries).toHaveCount(3);
  await expect(entries.nth(2)).toContainText("PC-10");
});

async function openPanel(page: Page, hub: Hub) {
  await stub(page, hub, "manager-token");
  await page.goto("/settings/devices-display");
  await pairButton(page).click();
}

const pushEvent = (hub: Hub, data: unknown) =>
  push(hub, `event: pairing.code_changed\ndata: ${JSON.stringify(data)}\n\n`);

test("a second manager's panel takes over and the first can take it back", async ({ page }) => {
  const hub = newHub();
  await openPanel(page, hub);
  await expect(page.getByTestId("pair-code")).toHaveText("123 456");
  pushEvent(hub, { replaced: true });
  await expect(page.getByTestId("pair-replaced")).toHaveText(
    en["hub.settings.devices_display.pair_replaced"],
  );
  await expect(page.getByTestId("pair-code")).toHaveCount(0);
  await expectNoSeriousAxe(page);
  await page
    .getByRole("button", { name: en["hub.settings.devices_display.pair_take_back"] })
    .click();
  await expect(page.getByTestId("pair-code")).toHaveText("654 321");
});

test("a failed code request shows retry, and retry yields a code", async ({ page }) => {
  const hub = newHub();
  hub.fail = 1;
  await openPanel(page, hub);
  await expect(page.getByTestId("pair-failed")).toContainText(
    en["hub.settings.devices_display.pair_failed"],
  );
  await page.getByRole("button", { name: en["hub.settings.devices_display.pair_retry"] }).click();
  await expect(page.getByTestId("pair-code")).toHaveText("123 456");
});

test("a code that arrives after Close is discarded", async ({ page }) => {
  const hub = newHub();
  let release: () => void = () => {};
  hub.hold = new Promise((resolve) => (release = resolve));
  await openPanel(page, hub);
  await page
    .getByTestId("pair-panel")
    .getByRole("button", { name: en["hub.settings.devices_display.pair_close"] })
    .click();
  await expect.poll(() => hub.discarded).toBe(1);
  release();
  await expect.poll(() => hub.issued).toBe(1);
  await expect.poll(() => hub.discarded).toBe(2);
});

test("leaving the page discards the code", async ({ page }) => {
  const hub = newHub();
  await openPanel(page, hub);
  await expect(page.getByTestId("pair-code")).toHaveText("123 456");
  await page
    .getByRole("navigation", { name: en["hub.settings.sections"] })
    .getByRole("link", { name: en["hub.settings.nav.branch"] })
    .click();
  await expect(page).toHaveURL(/\/settings\/branch/);
  await expect.poll(() => hub.discarded).toBe(1);
});

test("a missed renewal is fetched 2 s after expiry, announced, and a failure backs off", async ({
  page,
}) => {
  await page.clock.install({ time: T0 });
  const hub = newHub();
  await stub(page, hub, "manager-token");
  await page.goto("/settings/devices-display");
  await expect(pairButton(page)).toBeVisible();
  const t1 = (await page.evaluate(() => Date.now())) + 500;
  await page.clock.pauseAt(t1);
  hub.now = () => t1;
  await pairButton(page).click();
  await expect(page.getByTestId("pair-code")).toHaveText("123 456");
  hub.fail = 1;
  await page.clock.setSystemTime(t1 + TEN_MIN + 1_000);
  await page.clock.runFor(1_000);
  await expect.poll(() => hub.fail).toBe(0);
  await expect(page.getByTestId("pair-failed")).toBeVisible();
  await expect(page.getByTestId("pair-code")).toHaveCount(0);
  hub.now = () => t1 + TEN_MIN + 2_000;
  await page.clock.runFor(1_000);
  expect(hub.issued).toBe(1);
  await page.clock.runFor(4_000);
  await expect(page.getByTestId("pair-code")).toHaveText("654 321");
  await expect(page.getByTestId("pair-announce")).toHaveText(
    en["hub.settings.devices_display.pair_renewed"].replace("{code}", "654 321"),
  );
});
