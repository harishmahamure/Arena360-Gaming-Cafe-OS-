import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type { HubSnapshot, SessionStaff, SnapshotDevice, SnapshotLan } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { ORIGIN } from "./origin";

// Settings → Devices & display (Story 1.21): high contrast per device. "This Hub" and every
// paired device get a switch; each saves its own row with base_rev; a Counter's change needs a
// manager's PIN in place; the Hub UI switches to HC live (and before its first render) from
// `ui.hc`. The HC Hub shell (sign-in, floor, settings) passes axe with the HC outlines, focus
// ring and instant motion. The Hub's loopback API is stubbed with `page.route`.

const HUB = "http://127.0.0.1:7409";
const CORS = {
  "access-control-allow-origin": ORIGIN,
  "access-control-allow-methods": "GET, POST",
  "access-control-allow-headers": "authorization, content-type",
};
const KIOSK = "01JDEVKIOSK000000000000000";
const KIOSK_KEY = `display.hc.${KIOSK}`;
const STAFF: Record<string, SessionStaff> = {
  "manager-token": {
    staff_id: "01JMANAGER0000000000000000",
    display_name: "Manager",
    role_name: "Manager",
    permissions: ["settings.edit"],
  },
  "counter-token": {
    staff_id: "01JCOUNTER0000000000000000",
    display_name: "Counter",
    role_name: "Counter",
    permissions: ["session.start", "pos.sell"],
  },
};
const AUTH_FAILED = { code: "AUTH_FAILED", message: "sign-in failed" };
const PIN_REJECTED = { code: "PIN_REJECTED", message: "pin rejected" };

type Item = { intent_kind: string; key?: string };
type Hub = {
  settings: Map<string, { value: string; rev: number }>;
  devices: SnapshotDevice[];
  lan: SnapshotLan | null;
  intents: { key: string; value: string; base_rev?: number; ticket?: string }[];
  elevations: { items: Item[]; pin: string }[];
  tickets: Map<string, Item[]>;
  queues: Set<string[]>;
  snapshots: number;
};

function newHub(): Hub {
  return {
    settings: new Map([
      ["idle_lock_minutes", { value: "5", rev: 1 }],
      ["display.hc.hub", { value: "false", rev: 1 }],
      [KIOSK_KEY, { value: "false", rev: 1 }],
    ]),
    devices: [{ device_id: KIOSK, label: "Dev Kiosk", class: "paired", zone: null, online: true }],
    lan: {
      addresses: ["192.168.1.20", "10.0.0.4"],
      lan_port: 7400,
      web_port: 7401,
      fingerprint: "ab12cd34ef56" + "0".repeat(52),
    },
    intents: [],
    elevations: [],
    tickets: new Map(),
    queues: new Set(),
    snapshots: 0,
  };
}

const ui = (hub: Hub) => ({
  idle_lock_minutes: 5,
  hc: hub.settings.get("display.hc.hub")?.value === "true",
});

function snapshot(hub: Hub): HubSnapshot {
  return {
    branch: { name: "Dev Branch", branch_code: "DEV01" },
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
    ui: ui(hub),
    connectivity: { state: "online", checked_at: null },
    lan: hub.lan,
    paired: true,
  };
}

function pushRaw(hub: Hub, text: string) {
  for (const q of hub.queues) q.push(text);
}

function pushSettings(hub: Hub, keys: string[]) {
  const data = JSON.stringify({ locale: snapshot(hub).locale, keys, ui: ui(hub) });
  pushRaw(hub, `event: settings.changed\ndata: ${data}\n\n`);
}

function json(route: Route, status: number, body: unknown) {
  return route.fulfill({
    status,
    contentType: "application/json",
    headers: CORS,
    body: JSON.stringify(body),
  });
}

