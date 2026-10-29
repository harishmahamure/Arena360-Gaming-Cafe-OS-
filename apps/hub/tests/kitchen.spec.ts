import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type { HubSnapshot, KitchenDisplayView, SessionStaff } from "@arena/ui/generated";
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
    permissions: ["settings.edit", "device.manage"],
  },
  "counter-token": {
    staff_id: "01JCOUNTER0000000000000000",
    display_name: "Counter",
    role_name: "Counter",
    permissions: ["session.start", "pos.sell"],
  },
};

type Sent = { key?: string; value?: string; base_rev?: number; ticket?: string };

function settings(flags: string) {
  return new Map([
    ["licence.flags", { value: flags, rev: 1 }],
    ["kitchen.kot_mode", { value: "receipt", rev: 1 }],
    ["kitchen.kot_printer", { value: "", rev: 1 }],
    ["kitchen.kot_paper_mm", { value: "80", rev: 1 }],
  ]);
}

const json = (route: Route, status: number, body: unknown) =>
  route.fulfill({
    status,
    contentType: "application/json",
    headers: CORS,
    body: JSON.stringify(body),
  });

const SNAPSHOT: HubSnapshot = {
  branch: { name: "Dev Branch", branch_code: "DEV01" },
  groups: [],
  zones: [],
  devices: [],
  locale: null,
  ui: { idle_lock_minutes: 5, hc: false },
  connectivity: { state: "online", checked_at: null },
  lan: null,
  paired: true,
  setup: null,
};

type Display = KitchenDisplayView;

/** What the stubbed Hub holds for the kitchen displays and their pairing codes. */
type Kds = {
  displays: Display[];
  codes: number;
  discards: number;
  refuse?: { status: number; code: string };
  revokes: { display_id?: string; base_rev?: number }[];
};

const DISPLAY_A: Display = {
  id: "01JKITCHENDISPLAY0000000A",
  name: "Pass",
  connected: true,
  last_seen: 1_800_000_000_000,
  high_contrast: false,
  rev: 3,
};
const DISPLAY_B: Display = {
  id: "01JKITCHENDISPLAY0000000B",
  name: "Grill",
  connected: false,
  high_contrast: false,
  rev: 1,
};

async function open(
  page: Page,
  flags: string,
  token = "manager-token",
  stale = false,
  kds: Kds = { displays: [], codes: 0, discards: 0, revokes: [] },
) {
  const rows = settings(flags);
  const intents: Sent[] = [];
  await page.addInitScript((t) => sessionStorage.setItem("arena.hub.token", t), token);
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    const { pathname } = new URL(req.url());
    const staff = STAFF[(req.headers()["authorization"] ?? "").replace(/^Bearer /, "")];
    switch (pathname) {
      case "/ui/snapshot":
        return json(route, 200, { ...SNAPSHOT, kitchen_displays: kds.displays });
      case "/ui/kds-pairing-code":
        if (kds.refuse)
          return json(route, kds.refuse.status, { code: kds.refuse.code, message: "no" });
        kds.codes++;
        return json(route, 200, {
          code: "482913",
          expires_at: Date.now() + 10 * 60_000,
          address: "http://192.168.1.20:7401/kitchen",
        });
      case "/ui/kds-pairing-code/discard":
        kds.discards++;
        return route.fulfill({ status: 204, headers: CORS });
      case "/ui/me":
        return json(route, 200, staff);
      case "/ui/settings":
        return json(
          route,
          200,
          [...rows].map(([key, row]) => ({ key, ...row })),
        );
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: "retry: 400\n\n",
        });
      case "/ui/elevate": {
        const { items } = req.postDataJSON() as { items: unknown[] };
        return json(route, 200, { ticket: "T1", items, expires_in_s: 60 });
      }
      case "/ui/intent": {
        const { intent, elevation_ticket } = req.postDataJSON() as {
          elevation_ticket?: string;
          intent: { base_rev: number; payload: Sent };
        };
        if (stale) return json(route, 409, { code: "STALE", message: "stale", field: "base_rev" });
        if ((intent.payload as { kind?: string }).kind === "kds.revoke") {
          kds.revokes.push({
            display_id: (intent.payload as { display_id?: string }).display_id,
            base_rev: intent.base_rev,
          });
          kds.displays = kds.displays.filter((d) => d.id !== kds.revokes.at(-1)?.display_id);
          return json(route, 200, { applied: { intent_id: "I", changes: [] } });
        }
        const { key, value } = intent.payload;
        intents.push({ key, value, base_rev: intent.base_rev, ticket: elevation_ticket });
        rows.set(key!, { value: value!, rev: rows.get(key!)!.rev + 1 });
        return json(route, 200, { applied: { intent_id: "I", changes: [] } });
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/settings/kitchen");
  return intents;
}

