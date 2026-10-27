import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type {
  SessionStaff,
  SnapshotAlert,
  SnapshotDevice,
  SnapshotSession,
} from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { CORS, HUB, device, json, snapshot } from "./floor-fixture";

const MIN = 60_000;
const COUNTER: SessionStaff = {
  staff_id: "01JCOUNTER0000000000000000",
  display_name: "Counter",
  role_name: "Counter",
  permissions: ["session.start", "session.end", "device.screen", "device.remote"],
};

const tv = (n: number, extra: Partial<SnapshotDevice> = {}) =>
  device(n, {
    label: `PS5 ${n}`,
    kind: "console",
    group_id: "G2",
    manual: true,
    tv_number: n,
    ...extra,
  });

const session = (n: number, extra: Partial<SnapshotSession>): SnapshotSession => ({
  id: `S${n}`,
  device_id: device(n, {}).device_id,
  state: "active",
  started_at: Date.now() - 20 * MIN,
  paused_total_ms: 0,
  rev: 1,
  guest_no: n,
  ...extra,
});

type Sent = Record<string, unknown> & { kind: string };

async function open(
  page: Page,
  devices: SnapshotDevice[],
  sessions: SnapshotSession[] = [],
  alerts: SnapshotAlert[] = [],
) {
  const sent: Sent[] = [];
  await page.addInitScript(() => sessionStorage.setItem("arena.hub.token", "tok"));
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot": {
        const snap = snapshot(devices);
        snap.groups.push({
          id: "G2",
          name: "Consoles",
          kind: "console",
          colour: "orange",
          sort_order: 1,
          rev: 1,
          rate_minor: 12000,
        });
        snap.sessions = sessions;
        snap.alerts = alerts;
        return json(route, snap);
      }
      case "/ui/me":
        return json(route, COUNTER);
      case "/ui/settings":
        return json(route, []);
      case "/ui/intent": {
        const body = req.postDataJSON() as { intent: { payload: Sent } };
        sent.push(body.intent.payload);
        return json(route, {
          applied: {
            intent_id: "I",
            changes: [{ epoch: "1:H", seq: 9, tbl: "sessions", row_id: "S9", op: "insert" }],
          },
        });
      }
      case "/ui/events":
        return route.abort();
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/floor");
  return sent;
}

const tile = (page: Page, label: string) =>
  page.locator("[data-component=floor-tile]", { hasText: label });
const sheet = (page: Page) => page.getByRole("complementary");
const act = (page: Page, name: string) => sheet(page).getByRole("button", { name, exact: true });

test("a console tile shows a controller glyph, its TV number and the same states as a PC", async ({
  page,
}) => {
  await open(
    page,
    [tv(1, { status: "in_session" }), tv(2), device(3, { status: "in_session" })],
    [
      session(1, { ends_at: Date.now() + 30 * MIN }),
      session(3, { ends_at: Date.now() + 30 * MIN }),
    ],
  );
  await expect(tile(page, "PS5 1").getByTestId("console-glyph")).toBeVisible();
  await expect(tile(page, "PS5 1").getByTestId("tv-number")).toHaveText("TV 1");
  await expect(tile(page, "PS5 1")).toContainText(en["ui.status.in_session"]);
  await expect(tile(page, "PS5 2")).toContainText(en["hub.floor.online"]);
  await expect(tile(page, "PC 3").getByTestId("console-glyph")).toHaveCount(0);
  await expect(tile(page, "PS5 1").getByTestId("tile-warning-text")).toHaveCount(0);
  const results = await new AxeBuilder({ page }).analyze();
  expect(
    results.violations.filter((v) => v.impact === "serious" || v.impact === "critical"),
  ).toEqual([]);
});