async function stub(page: Page, hub: Hub, token: string | null) {
  if (token) await page.addInitScript((t) => sessionStorage.setItem("arena.hub.token", t), token);
  const queue: string[] = [];
  hub.queues.add(queue);
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    const url = new URL(req.url());
    const auth = (req.headers()["authorization"] ?? "").replace(/^Bearer /, "");
    switch (url.pathname) {
      case "/ui/snapshot":
        hub.snapshots++;
        return json(route, 200, snapshot(hub));
      case "/ui/me":
        return STAFF[auth] ? json(route, 200, STAFF[auth]) : json(route, 401, AUTH_FAILED);
      case "/ui/settings":
        if (!STAFF[auth]) return json(route, 401, AUTH_FAILED);
        return json(
          route,
          200,
          [...hub.settings].map(([key, row]) => ({ key, ...row })),
        );
      case "/ui/events": {
        const t = url.searchParams.get("token") ?? "";
        if (!STAFF[t]) return json(route, 401, AUTH_FAILED);
        const body = `retry: 300\n\n` + queue.splice(0).join("");
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body,
        });
      }
      case "/ui/elevate": {
        if (!STAFF[auth]) return json(route, 401, AUTH_FAILED);
        const body = req.postDataJSON() as { items: Item[]; pin: string };
        hub.elevations.push(body);
        if (body.pin !== "2222") return json(route, 403, PIN_REJECTED);
        const ticket = `T${hub.tickets.size + 1}`;
        hub.tickets.set(ticket, [...body.items]);
        return json(route, 200, { ticket, items: body.items, expires_in_s: 60 });
      }
      case "/ui/elevate/drop":
        return route.fulfill({ status: 204, headers: CORS });
      case "/ui/intent": {
        if (!STAFF[auth]) return json(route, 401, AUTH_FAILED);
        const body = req.postDataJSON() as {
          intent: { base_rev?: number; payload: { key: string; value: string } };
          elevation_ticket?: string;
        };
        const { key, value } = body.intent.payload;
        const ticket = body.elevation_ticket;
        hub.intents.push({ key, value, base_rev: body.intent.base_rev, ticket });
        const approved =
          ticket !== undefined &&
          (hub.tickets.get(ticket) ?? []).some(
            (i) => i.intent_kind === "settings.set" && i.key === key,
          );
        if (!approved && !STAFF[auth].permissions.includes("settings.edit")) {
          return json(route, 403, { code: "PERMISSION_DENIED", message: "denied" });
        }
        const row = hub.settings.get(key)!;
        if (body.intent.base_rev !== row.rev) {
          return json(route, 409, { code: "STALE", message: "stale", field: "base_rev" });
        }
        if (value !== "true" && value !== "false") {
          return json(route, 422, { code: "VALIDATION_FAILED", message: "bad", field: "value" });
        }
        hub.settings.set(key, { value, rev: row.rev + 1 });
        pushSettings(hub, [key]);
        return json(route, 200, { applied: { intent_id: "I", changes: [] } });
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
}

async function openSection(page: Page, hub: Hub, token = "manager-token") {
  await stub(page, hub, token);
  await page.goto("/settings/devices-display");
  await expect(page.getByTestId("hc-row-hub")).toBeVisible();
}

const theme = (page: Page) => page.evaluate(() => document.documentElement.dataset.theme);
const hubSwitch = (page: Page) =>
  page.getByRole("switch", {
    name: en["hub.settings.devices_display.hc_for"].replace(
      "{device}",
      en["hub.settings.devices_display.this_hub"],
    ),
  });
const kioskSwitch = (page: Page, label = "Dev Kiosk") =>
  page.getByRole("switch", {
    name: en["hub.settings.devices_display.hc_for"].replace("{device}", label),
  });