async function expectNoSeriousAxe(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  expect(bad.map((v) => v.id)).toEqual([]);
}

test("a Free branch sees the tier lock and no fields", async ({ page }) => {
  await open(page, "");
  const lock = page.getByTestId("kitchen-tier-lock");
  await expect(lock).toContainText(en["hub.settings.kitchen.locked"]);
  await expect(lock).toContainText(en["hub.settings.kitchen.locked_note"]);
  await expect(page.getByRole("radio")).toHaveCount(0);
  await expect(page.getByRole("button", { name: en["hub.settings.save"] })).toHaveCount(0);
  await expect(page.getByText(/upgrade/i)).toHaveCount(0);
  await expectNoSeriousAxe(page);
});

test("a Basic branch edits the KOT printer and saves with base_rev", async ({ page }) => {
  const intents = await open(page, "kitchen");
  await expect(page.getByTestId("settings-kitchen")).toBeVisible();
  await expect(page.getByTestId("kitchen-displays")).toHaveText(
    en["hub.settings.kitchen.displays_none"],
  );
  await expect(
    page.getByRole("button", { name: en["hub.settings.kitchen.pair"] }),
  ).not.toHaveAttribute("aria-disabled", "true");
  await expectNoSeriousAxe(page);
  await page.getByLabel(en["hub.settings.kitchen.mode.none"]).check();
  await page.getByRole("button", { name: en["hub.settings.save"] }).click();
  await expect(page.getByTestId("kitchen-status")).toHaveText(en["hub.settings.saved"]);
  expect(intents).toEqual([
    { key: "kitchen.kot_mode", value: "none", base_rev: 1, ticket: undefined },
  ]);
  await page.getByLabel(en["hub.settings.kitchen.mode.another"]).check();
  await page.getByLabel(en["hub.settings.printer.ip"]).fill("192.168.1.60");
  await page.getByRole("button", { name: en["hub.settings.printer.add"] }).click();
  await page.getByLabel(en["hub.settings.printer.paper_58"]).check();
  await page.getByRole("button", { name: en["hub.settings.save"] }).click();
  await expect.poll(() => intents.length).toBe(4);
  await expect(page.getByTestId("kitchen-selected")).toContainText("192.168.1.60");
  expect(intents.map((i) => i.key)).toEqual([
    "kitchen.kot_mode",
    "kitchen.kot_mode",
    "kitchen.kot_paper_mm",
    "kitchen.kot_printer",
  ]);
});

test("a counter's Save asks for a manager PIN", async ({ page }) => {
  const intents = await open(page, "kitchen", "counter-token");
  await page.getByLabel(en["hub.settings.kitchen.mode.none"]).check();
  await expect(page.getByTestId("kitchen-needs-approval")).toBeVisible();
  await page.getByRole("button", { name: en["hub.settings.save"] }).click();
  await expect(page.getByText(en["ui.pin.prompt"])).toBeVisible();
  expect(intents).toEqual([]);
});

test("a manager PIN approves a counter's save with one ticket", async ({ page }) => {
  const intents = await open(page, "kitchen", "counter-token");
  await page.getByLabel(en["hub.settings.kitchen.mode.none"]).check();
  await page.getByRole("button", { name: en["hub.settings.save"] }).click();
  await page.getByLabel(en["ui.pin.label"]).fill("2222");
  await page.getByLabel(en["ui.pin.label"]).press("Enter");
  await expect(page.getByTestId("kitchen-status")).toHaveText(en["hub.settings.saved"]);
  expect(intents).toEqual([{ key: "kitchen.kot_mode", value: "none", base_rev: 1, ticket: "T1" }]);
});

test("a stale save says so and keeps the edit", async ({ page }) => {
  await open(page, "kitchen", "manager-token", true);
  await page.getByLabel(en["hub.settings.kitchen.mode.none"]).check();
  await page.getByRole("button", { name: en["hub.settings.save"] }).click();
  await expect(page.getByTestId("kitchen-status")).toContainText(en["hub.settings.branch.stale"]);
  await expect(page.getByLabel(en["hub.settings.kitchen.mode.none"])).toBeChecked();
});

