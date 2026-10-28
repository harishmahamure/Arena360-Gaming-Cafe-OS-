import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type { HubSnapshot, SessionStaff } from "@arena/ui/generated";
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
    permissions: ["settings.edit", "user.manage"],
  },
  "counter-token": {
    staff_id: "01JCOUNTER0000000000000000",
    display_name: "Counter",
    role_name: "Counter",
    permissions: ["session.start", "pos.sell"],
  },
};
const KEY = "hub.stopped_at";

type Access = { notifications: boolean; battery_exempt: boolean; asked: boolean };
type Calls = { intents: { value: string; ticket?: string }[]; elevations: unknown[] };

let sessions: { id: string; state: string }[] = [];

function snapshot(paired: boolean): HubSnapshot {
  return {
    sessions: sessions as unknown as HubSnapshot["sessions"],
    branch: paired ? { name: "Dev Branch", branch_code: "DEV01" } : null,
    groups: [],
    zones: [],
    devices: [],
    locale: null,
    ui: { idle_lock_minutes: 5, hc: false },
    connectivity: { state: "online", checked_at: null },
    setup: null,
    lan: null,
    paired,
  };
}

function json(route: Route, status: number, body: unknown) {
  return route.fulfill({
    status,
    contentType: "application/json",
    headers: CORS,
    body: JSON.stringify(body),
  });
}