async function expectNoSeriousAxe(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  expect(bad.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual(
    [],
  );
}

test("Devices & display lists This Hub and each paired device with its current value", async ({
  page,
}) => {
  const hub = newHub();
  hub.settings.set(KIOSK_KEY, { value: "true", rev: 3 });
  await openSection(page, hub);
  const sections = page.getByRole("navigation", { name: en["hub.settings.sections"] });
  await expect(
    sections.getByRole("link", { name: en["hub.settings.nav.devices_display"] }),
  ).toHaveAttribute("aria-current", "page");
  const rows = page.getByTestId("hc-rows").getByRole("listitem");
  await expect(rows).toHaveCount(2);
  await expect(rows.nth(0)).toContainText(en["hub.settings.devices_display.this_hub"]);
  await expect(rows.nth(1)).toContainText("Dev Kiosk");
  await expect(hubSwitch(page)).not.toBeChecked();
  await expect(kioskSwitch(page)).toBeChecked();
  // The state is spelled out, never colour alone.
  await expect(page.getByTestId(`hc-row-${KIOSK}`)).toContainText(
    en["hub.settings.devices_display.on"],
  );
  await expect(page.getByTestId("hc-row-hub")).toContainText(
    en["hub.settings.devices_display.off"],
  );
  // A 44px target.
  const box = await kioskSwitch(page).boundingBox();
  expect(box!.height).toBeGreaterThanOrEqual(44);
  expect(box!.width).toBeGreaterThanOrEqual(44);
  await expectNoSeriousAxe(page);
});

test("turning HC on for the kiosk saves its row with base_rev and leaves the Hub dark", async ({
  page,
}) => {
  const hub = newHub();
  await openSection(page, hub);
  expect(await theme(page)).toBe("dark");
  await kioskSwitch(page).check();
  await expect(page.getByTestId(`hc-row-${KIOSK}-status`)).toContainText(en["hub.settings.saved"]);
  expect(hub.intents).toEqual([{ key: KIOSK_KEY, value: "true", base_rev: 1 }]);
  await expect(kioskSwitch(page)).toBeChecked();
  // The pushed settings.changed is for the kiosk: this Hub stays dark.
  await page.waitForTimeout(600);
  expect(await theme(page)).toBe("dark");
  // The next change uses the new rev.
  await kioskSwitch(page).uncheck();
  await expect
    .poll(() => hub.intents.at(-1))
    .toEqual({
      key: KIOSK_KEY,
      value: "false",
      base_rev: 2,
    });
});

test("turning HC on for This Hub switches the Hub UI to HC live", async ({ page }) => {
  const hub = newHub();
  await openSection(page, hub);
  await hubSwitch(page).check();
  await expect.poll(() => theme(page), { timeout: 3000 }).toBe("hc");
  expect(hub.intents).toEqual([{ key: "display.hc.hub", value: "true", base_rev: 1 }]);
  await expect(kioskSwitch(page)).not.toBeChecked();
  // Pure black ground once HC is on.
  expect(
    await page.evaluate(() => getComputedStyle(document.documentElement).backgroundColor),
  ).toBe("rgb(0, 0, 0)");
  await hubSwitch(page).uncheck();
  await expect.poll(() => theme(page), { timeout: 3000 }).toBe("dark");
});

test("another window's change updates the switches and the theme", async ({ page }) => {
  const hub = newHub();
  await openSection(page, hub);
  hub.settings.set("display.hc.hub", { value: "true", rev: 2 });
  hub.settings.set(KIOSK_KEY, { value: "true", rev: 2 });
  pushSettings(hub, ["display.hc.hub", KIOSK_KEY]);
  await expect.poll(() => theme(page), { timeout: 3000 }).toBe("hc");
  await expect(hubSwitch(page)).toBeChecked();
  await expect(kioskSwitch(page)).toBeChecked();
});

test("a Counter's toggle asks for a manager's PIN in place and saves with the ticket", async ({
  page,
}) => {
  const hub = newHub();
  await openSection(page, hub, "counter-token");
  await kioskSwitch(page).focus();
  await page.keyboard.press("Space");
  const approval = page.getByTestId("hc-approval");
  await expect(approval).toBeVisible();
  await expect(approval).toContainText(
    en["hub.settings.devices_display.approves"].replace("{device}", "Dev Kiosk"),
  );
  await expect(approval.getByLabel(en["ui.pin.label"])).toBeFocused();
  expect(hub.intents).toEqual([]);
  await expectNoSeriousAxe(page);
  // A wrong PIN is refused inline; nothing is saved.
  await page.keyboard.type("1111");
  await page.keyboard.press("Enter");
  await expect(approval).toContainText(en["ui.pin.rejected"]);
  expect(hub.intents).toEqual([]);
  await page.keyboard.type("2222");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId(`hc-row-${KIOSK}-status`)).toContainText(en["hub.settings.saved"]);
  await expect(approval).toHaveCount(0);
  expect(hub.elevations.at(-1)).toEqual({
    items: [{ intent_kind: "settings.set", key: KIOSK_KEY }],
    pin: "2222",
  });
  expect(hub.intents).toEqual([{ key: KIOSK_KEY, value: "true", base_rev: 1, ticket: "T1" }]);
  await expect(kioskSwitch(page)).toBeChecked();
  await expect(kioskSwitch(page)).toBeFocused();
  // Esc on a second toggle closes the PIN and keeps the value.
  await page.keyboard.press("Space");
  await expect(approval).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(approval).toHaveCount(0);
  await expect(kioskSwitch(page)).toBeChecked();
  await expect(kioskSwitch(page)).toBeFocused();
  expect(hub.intents).toHaveLength(1);
  // A refused elevated save (the row changed elsewhere) returns focus to the switch too.
  hub.settings.set(KIOSK_KEY, { value: "true", rev: 9 });
  await page.keyboard.press("Space");
  await page.keyboard.type("2222");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId(`hc-row-${KIOSK}-status`)).toContainText(
    en["hub.settings.branch.stale"],
  );
  await expect(kioskSwitch(page)).toBeFocused();
});