test("on Android, the KOT picker says why Bluetooth printers are missing and links to settings", async ({
  page,
}) => {
  let asked = 0;
  let settingsOpened = 0;
  await page.exposeFunction("__asked", () => asked++);
  await page.exposeFunction("__settings", () => settingsOpened++);
  await page.addInitScript(() => {
    const w = window as unknown as Record<string, () => void>;
    (window as unknown as { ArenaHub: unknown }).ArenaHub = {
      state: () => JSON.stringify({ notifications: true, battery_exempt: true, asked: true }),
      bluetoothState: () => "ask",
      requestBluetooth: () => {
        w.__asked();
        setTimeout(() => window.dispatchEvent(new Event("arena:bluetooth")), 100);
      },
      openAppSettings: () => w.__settings(),
    };
  });
  await open(page, "kitchen");
  await page.route(`${HUB}/ui/printer/find`, (route) =>
    route.request().method() === "OPTIONS"
      ? route.fulfill({ status: 204, headers: CORS })
      : json(route, 200, { printers: [], bluetooth: "denied", usb_supported: false }),
  );
  await page.getByLabel(en["hub.settings.kitchen.mode.another"]).check();
  await page.getByRole("button", { name: en["hub.settings.printer.find"] }).click();
  const note = page.getByTestId("kitchen-bluetooth-note");
  await expect(note).toContainText(en["hub.settings.printer.bluetooth.denied"]);
  await expect(page.getByTestId("kitchen-usb-unsupported")).toHaveText(
    en["hub.settings.printer.usb_unsupported"],
  );
  expect(asked).toBe(1);
  await page
    .getByRole("button", { name: en["hub.settings.printer.bluetooth.open_settings"] })
    .click();
  await expect.poll(() => settingsOpened).toBe(1);
  await expectNoSeriousAxe(page);
});

test("a manager pairs a kitchen display: the address, a 6-digit code and a countdown", async ({
  page,
}) => {
  const kds: Kds = { displays: [], codes: 0, discards: 0, revokes: [] };
  await open(page, "kitchen", "manager-token", false, kds);
  await page.getByRole("button", { name: en["hub.settings.kitchen.pair"] }).click();
  const panel = page.getByTestId("pair-kitchen-panel");
  await expect(panel.getByTestId("pair-kitchen-address")).toHaveText(
    "http://192.168.1.20:7401/kitchen",
  );
  await expect(panel.getByTestId("pair-kitchen-code")).toHaveText("482 913");
  await expect(panel.getByTestId("pair-kitchen-expires")).toContainText("Expires in");
  await expectNoSeriousAxe(page);
  expect(kds.codes).toBe(1);
  await panel.getByRole("button", { name: en["hub.settings.kitchen.pair_close"] }).click();
  await expect(panel).toHaveCount(0);
  await expect.poll(() => kds.discards).toBe(1);
});

test("the pair dialog says why a Free branch gets no code", async ({ page }) => {
  const kds: Kds = {
    displays: [],
    codes: 0,
    discards: 0,
    refuse: { status: 403, code: "TIER_LOCKED" },
    revokes: [],
  };
  await open(page, "kitchen", "manager-token", false, kds);
  await page.getByRole("button", { name: en["hub.settings.kitchen.pair"] }).click();
  await expect(page.getByTestId("pair-kitchen-failed")).toContainText(
    en["hub.settings.kitchen.locked"],
  );
});

test("a counter holds no device.manage: the pair button says so and opens nothing", async ({
  page,
}) => {
  await open(page, "kitchen", "counter-token");
  await expect(page.getByRole("button", { name: en["hub.settings.kitchen.pair"] })).toHaveAttribute(
    "aria-disabled",
    "true",
  );
  await expect(page.getByText(en["hub.settings.kitchen.pair_only_managers"]).first()).toBeVisible();
});

test("the display list shows name, connected state and last seen, and Revoke removes one", async ({
  page,
}) => {
  const kds: Kds = {
    displays: [{ ...DISPLAY_A }, { ...DISPLAY_B }],
    codes: 0,
    discards: 0,
    revokes: [],
  };
  await open(page, "kitchen", "manager-token", false, kds);
  const pass = page.getByTestId(`kitchen-display-${DISPLAY_A.id}`);
  await expect(pass).toContainText("Pass");
  await expect(pass.getByTestId("kitchen-display-state")).toContainText(
    en["hub.settings.kitchen.display_connected"],
  );
  await expect(pass).toContainText("Last seen");
  const grill = page.getByTestId(`kitchen-display-${DISPLAY_B.id}`);
  await expect(grill.getByTestId("kitchen-display-state")).toContainText(
    en["hub.settings.kitchen.display_disconnected"],
  );
  await expect(grill).toContainText(en["hub.settings.kitchen.display_never_seen"]);
  await expectNoSeriousAxe(page);

  await grill.getByRole("button", { name: en["hub.settings.kitchen.revoke"] }).click();
  await expect(page.getByTestId("kitchen-revoke-status")).toHaveText("Grill was revoked.");
  expect(kds.revokes).toEqual([{ display_id: DISPLAY_B.id, base_rev: 1 }]);
});