async function stub(page: Page, token: string | null, paired = true): Promise<Calls> {
  const calls: Calls = { intents: [], elevations: [] };
  if (token) await page.addInitScript((t) => sessionStorage.setItem("arena.hub.token", t), token);
  await page.route(`${HUB}/**`, (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    const url = new URL(req.url());
    const auth = (req.headers()["authorization"] ?? "").replace(/^Bearer /, "");
    switch (url.pathname) {
      case "/ui/snapshot":
        return json(route, 200, snapshot(paired));
      case "/ui/me":
        return STAFF[auth]
          ? json(route, 200, STAFF[auth])
          : json(route, 401, { code: "AUTH_FAILED", message: "no" });
      case "/ui/settings":
        return json(route, 200, [{ key: KEY, value: "", rev: 1 }]);
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: "retry: 60000\n\n",
        });
      case "/ui/elevate": {
        const body = req.postDataJSON() as { pin: string };
        calls.elevations.push(body);
        return body.pin === "2222"
          ? json(route, 200, { ticket: "T1", items: [], expires_in_s: 60 })
          : json(route, 403, { code: "PIN_REJECTED", message: "no" });
      }
      case "/ui/intent": {
        const body = req.postDataJSON() as {
          intent: { payload: { key: string; value: string } };
          elevation_ticket?: string;
        };
        if (body.intent.payload.key !== KEY) return route.fulfill({ status: 404, headers: CORS });
        if (!body.elevation_ticket && !STAFF[auth].permissions.includes("settings.edit")) {
          return json(route, 403, { code: "PERMISSION_DENIED", message: "denied" });
        }
        calls.intents.push({
          value: body.intent.payload.value,
          ticket: body.elevation_ticket,
        });
        return json(route, 200, { applied: { intent_id: "I", changes: [] } });
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  return calls;
}

async function bridge(page: Page, access: Access, update?: string) {
  await page.addInitScript(
    ([a, u]: [Access, string | undefined]) => {
      const w = window as unknown as { __bridge: string[]; __access: Access; ArenaHub: unknown };
      w.__bridge = [];
      w.__access = a;
      const note = (name: string) => () => void w.__bridge.push(name);
      w.ArenaHub = {
        state: () => JSON.stringify(w.__access),
        startHub: note("startHub"),
        stopHub: note("stopHub"),
        requestAccess: note("requestAccess"),
        skipAccess: note("skipAccess"),
        openBatterySettings: note("openBatterySettings"),
        openNotificationSettings: note("openNotificationSettings"),
        ...(u === undefined
          ? {}
          : {
              updateState: () => u,
              checkUpdate: note("checkUpdate"),
              installUpdate: note("installUpdate"),
            }),
      };
    },
    [access, update] as [Access, string | undefined],
  );
}

const bridgeCalls = (page: Page) =>
  page.evaluate(() => (window as unknown as { __bridge: string[] }).__bridge);

const GOOD: Access = { notifications: true, battery_exempt: true, asked: true };
const RESTRICTED: Access = { notifications: true, battery_exempt: false, asked: true };

test.describe("first-run access prompt", () => {
  test("the pairing screen asks once, in one line, and Allow hands off to Android", async ({
    page,
  }) => {
    await bridge(page, { notifications: false, battery_exempt: false, asked: false });
    await stub(page, null, false);
    await page.goto("/");
    const prompt = page.getByTestId("hub-access");
    await expect(prompt).toContainText(en["hub.access.body"]);
    await prompt.getByRole("button", { name: en["hub.access.allow"] }).click();
    expect(await bridgeCalls(page)).toContain("requestAccess");
  });

  test("Not now is remembered and the Hub still pairs", async ({ page }) => {
    await bridge(page, { notifications: false, battery_exempt: false, asked: false });
    await stub(page, null, false);
    await page.goto("/");
    await page.getByRole("button", { name: en["hub.access.skip"] }).click();
    expect(await bridgeCalls(page)).toContain("skipAccess");
    await expect(page.getByRole("heading", { level: 1 })).toHaveText(en["hub.pair.title"]);
  });

  test("an already answered prompt is not shown again", async ({ page }) => {
    await bridge(page, RESTRICTED);
    await stub(page, null, false);
    await page.goto("/");
    await expect(page.getByRole("heading", { level: 1 })).toHaveText(en["hub.pair.title"]);
    await expect(page.getByTestId("hub-access")).toHaveCount(0);
  });
});

test.describe("when Android restricts the Hub", () => {
  test("the header alert badge opens the screen with the button to the system setting", async ({
    page,
  }) => {
    await bridge(page, RESTRICTED);
    await stub(page, "counter-token");
    await page.goto("/floor");
    const alert = page.getByTestId("hub-service-alert");
    await expect(alert).toBeVisible();
    await alert.click();
    await expect(page).toHaveURL(/\/system\/service$/);
    await expect(page.getByTestId("service-battery")).toContainText(en["hub.service.battery_on"]);
    await page.getByRole("button", { name: en["hub.service.battery_button"] }).click();
    expect(await bridgeCalls(page)).toContain("openBatterySettings");
    expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
  });

  test("refused notifications get their own line and button", async ({ page }) => {
    await bridge(page, { notifications: false, battery_exempt: true, asked: true });
    await stub(page, "counter-token");
    await page.goto("/system/service");
    await expect(page.getByTestId("service-battery")).toContainText(en["hub.service.battery_off"]);
    await page.getByRole("button", { name: en["hub.service.notifications_button"] }).click();
    expect(await bridgeCalls(page)).toContain("openNotificationSettings");
  });

  test("no alert when nothing is restricted, and none off Android", async ({ page }) => {
    await bridge(page, GOOD);
    await stub(page, "counter-token");
    await page.goto("/floor");
    await expect(page.getByTestId("hub-branch")).toBeVisible();
    await expect(page.getByTestId("hub-service-alert")).toHaveCount(0);
  });

  test("without the Android bridge there is no Hub service section", async ({ page }) => {
    await stub(page, "counter-token");
    await page.goto("/system/audit");
    await expect(page.getByRole("link", { name: en["hub.system.nav.audit"] })).toBeVisible();
    await expect(page.getByRole("link", { name: en["hub.system.nav.service"] })).toHaveCount(0);
  });
});

test.describe("Stop Hub", () => {
  test("a Counter needs a manager's PIN in place, then the stop is audited and the service stops", async ({
    page,
  }) => {
    await bridge(page, GOOD);
    const calls = await stub(page, "counter-token");
    await page.goto("/system/service");
    await page.getByRole("button", { name: en["hub.service.stop"] }).click();
    const approval = page.getByTestId("service-approval");
    await expect(approval).toContainText(en["hub.service.approves"]);
    await approval.getByLabel(en["ui.pin.label"]).fill("1111");
    await page.keyboard.press("Enter");
    await expect(approval).toContainText(en["ui.pin.rejected"]);
    expect(await bridgeCalls(page)).not.toContain("stopHub");
    await approval.getByLabel(en["ui.pin.label"]).fill("2222");
    await page.keyboard.press("Enter");
    await expect.poll(() => bridgeCalls(page)).toContain("stopHub");
    expect(calls.intents).toHaveLength(1);
    expect(calls.intents[0].ticket).toBe("T1");
    expect(Number(calls.intents[0].value)).toBeGreaterThan(1_700_000_000_000);
  });

  test("a Manager confirms once and no second PIN is asked", async ({ page }) => {
    await bridge(page, GOOD);
    const calls = await stub(page, "manager-token");
    await page.goto("/system/service");
    await page.getByRole("button", { name: en["hub.service.stop"] }).click();
    await page.getByRole("button", { name: en["hub.service.stop_confirm"] }).click();
    await expect.poll(() => bridgeCalls(page)).toContain("stopHub");
    expect(calls.elevations).toHaveLength(0);
    expect(calls.intents[0].ticket).toBeUndefined();
  });

  test("Cancel leaves the Hub running", async ({ page }) => {
    await bridge(page, GOOD);
    const calls = await stub(page, "manager-token");
    await page.goto("/system/service");
    await page.getByRole("button", { name: en["hub.service.stop"] }).click();
    await page.getByRole("button", { name: en["hub.service.cancel"] }).click();
    expect(await bridgeCalls(page)).not.toContain("stopHub");
    expect(calls.intents).toHaveLength(0);
  });
});

test.describe("Play update (Story 4.16)", () => {
  test.afterEach(() => {
    sessions = [];
  });

  test("Update ready installs only when no session is active, paused or in grace", async ({
    page,
  }) => {
    sessions = [{ id: "S1", state: "paused" }];
    await bridge(page, GOOD, "ready");
    await stub(page, "counter-token");
    await page.goto("/system/service");
    const panel = page.getByTestId("service-update");
    await expect(panel).toContainText(en["hub.update.ready"]);
    expect(await bridgeCalls(page)).toContain("checkUpdate");
    const install = page.getByRole("button", { name: en["hub.update.install"] });
    await expect(install).toBeDisabled();
    await expect(panel).toContainText(en["hub.update.install_waiting"]);
    expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);

    sessions = [{ id: "S1", state: "ended" }];
    await page.reload();
    await expect(install).toBeEnabled();
    await install.click();
    expect(await bridgeCalls(page)).toContain("installUpdate");
  });

  test("a download in progress and an arena:update event are shown", async ({ page }) => {
    await bridge(page, GOOD, "downloading");
    await stub(page, "counter-token");
    await page.goto("/system/service");
    const panel = page.getByTestId("service-update");
    await expect(panel).toContainText(en["hub.update.downloading"]);
    await page.evaluate(() =>
      window.dispatchEvent(new CustomEvent("arena:update", { detail: { status: "ready" } })),
    );
    await expect(panel).toContainText(en["hub.update.ready"]);
  });

  test("with nothing to install the panel says the Hub is up to date", async ({ page }) => {
    await bridge(page, GOOD, "none");
    await stub(page, "counter-token");
    await page.goto("/system/service");
    await expect(page.getByTestId("service-update")).toContainText(en["hub.update.none"]);
  });
});