test("a newly paired device appears without a reload", async ({ page }) => {
  const hub = newHub();
  await openSection(page, hub);
  await expect(page.getByTestId("hc-rows").getByRole("listitem")).toHaveCount(2);
  const second = "01JDEVKIOSK000000000000001";
  hub.devices = [
    ...hub.devices,
    { device_id: second, label: "Kiosk 2", class: "paired", zone: null, online: false },
  ];
  hub.settings.set(`display.hc.${second}`, { value: "false", rev: 1 });
  pushRaw(hub, "event: devices.changed\ndata: {}\n\n");
  await expect(page.getByTestId("hc-rows").getByRole("listitem")).toHaveCount(3);
  await expect(kioskSwitch(page, "Kiosk 2")).not.toBeChecked();
  await expect(kioskSwitch(page, "Kiosk 2")).toBeEnabled();
});

// ---- the HC Hub shell ----

/** Opens `path` on a Hub whose `display.hc.hub` is on, recording the theme the shell first rendered with. */
async function openHc(page: Page, path: string, token: string | null = "manager-token") {
  const hub = newHub();
  hub.settings.set("display.hc.hub", { value: "true", rev: 2 });
  await page.addInitScript(() => {
    const w = window as unknown as { __shellTheme?: string | null };
    w.__shellTheme = null;
    new MutationObserver((_, o) => {
      if (document.querySelector("main, form")) {
        w.__shellTheme = document.documentElement.dataset.theme ?? "";
        o.disconnect();
      }
    }).observe(document, { childList: true, subtree: true });
  });
  await stub(page, hub, token);
  await page.goto(path);
  return hub;
}

async function expectHcStyles(page: Page) {
  const button = page.locator("button:visible, a[href]:visible").first();
  const style = (prop: string) =>
    button.evaluate((el, p) => getComputedStyle(el).getPropertyValue(p), prop);
  expect(await style("outline-style")).toBe("solid");
  expect(await style("outline-width")).toBe("2px");
  expect(await style("outline-color")).toBe("rgb(255, 255, 255)");
  expect(await style("transition-duration")).toBe("0s");
  expect(await style("box-shadow")).toBe("none");
  await page.keyboard.press("Tab");
  const focused = page.locator(":focus-visible");
  await expect(focused).toHaveCount(1);
  expect(await focused.evaluate((el) => getComputedStyle(el).outlineWidth)).toBe("4px");
  expect(await focused.evaluate((el) => getComputedStyle(el).outlineColor)).toBe(
    "rgb(255, 225, 77)",
  );
}

for (const [name, path, token] of [
  ["sign-in", "/sign-in", null],
  ["floor", "/floor", "manager-token"],
  ["settings", "/settings/devices-display", "manager-token"],
] as const) {
  test(`HC ${name}: rendered in HC from the start, no serious axe violations, HC outlines`, async ({
    page,
  }) => {
    await openHc(page, path, token);
    await expect(page.locator("h1").first()).toBeVisible();
    expect(await theme(page)).toBe("hc");
    expect(
      await page.evaluate(() => (window as unknown as { __shellTheme: string }).__shellTheme),
    ).toBe("hc");
    await expectNoSeriousAxe(page);
    await expectHcStyles(page);
  });
}

