import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type {
  SessionStaff,
  SnapshotBill,
  SnapshotDevice,
  SnapshotSession,
} from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { CORS, HUB, device, json, snapshot } from "./floor-fixture";

const STAFF: SessionStaff = {
  staff_id: "01JPAUSER000000000000000000",
  display_name: "Counter",
  role_name: "Counter",
  permissions: ["session.start", "session.end", "session.pause", "device.screen"],
};
const NO_PAUSE: SessionStaff = {
  ...STAFF,
  permissions: ["session.start", "session.end", "device.screen"],
};

type Sent = {
  payload: Record<string, unknown> & { kind: string };
  key: string;
  rev?: number;
  ticket?: string;
};
type Hub = {
  devices: SnapshotDevice[];
  sessions: SnapshotSession[];
  bills: SnapshotBill[];
  sent: Sent[];
  fail?: { status: number; code: string };
};

const PIN = "2222";
const BUSY = device(1, { status: "in_session" });
const HOUR = 3_600_000;
const running = (): SnapshotSession => ({
  id: "S1",
  device_id: BUSY.device_id,
  state: "active",
  started_at: Date.now() - HOUR,
  paused_total_ms: 0,
  rev: 3,
  guest_no: 1,
});
const paused = (): SnapshotSession => ({
  ...running(),
  state: "paused",
  paused_at: Date.now() - 20 * 60_000,
  paused_total_ms: 10 * 60_000,
  pause_reason: "hardware_issue",
});
const bill = (): SnapshotBill => ({
  session_id: "S1",
  device_id: BUSY.device_id,
  lines: [
    {
      device_id: BUSY.device_id,
      from_at: Date.now() - HOUR,
      to_at: Date.now(),
      minutes: 30,
      rate_minor_per_hour: 10000,
      amount_minor: 5000,
    },
  ],
  subtotal_minor: 5000,
  paid_minor: 0,
  remaining_minor: 5000,
});

async function open(page: Page, session: SnapshotSession, staff = STAFF, fail?: Hub["fail"]) {
  const hub: Hub = {
    devices: [BUSY, device(2, { status: "online" })],
    sessions: [session],
    bills: [bill()],
    sent: [],
    fail,
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
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot": {
        const snap = snapshot(hub.devices);
        snap.sessions = hub.sessions;
        snap.bills = hub.bills;
        return json(route, snap);
      }
      case "/ui/me":
        return json(route, staff);
      case "/ui/settings":
        return json(route, []);
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
          intent: { payload: Sent["payload"]; idempotency_key: string; base_rev?: number };
          elevation_ticket?: string;
        };
        hub.sent.push({
          payload: body.intent.payload,
          key: body.intent.idempotency_key,
          rev: body.intent.base_rev,
          ticket: body.elevation_ticket,
        });
        if (hub.fail) {
          return route.fulfill({
            status: hub.fail.status,
            contentType: "application/json",
            headers: CORS,
            body: JSON.stringify({ message: "no", ...hub.fail }),
          });
        }
        const p = body.intent.payload;
        const now = Date.now();
        hub.sessions = hub.sessions.map((s) =>
          p.kind === "session.pause"
            ? {
                ...s,
                state: "paused",
                paused_at: now,
                rev: s.rev + 1,
                pause_reason: p.reason as SnapshotSession["pause_reason"],
                pause_note: (p.note as string | undefined) ?? null,
              }
            : {
                ...s,
                state: "active",
                paused_at: null,
                paused_total_ms: s.paused_total_ms + (now - (s.paused_at ?? now)),
                rev: s.rev + 1,
                pause_reason: null,
                pause_note: null,
              },
        );
        await json(route, {
          applied: {
            intent_id: "I",
            changes: [{ epoch: "1:H", seq: 9, tbl: "sessions", row_id: "S1", op: "update" }],
          },
        });
        await page.evaluate(() =>
          (window as unknown as { __emit: (n: string, d: unknown) => void }).__emit(
            "devices.changed",
            {},
          ),
        );
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
const button = (page: Page, key: keyof typeof en) =>
  sheet(page).getByRole("button", { name: en[key], exact: true });
