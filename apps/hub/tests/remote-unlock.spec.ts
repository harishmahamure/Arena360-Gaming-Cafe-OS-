import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type { AuditDetail, AuditItem, HubSnapshot, SessionStaff } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { ORIGIN } from "./origin";

// Story 1.20 on the Hub, launched from the tile sheet since Story 3.5: "Admin unlock" (an online
// paired kiosk, the kiosk's ack over `/ui/events` within 5 s, "Failed — retry" otherwise and at
// once for an offline one, a Counter approved by a manager's PIN in place), and the audit
// viewer's admin-exit summary with its "Flagged for review" marker. The Hub's loopback API is stubbed with `page.route`.

const HUB = "http://127.0.0.1:7409";
const CORS = {
  "access-control-allow-origin": ORIGIN,
  "access-control-allow-methods": "GET, POST",
  "access-control-allow-headers": "authorization, content-type, x-arena-elevation",
};
const KIOSK = "01JDEVKIOSK0000000000000000";
const EXIT = "01JEXIT0000000000000000000";
const STAFF: Record<string, SessionStaff> = {
  "manager-token": {
    staff_id: "01JMANAGER0000000000000000",
    display_name: "Manager",
    role_name: "Manager",
    permissions: ["device.remote", "audit.view"],
  },
  "counter-token": {
    staff_id: "01JCOUNTER0000000000000000",
    display_name: "Counter",
    role_name: "Counter",
    permissions: ["session.start"],
  },
};