test("HC More sheet: an opaque backdrop, no translucency", async ({ page }, info) => {
  test.skip(info.project.name !== "phone", "the More sheet is phone-only");
  await openHc(page, "/floor");
  await page.getByRole("button", { name: en["hub.nav.more"] }).click();
  await expect(page.getByRole("dialog", { name: en["hub.more.title"] })).toBeVisible();
  expect(
    await page
      .getByTestId("more-sheet-backdrop")
      .evaluate((el) => getComputedStyle(el).backgroundColor),
  ).toBe("rgb(0, 0, 0)");
  await expectNoSeriousAxe(page);
});

test("the Hub address row lists each LAN address with copy, the short fingerprint and the hint", async ({
  page,
  context,
}) => {
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  const hub = newHub();
  await openSection(page, hub);
  const row = page.getByTestId("hub-address");
  await expect(row).toContainText(en["hub.settings.devices_display.address"]);
  await expect(row.getByTestId("hub-address-ip")).toHaveText(["192.168.1.20", "10.0.0.4"]);
  await expect(row.getByTestId("hub-address-fingerprint")).toHaveText(
    en["hub.settings.devices_display.fingerprint"].replace("{fingerprint}", "ab12 cd34 ef56 0000"),
  );
  await expect(row).toContainText(en["hub.settings.devices_display.address_hint"]);
  const copy = row.getByRole("button", {
    name: en["hub.settings.devices_display.copy_address"].replace("{address}", "10.0.0.4"),
  });
  const box = await copy.boundingBox();
  expect(box!.height).toBeGreaterThanOrEqual(44);
  await copy.click();
  await expect(row.getByTestId("hub-address-status")).toHaveText(
    en["hub.settings.devices_display.copied"].replace("{address}", "10.0.0.4"),
  );
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe("10.0.0.4");
  await expectNoSeriousAxe(page);

  hub.lan = { ...hub.lan!, addresses: ["192.168.1.21"] };
  pushRaw(hub, "event: devices.changed\ndata: {}\n\n");
  await expect(row.getByTestId("hub-address-ip")).toHaveText(["192.168.1.21"]);
});

test("a Hub without a LAN address says so, and an older Hub shows no address row", async ({
  page,
}) => {
  const hub = newHub();
  hub.lan = { ...hub.lan!, addresses: [] };
  await openSection(page, hub);
  await expect(page.getByTestId("hub-address")).toContainText(
    en["hub.settings.devices_display.address_none"],
  );
  hub.lan = null;
  pushRaw(hub, "event: devices.changed\ndata: {}\n\n");
  await expect(page.getByTestId("hub-address")).toHaveCount(0);
});

test("the copied note clears after a few seconds and when the addresses change", async ({
  page,
  context,
}) => {
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await page.clock.install();
  const hub = newHub();
  await openSection(page, hub);
  const row = page.getByTestId("hub-address");
  const status = row.getByTestId("hub-address-status");
  const copy = (address: string) =>
    row.getByRole("button", {
      name: en["hub.settings.devices_display.copy_address"].replace("{address}", address),
    });
  await copy("10.0.0.4").click();
  await expect(status).toHaveText(
    en["hub.settings.devices_display.copied"].replace("{address}", "10.0.0.4"),
  );
  await page.clock.fastForward(4500);
  await expect(status).toHaveText("");

  await copy("10.0.0.4").click();
  await expect(status).not.toHaveText("");
  hub.lan = { ...hub.lan!, addresses: ["192.168.1.21"] };
  pushRaw(hub, "event: devices.changed\ndata: {}\n\n");
  await page.clock.fastForward(1000);
  await expect(row.getByTestId("hub-address-ip")).toHaveText(["192.168.1.21"]);
  await expect(status).toHaveText("");
});

test("without a clipboard the address is selected with a Ctrl+C hint", async ({ page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: () => Promise.reject(new Error("denied")) },
    });
  });
  const hub = newHub();
  await openSection(page, hub);
  const row = page.getByTestId("hub-address");
  await row
    .getByRole("button", {
      name: en["hub.settings.devices_display.copy_address"].replace("{address}", "192.168.1.20"),
    })
    .click();
  await expect(row.getByTestId("hub-address-status")).toHaveText(
    en["hub.settings.devices_display.copy_manual"],
  );
  expect(await page.evaluate(() => window.getSelection()?.toString())).toBe("192.168.1.20");
});