const status = (page: Page) => page.getByTestId("unlock-status");
async function openSheet(page: Page, label = "PC 1") {
  await tile(page, label).locator("[data-tile-face]").focus();
  await page.keyboard.press("Enter");
  await expect(sheet(page)).toHaveAccessibleName(label);
}
const serious = async (page: Page) =>
  (await new AxeBuilder({ page }).analyze()).violations.filter(
    (v) => v.impact === "serious" || v.impact === "critical",
  );

test("Pause asks for a reason in place and sends session.pause with the session rev", async ({
  page,
}) => {
  const hub = await open(page, running());
  await openSheet(page);
  await expect(button(page, "hub.session.resume")).toHaveCount(0);
  await button(page, "hub.session.pause").click();
  const picker = sheet(page).locator("[data-component=reason-picker]");
  await expect(picker).toBeVisible();
  await expect(picker.getByRole("radio")).toHaveCount(4);
  expect(await serious(page)).toEqual([]);
  await picker.getByLabel(en["hub.pause.reason.hardware_issue"]).check();
  await picker.getByRole("button", { name: en["ui.reason.confirm"] }).click();
  await expect(status(page)).toHaveText(en["hub.sheet.done.paused"]);
  expect(hub.sent).toEqual([
    {
      payload: { kind: "session.pause", session_id: "S1", reason: "hardware_issue" },
      key: hub.sent[0].key,
      rev: 3,
      ticket: undefined,
    },
  ]);
  const t = tile(page, "PC 1");
  await expect(t.locator("[data-component=status-badge]")).toContainText(en["ui.status.paused"]);
  await expect(t.locator("[data-component=status-badge]")).toContainText("⏸");
  await expect(t.getByTestId("pause-line")).toHaveText(en["hub.pause.reason.hardware_issue"]);
  await expect(button(page, "hub.session.resume")).toBeVisible();
  await expect(button(page, "hub.session.pause")).toHaveCount(0);
  await expect(button(page, "hub.session.end")).toBeVisible();
  await expect(page.getByTestId("sheet-pause")).toHaveText(en["hub.pause.reason.hardware_issue"]);
  expect(await serious(page)).toEqual([]);
});

test("Other needs a note and the note is the tile's secondary line", async ({ page }) => {
  const hub = await open(page, running());
  await openSheet(page);
  await button(page, "hub.session.pause").click();
  const picker = sheet(page).locator("[data-component=reason-picker]");
  await expect(picker.getByLabel(en["hub.pause.note"])).toHaveCount(0);
  await picker.getByLabel(en["hub.pause.reason.other"]).check();
  const note = picker.getByLabel(en["hub.pause.note"]);
  await expect(note).toBeVisible();
  await picker.getByRole("button", { name: en["ui.reason.confirm"] }).click();
  await expect(picker).toContainText(en["ui.reason.note_required"]);
  await expect(note).toBeFocused();
  expect(hub.sent).toHaveLength(0);
  expect(await serious(page)).toEqual([]);
  await note.fill("  Fan noise ");
  await picker.getByRole("button", { name: en["ui.reason.confirm"] }).click();
  await expect(status(page)).toHaveText(en["hub.sheet.done.paused"]);
  expect(hub.sent[0].payload).toEqual({
    kind: "session.pause",
    session_id: "S1",
    reason: "other",
    note: "Fan noise",
  });
  await expect(tile(page, "PC 1").getByTestId("pause-line")).toHaveText("Fan noise");
});

test("Esc closes the reason picker and returns to the Pause button", async ({ page }) => {
  const hub = await open(page, running());
  await openSheet(page);
  await button(page, "hub.session.pause").click();
  await page.keyboard.press("Escape");
  await expect(sheet(page).locator("[data-component=reason-picker]")).toHaveCount(0);
  await expect(button(page, "hub.session.pause")).toBeFocused();
  await expect(sheet(page)).toBeVisible();
  expect(hub.sent).toHaveLength(0);
});