const snapshot = (online: boolean): HubSnapshot => ({
  branch: { name: "Dev Branch", branch_code: "DEV01" },
  groups: [{ id: "G1", name: "Gaming", kind: "pc", colour: "teal", sort_order: 0, rev: 1 }],
  zones: [{ id: "Z1", name: "Main hall", sort_order: 0, rev: 1 }],
  devices: [
    {
      device_id: KIOSK,
      label: "Kiosk 3",
      class: "paired",
      zone: "Z1",
      online,
      kind: "pc",
      group_id: "G1",
      floor_x: 0,
      floor_y: 0,
      status: online ? "online" : "maintenance",
    },
  ],
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

type Stub = { online: boolean; ack: boolean; intents: unknown[]; elevations: unknown[] };

const ITEM: AuditItem = {
  epoch: "1:01JHUB",
  seq: 7,
  at: Date.UTC(2026, 9, 21, 6, 30),
  business_day: null,
  actor: { kind: "device", id: KIOSK, name: "Kiosk 3" },
  elevated_by: {
    kind: "pin",
    id: "01JMANAGER0000000000000000",
    name: "Manager",
    mode: "cached_verifier",
  },
  module: "device",
  intent_kind: "device.admin_exit",
  intent_id: "I7",
  tbl: "device_admin_exits",
  row_id: EXIT,
  op: "insert",
  flagged: true,
};

const DETAIL: AuditDetail = {
  item: ITEM,
  before: null,
  after: { exit_id: EXIT, mode: "cached_verifier", flagged: 1 },
  changed: [],
  admin_exit: {
    device_id: KIOSK,
    device_label: "Kiosk 3",
    staff_id: "01JMANAGER0000000000000000",
    staff_name: "Manager",
    mode: "cached_verifier",
    started_at: ITEM.at,
    relock_at: ITEM.at + 15 * 60_000,
    relocked_at: null,
    flagged: true,
    flag_reason: "stale_verifier",
  },
};

async function stub(page: Page, token: string, s: Stub) {
  await page.addInitScript((t) => sessionStorage.setItem("arena.hub.token", t), token);
  let streamed = false;
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    const url = new URL(req.url());
    switch (url.pathname) {
      case "/ui/snapshot":
        return json(route, 200, snapshot(s.online));
      case "/ui/me":
        return json(route, 200, STAFF[token]);
      case "/ui/settings":
        return json(route, 200, []);
      case "/ui/elevate": {
        const body = req.postDataJSON() as { items: unknown[]; pin: string };
        s.elevations.push(body);
        if (body.pin !== "2222") {
          return json(route, 403, { code: "PIN_REJECTED", message: "no" });
        }
        return json(route, 200, { ticket: "T1", items: body.items, expires_in_s: 60 });
      }
      case "/ui/intent": {
        s.intents.push(req.postDataJSON());
        return json(route, 200, {
          applied: {
            intent_id: "I",
            changes: [
              { epoch: "1:01JHUB", seq: 9, tbl: "device_admin_exits", row_id: EXIT, op: "insert" },
            ],
          },
        });
      }
      case "/ui/events": {
        if (streamed) return route.abort();
        streamed = true;
        const ack = s.ack
          ? `event: device.admin_unlock_ack\ndata: ${JSON.stringify({ exit_id: EXIT, device_id: KIOSK })}\n\n`
          : "";
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: `retry: 600000\n\n${ack}`,
        });
      }
      case "/ui/audit/facets":
        return json(route, 200, { staff: [], modules: ["device"] });
      case "/ui/audit":
        return json(route, 200, { items: [ITEM], next_cursor: null });
      case "/ui/audit/item":
        return json(route, 200, DETAIL);
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
}

const newStub = (over: Partial<Stub> = {}): Stub => ({
  online: true,
  ack: true,
  intents: [],
  elevations: [],
  ...over,
});

const unlockButton = (page: Page) =>
  page.getByRole("complementary").getByRole("button", { name: en["hub.sheet.admin_unlock"] });
const status = (page: Page) => page.getByTestId("unlock-status");
const openSheet = async (page: Page) => {
  await page.locator("[data-tile-face]").focus();
  await page.keyboard.press("Enter");
};

test("a manager's remote unlock is acknowledged by the kiosk and shows success", async ({
  page,
}) => {
  const s = newStub();
  await stub(page, "manager-token", s);
  await page.goto("/floor");
  await openSheet(page);
  await unlockButton(page).click();
  await expect(status(page)).toHaveText(en["hub.floor.remote_unlock_done"]);
  expect(s.intents).toHaveLength(1);
  const sent = s.intents[0] as { intent: { payload: unknown }; elevation_ticket?: string };
  expect(sent.intent.payload).toEqual({ kind: "device.admin_unlock", device_id: KIOSK });
  expect(sent.elevation_ticket).toBeUndefined();
  const results = await new AxeBuilder({ page }).analyze();
  expect(
    results.violations.filter((v) => v.impact === "serious" || v.impact === "critical"),
  ).toEqual([]);
});

test("no ack within 5 s shows Failed — retry", async ({ page }) => {
  const s = newStub({ ack: false });
  await stub(page, "manager-token", s);
  await page.goto("/floor");
  await openSheet(page);
  await unlockButton(page).click();
  await expect(status(page)).toHaveText(en["hub.floor.remote_unlock_waiting"]);
  await expect(status(page)).toContainText(en["hub.floor.remote_unlock_failed"], {
    timeout: 8000,
  });
});

test("an offline kiosk in maintenance fails at once without asking the Hub", async ({ page }) => {
  const s = newStub({ online: false });
  await stub(page, "manager-token", s);
  await page.goto("/floor");
  await openSheet(page);
  await unlockButton(page).click();
  await expect(status(page)).toContainText(en["hub.floor.remote_unlock_failed"]);
  expect(s.intents).toHaveLength(0);
});

test("a Counter's remote unlock opens the manager PIN in place and carries its ticket", async ({
  page,
}) => {
  const s = newStub();
  await stub(page, "counter-token", s);
  await page.goto("/floor");
  await openSheet(page);
  await expect(page.getByTestId("unlock-needs-approval")).toHaveText(
    en["hub.floor.remote_unlock_needs_approval"],
  );
  await unlockButton(page).click();
  await page.keyboard.type("9999");
  await page.keyboard.press("Enter");
  await expect(page.getByText(en["ui.pin.rejected"])).toBeVisible();
  await page.keyboard.type("2222");
  await page.keyboard.press("Enter");
  await expect(status(page)).toHaveText(en["hub.floor.remote_unlock_done"]);
  expect(s.elevations).toHaveLength(2);
  expect((s.elevations[1] as { items: unknown[] }).items).toEqual([
    { intent_kind: "device.admin_unlock" },
  ]);
  expect((s.intents[0] as { elevation_ticket?: string }).elevation_ticket).toBe("T1");
});

test("the audit viewer names the kiosk, manager, mode and times and flags a stale exit", async ({
  page,
}) => {
  await stub(page, "manager-token", newStub());
  await page.goto("/system/audit");
  const row = page.getByTestId("audit-row").first();
  await expect(row).toContainText(en["hub.audit.intent.device.admin_exit"]);
  await expect(row.getByTestId("audit-flagged")).toContainText(en["hub.audit.flagged"]);
  await row.getByRole("button").click();
  const exit = page.getByTestId("audit-exit");
  await expect(exit).toContainText("Kiosk 3");
  await expect(exit).toContainText("Manager");
  await expect(exit).toContainText(en["hub.audit.exit.mode.cached_verifier"]);
  await expect(exit).toContainText(en["hub.audit.exit.not_relocked"]);
  await expect(page.getByTestId("audit-exit-flagged")).toContainText(en["hub.audit.flagged"]);
});
