import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type {
  BillLine,
  SessionStaff,
  SnapshotBill,
  SnapshotDevice,
  SnapshotGroupBill,
  SnapshotSession,
} from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { CORS, HUB, device, json, snapshot } from "./floor-fixture";

const COUNTER: SessionStaff = {
  staff_id: "01JCOUNTER0000000000000000",
  display_name: "Counter",
  role_name: "Counter",
  permissions: ["session.start", "session.end", "session.pause", "pos.sell", "player.view"],
};

type Sent = { payload: Record<string, unknown> & { kind: string }; key: string; rev?: number };
type Hub = {
  devices: SnapshotDevice[];
  sessions: SnapshotSession[];
  bills: SnapshotBill[];
  groups: SnapshotGroupBill[];
  sent: Sent[];
  fail?: { status: number; code: string; field: string; message: string };
};

const session = (id: string, n: number, extra: Partial<SnapshotSession> = {}): SnapshotSession => ({
  id,
  device_id: device(n, {}).device_id,
  state: "active",
  started_at: Date.now() - 600_000,
  paused_total_ms: 0,
  rev: 3,
  player_id: null,
  username: null,
  guest_no: n,
  mode: "postpaid",
  ...extra,
});

const line = (n: number, amount: number): BillLine => ({
  device_id: device(n, {}).device_id,
  from_at: Date.now() - 600_000,
  to_at: Date.now(),
  minutes: 30,
  rate_minor_per_hour: amount * 2,
  amount_minor: amount,
});

const bill = (id: string, n: number, amount: number, group?: string): SnapshotBill => ({
  session_id: id,
  device_id: device(n, {}).device_id,
  lines: [line(n, amount)],
  subtotal_minor: amount,
  paid_minor: 0,
  remaining_minor: amount,
  group_bill_id: group,
});

