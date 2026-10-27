import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type { PlayerHit, SessionStaff, SnapshotDevice, SnapshotSession } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { CORS, HUB, device, json, snapshot } from "./floor-fixture";

const STAFF: Record<string, SessionStaff> = {
  lookup: {
    staff_id: "01JLOOKUP00000000000000000",
    display_name: "Counter",
    role_name: "Counter",
    permissions: ["session.start", "player.view", "device.screen"],
  },
  guest: {
    staff_id: "01JGUESTONLY0000000000000",
    display_name: "Guest only",
    role_name: "Counter",
    permissions: ["session.start", "device.screen"],
  },
  kitchen: {
    staff_id: "01JKITCHEN00000000000000",
    display_name: "Kitchen",
    role_name: "Kitchen",
    permissions: ["device.screen"],
  },
  manager: {
    staff_id: "01JMANAGER0000000000000000",
    display_name: "Manager",
    role_name: "Manager",
    permissions: ["session.start", "player.view", "device.remote", "device.screen"],
  },
};

const PLAYERS: PlayerHit[] = [
  { player_id: "P1", name: "Asha Rao", username: "asha", phone: "+91 98765 43210", tags: [] },
];

type Sent = { payload: Record<string, unknown> & { kind: string }; key: string; ticket?: string };
type Hub = {
  devices: SnapshotDevice[];
  sessions: SnapshotSession[];
  sent: Sent[];
  queries: string[];
  fail?: { status: number; code: string; message?: string };
  rate: number | null;
  hc: boolean;
  ack: boolean;
};

const PIN = "2222";
const FREE = device(1, { status: "online" });