test("a paused tile freezes its timer, shows the reason and Resume sends session.resume", async ({
  page,
}) => {
  await page.clock.install();
  const hub = await open(page, paused());
  await openSheet(page);
  const t = tile(page, "PC 1");
  await expect(t.locator("[data-component=status-badge]")).toContainText(en["ui.status.paused"]);
  await expect(t.getByTestId("pause-line")).toHaveText(en["hub.pause.reason.hardware_issue"]);
  const time = t.locator(".time");
  await expect(time).toHaveText("0:30:00");
  await page.clock.runFor(5_000);
  await expect(time).toHaveText("0:30:00");
  await button(page, "hub.session.resume").click();
  await expect(status(page)).toHaveText(en["hub.sheet.done.resumed"]);
  expect(hub.sent).toEqual([
    {
      payload: { kind: "session.resume", session_id: "S1" },
      key: hub.sent[0].key,
      rev: 3,
      ticket: undefined,
    },
  ]);
  await expect(t.locator("[data-component=status-badge]")).toContainText(
    en["ui.status.in_session"],
  );
  await expect(t.getByTestId("session-line")).toBeVisible();
  await expect(t.getByTestId("pause-line")).toHaveCount(0);
  await expect(button(page, "hub.session.pause")).toBeVisible();
  await page.clock.runFor(3_000);
  await expect(time).not.toHaveText("0:30:00");
});

test("End stays available on a paused session", async ({ page }) => {
  await open(page, paused());
  await openSheet(page);
  await expect(button(page, "hub.session.end")).toBeVisible();
  const box = (await button(page, "hub.session.resume").boundingBox())!;
  expect(box.height).toBeGreaterThanOrEqual(44);
});

test("a stale Pause is refused inline and the tile stays in session", async ({ page }) => {
  await open(page, running(), STAFF, { status: 409, code: "STALE" });
  await openSheet(page);
  await button(page, "hub.session.pause").click();
  const picker = sheet(page).locator("[data-component=reason-picker]");
  await picker.getByLabel(en["hub.pause.reason.power_cut"]).check();
  await picker.getByRole("button", { name: en["ui.reason.confirm"] }).click();
  await expect(status(page)).toContainText(en["hub.session.pause_stale"]);
  await expect(tile(page, "PC 1").getByTestId("session-line")).toBeVisible();
});

test("a refused Resume says why and the tile stays paused", async ({ page }) => {
  await open(page, paused(), STAFF, { status: 422, code: "VALIDATION_FAILED" });
  await openSheet(page);
  await button(page, "hub.session.resume").click();
  await expect(status(page)).toContainText(en["hub.session.resume_refused"]);
  await expect(tile(page, "PC 1").getByTestId("pause-line")).toBeVisible();
});

test("without session.pause a manager PIN approves the Pause", async ({ page }) => {
  const hub = await open(page, running(), NO_PAUSE);
  await openSheet(page);
  await button(page, "hub.session.pause").click();
  const picker = sheet(page).locator("[data-component=reason-picker]");
  await picker.getByLabel(en["hub.pause.reason.player_stepped_away"]).check();
  await picker.getByRole("button", { name: en["ui.reason.confirm"] }).click();
  await page.keyboard.type(PIN);
  await page.keyboard.press("Enter");
  await expect(status(page)).toHaveText(en["hub.sheet.done.paused"]);
  expect(hub.sent).toHaveLength(1);
  expect(hub.sent[0].ticket).toBe("T1");
  expect(hub.sent[0].payload.reason).toBe("player_stepped_away");
});

test("Pause and Resume work by keyboard", async ({ page }) => {
  const hub = await open(page, running());
  await openSheet(page);
  await button(page, "hub.session.pause").focus();
  await page.keyboard.press("Enter");
  await page.getByLabel(en["hub.pause.reason.power_cut"]).focus();
  await page.keyboard.press("Space");
  await page.keyboard.press("Enter");
  await expect(status(page)).toHaveText(en["hub.sheet.done.paused"]);
  await button(page, "hub.session.resume").focus();
  await page.keyboard.press("Enter");
  await expect(status(page)).toHaveText(en["hub.sheet.done.resumed"]);
  expect(hub.sent.map((s) => s.payload.kind)).toEqual(["session.pause", "session.resume"]);
});