test("a running console session warns on its tile at 10, 5 and 1 minutes", async ({ page }) => {
  await open(
    page,
    [
      tv(1, { status: "in_session" }),
      tv(2, { status: "in_session" }),
      tv(3, { status: "in_session" }),
      tv(4, { status: "in_session" }),
      device(5, { status: "in_session" }),
    ],
    [
      session(1, { ends_at: Date.now() + 9 * MIN + 30_000 }),
      session(2, { ends_at: Date.now() + 4 * MIN + 30_000 }),
      session(3, { ends_at: Date.now() + 40_000 }),
      session(4, { ends_at: Date.now() + 20 * MIN }),
      session(5, { ends_at: Date.now() + 9 * MIN }),
    ],
  );
  for (const [label, text] of [
    ["PS5 1", "10 min left"],
    ["PS5 2", "5 min left"],
    ["PS5 3", "1 min left"],
  ]) {
    const t = tile(page, label);
    await expect(t.getByTestId("tile-time-warning")).toBeVisible();
    await expect(t.getByTestId("tile-warning-text")).toHaveText(text);
    await expect(t.locator("[data-tile-face]")).toHaveAttribute("aria-label", new RegExp(text));
  }
  await expect(tile(page, "PS5 4").getByTestId("tile-time-warning")).toHaveCount(0);
  await expect(tile(page, "PC 5").getByTestId("tile-warning-text")).toHaveCount(0);
});

test("a free console starts a session from its sheet, with no remote PC actions", async ({
  page,
}) => {
  const sent = await open(page, [tv(1)]);
  await tile(page, "PS5 1").click({ position: { x: 20, y: 20 } });
  await expect(sheet(page).getByTestId("session-start")).toBeVisible();
  for (const name of ["Unlock", "Message", "Lock", "Restart", "Shut down", "Admin unlock"]) {
    await expect(act(page, name)).toHaveCount(0);
  }
  await expect(sheet(page).getByTestId("sheet-details-only")).toHaveCount(0);
  await act(page, en["hub.players.lookup.guest"]).click();
  await act(page, en["hub.session.start"]).click();
  await expect.poll(() => sent.length).toBe(1);
  expect(sent[0]).toMatchObject({
    kind: "session.start",
    device_id: tv(1).device_id,
    mode: "postpaid",
  });
});

test("a console past its end time shows Time's up with a TIME UP badge until acknowledged", async ({
  page,
}) => {
  await open(
    page,
    [tv(1, { status: "in_session" })],
    [session(1, { state: "expired", ends_at: Date.now() - MIN })],
    [{ id: "A1", kind: "time_up", device_id: tv(1).device_id, raised_at: Date.now() - MIN }],
  );
  const t = tile(page, "PS5 1");
  await expect(t).toContainText(en["hub.floor.times_up"]);
  await expect(t.locator("[data-component=alert-badge]")).toContainText(en["ui.alert.tag.time_up"]);
});

test("an ended console shows CHECK CONTROLLERS and blocks Start until Returned or Missing", async ({
  page,
}) => {
  const sent = await open(page, [tv(1, { controller_check: "S7" })]);
  await expect(tile(page, "PS5 1")).toContainText(en["hub.floor.check_controllers"]);
  await tile(page, "PS5 1").click({ position: { x: 20, y: 20 } });
  const check = sheet(page).getByTestId("controller-check");
  await expect(check).toBeVisible();
  await act(page, en["hub.players.lookup.guest"]).click();
  const start = act(page, en["hub.session.start"]);
  await expect(start).toBeDisabled();
  await expect(sheet(page)).toContainText(en["hub.console.mark_first"]);
  await check.getByRole("button", { name: en["hub.console.missing"], exact: true }).click();
  await expect.poll(() => sent.length).toBe(1);
  expect(sent[0]).toMatchObject({ kind: "controller.check", session_id: "S7", result: "missing" });
  await check.getByRole("button", { name: en["hub.console.returned"], exact: true }).click();
  await expect.poll(() => sent.length).toBe(2);
  expect(sent[1]).toMatchObject({ kind: "controller.check", session_id: "S7", result: "returned" });
});

test("a console with its controllers marked starts a session as usual", async ({ page }) => {
  await open(page, [tv(1)]);
  await tile(page, "PS5 1").click({ position: { x: 20, y: 20 } });
  await expect(sheet(page).getByTestId("controller-check")).toHaveCount(0);
  await expect(tile(page, "PS5 1")).not.toContainText(en["hub.floor.check_controllers"]);
});