async function open(page: Page, over: Partial<Hub> = {}): Promise<Hub> {
  const hub: Hub = {
    devices: [
      device(1, { status: "in_session" }),
      device(2, { status: "in_session" }),
      device(3, { status: "online" }),
      device(4, { status: "in_session" }),
      device(5, { status: "offline", online: false }),
    ],
    sessions: [
      session("S1", 1),
      session("S2", 2),
      session("S4", 4, { mode: "pack", player_id: "P1", username: "ravi", guest_no: null }),
    ],
    bills: [bill("S1", 1, 5000), bill("S2", 2, 3000)],
    groups: [],
    sent: [],
    ...over,
  };
  await page.addInitScript(() => sessionStorage.setItem("arena.hub.token", "tok"));
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    const url = new URL(req.url());
    switch (url.pathname) {
      case "/ui/snapshot": {
        const snap = snapshot(hub.devices);
        snap.sessions = hub.sessions;
        snap.bills = hub.bills;
        snap.group_bills = hub.groups;
        return json(route, snap);
      }
      case "/ui/me":
        return json(route, COUNTER);
      case "/ui/settings":
        return json(route, []);
      case "/ui/intent": {
        const body = req.postDataJSON() as {
          intent: { payload: Sent["payload"]; idempotency_key: string; base_rev?: number };
        };
        hub.sent.push({
          payload: body.intent.payload,
          key: body.intent.idempotency_key,
          rev: body.intent.base_rev,
        });
        if (hub.fail) {
          return route.fulfill({
            status: hub.fail.status,
            contentType: "application/json",
            headers: CORS,
            body: JSON.stringify(hub.fail),
          });
        }
        const payload = body.intent.payload;
        if (payload.kind === "session.group") {
          const ids = [String(payload.session_id), ...(payload.member_ids as string[])];
          hub.groups = [
            {
              id: "GB1",
              rev: 2,
              payer_guest_no: 1,
              member_ids: ids,
              ended_lines: [],
            },
          ];
          hub.sessions = hub.sessions.map((s) =>
            ids.includes(s.id) ? { ...s, group_bill_id: "GB1", rev: s.rev + 1 } : s,
          );
        }
        return json(route, {
          applied: {
            intent_id: "I",
            changes: [{ epoch: "1:H", seq: 9, tbl: "sessions", row_id: "S1", op: "update" }],
          },
        });
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
async function openSheet(page: Page, label = "PC 1") {
  await tile(page, label).locator("[data-tile-face]").focus();
  await page.keyboard.press("Enter");
  await expect(sheet(page)).toHaveAccessibleName(label);
}

test("Add to group highlights running postpaid sessions and free PCs, not prepaid or offline ones", async ({
  page,
}) => {
  await open(page);
  await openSheet(page);
  await act(page, en["hub.group.add"]).click();
  await expect(page.getByTestId("group-hint")).toHaveText(en["hub.group.hint"]);
  const targets = page.locator("[data-group-target]");
  await expect(targets).toHaveCount(2);
  await expect(tile(page, "PC 2").getByTestId("group-pick-here")).toHaveText(
    en["hub.group.add_here"],
  );
  await expect(tile(page, "PC 3").getByTestId("group-pick-here")).toHaveText(
    en["hub.group.start_here"],
  );
  expect(
    (await new AxeBuilder({ page }).analyze()).violations.filter(
      (v) => v.impact === "serious" || v.impact === "critical",
    ),
  ).toEqual([]);
});

test("picking a running session sends session.group and tiles then name the payer", async ({
  page,
}) => {
  const hub = await open(page);
  await openSheet(page);
  await act(page, en["hub.group.add"]).click();
  await tile(page, "PC 2").locator("[data-tile-face]").click();
  await expect(tile(page, "PC 2").getByTestId("group-marker")).toHaveText(
    en["hub.group.marker"].replace("{payer}", "Guest 1"),
  );
  expect(hub.sent).toHaveLength(1);
  expect(hub.sent[0].payload).toEqual({
    kind: "session.group",
    session_id: "S1",
    member_ids: ["S2"],
  });
  expect(hub.sent[0].rev).toBe(3);
  await expect(page.getByTestId("unlock-status")).toHaveText(en["hub.sheet.done.grouped"]);
  await expect(page.getByTestId("group-hint")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.locator("[data-group-target]")).toHaveCount(0);
});

test("a free PC starts a guest session in the group", async ({ page }) => {
  const hub = await open(page);
  await openSheet(page);
  await act(page, en["hub.group.add"]).click();
  await tile(page, "PC 3").locator("[data-tile-face]").focus();
  await page.keyboard.press("Enter");
  await expect.poll(() => hub.sent.length).toBe(1);
  expect(hub.sent[0].payload).toMatchObject({
    kind: "session.start",
    device_id: device(3, {}).device_id,
    mode: "postpaid",
    group_with: "S1",
  });
});

test("a prepaid session is refused with the reason shown inline and nothing changes", async ({
  page,
}) => {
  const reason = "Only postpaid sessions can share a bill";
  const hub = await open(page, {
    fail: { status: 422, code: "VALIDATION_FAILED", field: "member_ids", message: reason },
  });
  await openSheet(page);
  await act(page, en["hub.group.add"]).click();
  await tile(page, "PC 4").locator("[data-tile-face]").focus();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("unlock-status")).toContainText(reason);
  await expect(page.locator("[data-testid=group-marker]")).toHaveCount(0);
  expect(hub.sent).toHaveLength(1);
});

test("a member's sheet shows each device's subtotal and one group total", async ({ page }) => {
  await open(page, {
    groups: [
      {
        id: "GB1",
        rev: 2,
        payer_guest_no: 1,
        member_ids: ["S1", "S2"],
        ended_lines: [line(3, 2000)],
      },
    ],
    sessions: [
      session("S1", 1, { group_bill_id: "GB1" }),
      session("S2", 2, { group_bill_id: "GB1" }),
    ],
  });
  await openSheet(page);
  await expect(page.getByTestId("bill-device-subtotal")).toHaveCount(3);
  await expect(page.getByTestId("bill-subtotal")).toContainText(en["hub.bill.group_total"]);
  await expect(page.getByTestId("bill-subtotal")).toContainText("100.00");
  await expect(page.getByTestId("group-payer")).toHaveText(
    en["hub.group.payer_line"].replace("{payer}", "Guest 1"),
  );
});

test("changing the payer to another member sends group.payer with the group revision", async ({
  page,
}) => {
  const hub = await open(page, {
    groups: [{ id: "GB1", rev: 2, payer_guest_no: 1, member_ids: ["S1", "S2"], ended_lines: [] }],
    sessions: [
      session("S1", 1, { group_bill_id: "GB1" }),
      session("S2", 2, { group_bill_id: "GB1" }),
    ],
  });
  await openSheet(page);
  await act(page, en["hub.group.payer_change"]).click();
  await page.getByTestId("group-payer-select").selectOption({ index: 2 });
  await act(page, en["hub.group.payer_save"]).click();
  await expect.poll(() => hub.sent.length).toBe(1);
  expect(hub.sent[0].payload).toEqual({ kind: "group.payer", group_bill_id: "GB1", guest_no: 2 });
  expect(hub.sent[0].rev).toBe(2);
});
