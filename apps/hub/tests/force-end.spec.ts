import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type { SessionStaff, SnapshotDevice, SnapshotSession } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { CORS, HUB, device, json, snapshot } from "./floor-fixture";

const COUNTER: SessionStaff = {
  staff_id: "01JFORCEUSER00000000000000",
  display_name: "Counter",
  role_name: "Counter",
  permissions: ["session.start", "session.end", "session.pause", "device.screen"],
};
const MANAGER: SessionStaff = {
  ...COUNTER,
  display_name: "Manager",
  role_name: "Manager",
  permissions: [...COUNTER.permissions, "session.force_end"],
};

type Sent = {
  payload: Record<string, unknown> & { kind: string };
  reason?: string;
  rev?: number;
  ticket?: string;
};
type Hub = { sessions: SnapshotSession[]; sent: Sent[]; devices: SnapshotDevice[] };

const PIN = "2222";
const BUSY = device(1, { status: "in_session" });
const HOUR = 3_600_000;
const session = (mode: "postpaid" | "pack"): SnapshotSession => ({
  id: "S1",
  device_id: BUSY.device_id,
  state: "active",
  started_at: Date.now() - HOUR,
  paused_total_ms: 0,
  rev: 3,
  player_id: mode === "pack" ? "P1" : undefined,
  username: mode === "pack" ? "asha" : undefined,
  guest_no: mode === "pack" ? undefined : 1,
  mode,
  ends_at: mode === "pack" ? Date.now() + HOUR : undefined,
});

async function open(page: Page, running: SnapshotSession, staff: SessionStaff) {
  const hub: Hub = {
    sessions: [running],
    sent: [],
    devices: [BUSY, device(2, { status: "online" })],
  };
  await page.addInitScript(() => {
    sessionStorage.setItem("arena.hub.token", "tok");
    class FakeEventSource {
      static CLOSED = 2;
      readyState = 1;
      addEventListener() {}
      close() {}
    }
    (window as unknown as Record<string, unknown>).EventSource = FakeEventSource;
  });
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot": {
        const snap = snapshot(hub.devices);
        snap.sessions = hub.sessions;
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
          intent: { payload: Sent["payload"]; base_rev?: number; reason?: string };
          elevation_ticket?: string;
        };
        const p = body.intent.payload;
        hub.sent.push({
          payload: p,
          reason: body.intent.reason,
          rev: body.intent.base_rev,
          ticket: body.elevation_ticket,
        });
        const applied = {
          intent_id: "I",
          changes: [{ epoch: "1:H", seq: 9, tbl: "sessions", row_id: "S1", op: "update" }],
        };
        if (p.kind === "session.force_end" || p.kind === "session.end") {
          hub.sessions = [];
          return json(route, {
            applied,
            ended: {
              session_id: "S1",
              order_id: "O1",
              ended_at: Date.now(),
              undo_until: Date.now() + 8000,
              session_rev: 4,
              total_minor: 0,
              irreversible: false,
            },
          });
        }
        hub.sessions = [running];
        return json(route, { applied });
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
const picker = (page: Page) => sheet(page).locator("[data-component=reason-picker]");
async function openSheet(page: Page) {
  await tile(page, "PC 1").locator("[data-tile-face]").focus();
  await page.keyboard.press("Enter");
  await expect(sheet(page)).toHaveAccessibleName("PC 1");
}
const confirm = (page: Page) =>
  picker(page).getByRole("button", { name: en["ui.reason.confirm"] }).click();

test("End on a paid session with time left asks for a reason and sends session.force_end", async ({
  page,
}) => {
  const hub = await open(page, session("pack"), MANAGER);
  await openSheet(page);
  await expect(button(page, "hub.session.force_end")).toHaveCount(0);
  await button(page, "hub.session.end").click();
  await expect(picker(page)).toBeVisible();
  await expect(picker(page).getByRole("radio")).toHaveCount(4);
  for (const key of [
    "reason.rule_violation",
    "reason.hardware_issue",
    "reason.closing",
    "reason.other",
  ] as const) {
    await expect(picker(page).getByLabel(en[key])).toBeVisible();
  }
  expect(
    (await new AxeBuilder({ page }).analyze()).violations.filter(
      (v) => v.impact === "serious" || v.impact === "critical",
    ),
  ).toEqual([]);
  await confirm(page);
  await expect(picker(page)).toContainText(en["ui.reason.required"]);
  expect(hub.sent).toHaveLength(0);
  await picker(page).getByLabel(en["reason.other"]).check();
  await confirm(page);
  await expect(picker(page)).toContainText(en["ui.reason.note_required"]);
  await picker(page).getByLabel(en["hub.force.note"]).fill("  Fighting  ");
  await confirm(page);
  await expect(page.getByTestId("undo-message")).toHaveText("Session ended early · ₹0.00");
  expect(hub.sent).toEqual([
    {
      payload: { kind: "session.force_end", session_id: "S1", note: "Fighting" },
      reason: "other",
      rev: 3,
      ticket: undefined,
    },
  ]);
});

test("without session.force_end a manager's PIN approves the early end", async ({ page }) => {
  const hub = await open(page, session("pack"), COUNTER);
  await openSheet(page);
  await button(page, "hub.session.end").click();
  await picker(page).getByLabel(en["reason.closing"]).check();
  await confirm(page);
  await page.keyboard.type("0000");
  await page.keyboard.press("Enter");
  await expect(sheet(page)).toContainText(en["ui.pin.rejected"]);
  expect(hub.sent).toHaveLength(0);
  await page.keyboard.type(PIN);
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("undo-message")).toHaveText("Session ended early · ₹0.00");
  expect(hub.sent).toHaveLength(1);
  expect(hub.sent[0].ticket).toBe("T1");
  expect(hub.sent[0].reason).toBe("closing");
  expect(hub.sent[0].payload.kind).toBe("session.force_end");
});

test("Undo of an early end sends session.force_end.reverted and needs the same approval", async ({
  page,
}) => {
  const hub = await open(page, session("pack"), COUNTER);
  await openSheet(page);
  await button(page, "hub.session.end").click();
  await picker(page).getByLabel(en["reason.hardware_issue"]).check();
  await confirm(page);
  await page.keyboard.type(PIN);
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("undo-message")).toBeVisible();
  await page.getByTestId("undo-action").click();
  await page.keyboard.type(PIN);
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("undo-notice")).toHaveText(en["hub.undo.done.forced"]);
  const undone = hub.sent.at(-1)!;
  expect(undone.payload).toEqual({ kind: "session.force_end.reverted", session_id: "S1" });
  expect(undone.rev).toBe(4);
  expect(undone.ticket).toBe("T1");
  await expect(tile(page, "PC 1").getByTestId("session-line")).toBeVisible();
});

test("a postpaid session ends as before, with Force end beside it", async ({ page }) => {
  const hub = await open(page, session("postpaid"), COUNTER);
  await openSheet(page);
  await button(page, "hub.session.end").click();
  await expect(picker(page)).toHaveCount(0);
  await expect(page.getByTestId("undo-message")).toContainText("Session ended");
  expect(hub.sent.map((s) => s.payload.kind)).toEqual(["session.end"]);
});

test("Force end on a postpaid session opens the reason picker", async ({ page }) => {
  await open(page, session("postpaid"), COUNTER);
  await openSheet(page);
  await button(page, "hub.session.force_end").click();
  await expect(picker(page)).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(picker(page)).toHaveCount(0);
});