async function open(page: Page, who: keyof typeof STAFF, over: Partial<Hub> = {}): Promise<Hub> {
  const hub: Hub = {
    devices: [FREE, device(2, { status: "online" })],
    sessions: [],
    sent: [],
    queries: [],
    rate: 10000,
    hc: false,
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
        const snap = snapshot(hub.devices, hub.rate);
        snap.sessions = hub.sessions;
        snap.ui = { idle_lock_minutes: 5, hc: hub.hc };
        return json(route, snap);
      }
      case "/ui/me":
        return json(route, STAFF[who]);
      case "/ui/settings":
        return json(route, []);
      case "/ui/players": {
        const digits = (url.searchParams.get("q") ?? "").replace(/\D/g, "");
        hub.queries.push(digits);
        return json(
          route,
          digits ? PLAYERS.filter((p) => p.phone.replace(/\D/g, "").includes(digits)) : [],
        );
      }
      case "/ui/elevate": {
        const body = req.postDataJSON() as { items: unknown[]; pin: string };
        if (body.pin !== PIN) {
          return route.fulfill({
            status: 403,
            contentType: "application/json",
            headers: CORS,
            body: JSON.stringify({ code: "PIN_REJECTED", message: "no" }),
          });
        }
        return json(route, { ticket: "T1", items: body.items, expires_in_s: 60 });
      }
      case "/ui/intent": {
        const body = req.postDataJSON() as {
          intent: { payload: Sent["payload"]; idempotency_key: string };
          elevation_ticket?: string;
        };
        const sent: Sent = {
          payload: body.intent.payload,
          key: body.intent.idempotency_key,
          ticket: body.elevation_ticket,
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
            changes: [{ epoch: "1:H", seq: 9, tbl: "sessions", row_id: "S9", op: "insert" }],
          },
        });
        if (sent.payload.kind === "session.start") {
          const id = String(sent.payload.device_id);
          const player = PLAYERS.find((p) => p.player_id === sent.payload.player_id);
          hub.devices = hub.devices.map((d) =>
            d.device_id === id ? { ...d, status: "in_session" } : d,
          );
          hub.sessions.push({
            id: "S9",
            device_id: id,
            state: "active",
            started_at: Date.now() - 65_000,
            player_id: player?.player_id ?? null,
            username: player?.username ?? null,
            guest_no: player ? null : hub.sessions.length + 1,
          });
          await page.evaluate((device_id) => {
            const emit = (window as unknown as { __emit: (n: string, d: unknown) => void }).__emit;
            emit("session.started", { session_id: "S9", device_id });
            emit("devices.changed", {});
          }, id);
          if (hub.ack && !hub.devices.find((d) => d.device_id === id)?.manual) {
            await page.evaluate(
              ([cmd_id, device_id]) =>
                (window as unknown as { __emit: (n: string, d: unknown) => void }).__emit(
                  "device.pc_ack",
                  { cmd_id, device_id },
                ),
              [sent.key, id],
            );
          }
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
const start = (page: Page) => act(page, en["hub.session.start"]);
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

test("a free PC sheet offers lookup, Guest, the rate and a Start that waits for a pick", async ({
  page,
}) => {
  await open(page, "lookup");
  await openSheet(page);
  const area = page.getByTestId("session-start");
  await expect(area.getByTestId("player-lookup")).toBeVisible();
  await expect(act(page, "Guest")).toBeVisible();
  await expect(page.getByTestId("start-rate")).toHaveText("Rate ₹100.00/hr");
  await expect(start(page)).toHaveAttribute("aria-disabled", "true");
  await act(page, "Guest").click();
  await expect(page.getByTestId("start-picked")).toHaveText(en["hub.players.lookup.guest_picked"]);
  await expect(start(page)).not.toHaveAttribute("aria-disabled", "true");
  const box = (await start(page).boundingBox())!;
  expect(box.height).toBeGreaterThanOrEqual(44);
});

test("a guest start sends session.start postpaid and the tile shows In session, elapsed and Guest 1", async ({
  page,
}) => {
  const hub = await open(page, "lookup");
  await openSheet(page);
  await act(page, "Guest").click();
  await start(page).click();
  await expect(status(page)).toHaveText(en["hub.sheet.done.started"]);
  expect(hub.sent.map((s) => s.payload)).toEqual([
    { kind: "session.start", device_id: FREE.device_id, mode: "postpaid" },
  ]);
  await expect(tile(page, "PC 1").getByTestId("device-presence")).toHaveText(
    en["ui.status.in_session"],
  );
  await expect(tile(page, "PC 1")).toContainText(/0:01:\d\d/);
  await expect(tile(page, "PC 1").getByTestId("session-line")).toHaveText("Guest 1");
  await expect(page.getByTestId("session-start")).toHaveCount(0);
});

test("no kiosk ack in 5 s fails the start, and Retry resends the same idempotency key", async ({
  page,
}) => {
  const hub = await open(page, "lookup", { ack: false });
  await openSheet(page);
  await act(page, "Guest").click();
  await start(page).click();
  await expect(status(page)).toContainText(en["hub.floor.remote_unlock_failed"], {
    timeout: 8000,
  });
  hub.ack = true;
  await act(page, en["hub.sheet.retry"]).click();
  await expect(status(page)).toHaveText(en["hub.sheet.done.started"]);
  expect(hub.sent).toHaveLength(2);
  expect(hub.sent[1].key).toBe(hub.sent[0].key);
});

test("a looked-up player is started by id and the tile names the username", async ({ page }) => {
  const hub = await open(page, "lookup");
  await openSheet(page);
  await sheet(page).getByLabel(en["hub.players.lookup.label"]).fill("98765");
  await page
    .getByTestId("player-hits")
    .getByRole("button", { name: /Asha Rao/ })
    .click();
  await expect(page.getByTestId("start-picked")).toHaveText("Player: Asha Rao");
  await start(page).click();
  await expect(status(page)).toHaveText(en["hub.sheet.done.started"]);
  expect(hub.sent[0].payload).toEqual({
    kind: "session.start",
    device_id: FREE.device_id,
    mode: "postpaid",
    player_id: "P1",
  });
  await expect(tile(page, "PC 1").getByTestId("session-line")).toHaveText("asha");
  expect(hub.queries.at(-1)).toBe("98765");
});

test("without player.view only Guest is offered", async ({ page }) => {
  await open(page, "guest");
  await openSheet(page);
  await expect(page.getByTestId("player-lookup")).toHaveCount(0);
  await expect(act(page, "Guest")).toBeVisible();
});

test("the whole start works by keyboard", async ({ page }) => {
  const hub = await open(page, "guest");
  await openSheet(page);
  await act(page, "Guest").focus();
  await page.keyboard.press("Enter");
  await start(page).focus();
  await page.keyboard.press("Enter");
  await expect(status(page)).toHaveText(en["hub.sheet.done.started"]);
  expect(hub.sent).toHaveLength(1);
});

for (const [code, text] of [
  ["PC_BUSY", en["hub.session.busy"]],
  ["PC_UNAVAILABLE", en["hub.session.unavailable"]],
  ["NO_RATE", en["hub.session.no_rate"]],
] as const) {
  test(`${code} is refused inline in plain words and the tile stays free`, async ({ page }) => {
    await open(page, "lookup", { fail: { status: 409, code } });
    await openSheet(page);
    await act(page, "Guest").click();
    await start(page).click();
    await expect(status(page)).toContainText(text);
    await expect(tile(page, "PC 1").getByTestId("device-presence")).toHaveText(
      en["hub.floor.online"],
    );
  });
}

test("DEVICE_UNASSIGNED offers Assign to group, which lands on the assign form", async ({
  page,
}) => {
  const hub = await open(page, "lookup", { fail: { status: 409, code: "DEVICE_UNASSIGNED" } });
  await openSheet(page);
  await act(page, "Guest").click();
  await start(page).click();
  await expect(status(page)).toContainText(en["hub.session.unassigned"]);
  hub.devices = hub.devices.map((d) =>
    d.device_id === FREE.device_id ? { ...d, group_id: null, zone: null } : d,
  );
  await page.evaluate(() =>
    (window as unknown as { __emit: (n: string, d: unknown) => void }).__emit(
      "devices.changed",
      {},
    ),
  );
  await expect(page.getByTestId("sheet-assign")).toBeVisible();
  await act(page, "Assign to group").first().click();
  await expect(sheet(page).getByRole("combobox")).toBeFocused();
});

test("Restart on a PC in session is refused with PC_BUSY inline", async ({ page }) => {
  const busy = device(1, { status: "in_session" });
  await open(page, "manager", {
    devices: [busy],
    sessions: [
      { id: "S1", device_id: busy.device_id, state: "active", started_at: Date.now() - 1000 },
    ],
    fail: { status: 409, code: "PC_BUSY" },
  });
  await openSheet(page);
  await act(page, "Restart").click();
  await expect(status(page)).toContainText(en["hub.session.power_busy"]);
  await expect(act(page, "Shut down")).toBeVisible();
});

test("a role without session.start gets the manager PIN in place", async ({ page }) => {
  const hub = await open(page, "kitchen");
  await openSheet(page);
  await act(page, "Guest").click();
  await start(page).click();
  await page.keyboard.type(PIN);
  await page.keyboard.press("Enter");
  await expect(status(page)).toHaveText(en["hub.sheet.done.started"]);
  expect(hub.sent[0].ticket).toBe("T1");
});

for (const hc of [false, true]) {
  test(`axe: the start area has no serious violations (${hc ? "HC" : "dark"})`, async ({
    page,
  }) => {
    await open(page, "lookup", { hc });
    await openSheet(page);
    await act(page, "Guest").click();
    await expect(start(page)).toBeVisible();
    await clean(page);
  });
}
