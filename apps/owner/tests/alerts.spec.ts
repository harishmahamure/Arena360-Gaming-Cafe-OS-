import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import { FREE, PAID, alertOf, stubCloud, t } from "./stub";

const ONE = [PAID[0]];
const OFFLINE = alertOf(
  {
    id: "al3",
    kind: "branch_offline",
    text: "The branch has been offline for 20 minutes.",
    url: "/#today-b1",
  },
  60_000,
);
const CASH = alertOf(
  {
    id: "al2",
    kind: "cash_variance",
    text: "Cash is off by Rs 250 at the close of the evening shift.",
    url: "/reports?tab=cash&branch=b1",
  },
  300_000,
);
const STOCK = alertOf({}, 900_000);

const rows = (page: Page) => page.locator('[data-component="alert-row"]');
const live = (page: Page) => page.locator('.visually-hidden[aria-live="polite"]');
const pushSwitch = (page: Page) => page.getByRole("switch", { name: t("owner.alerts.push.label") });
const whatsappSwitch = (page: Page) =>
  page.getByRole("switch", { name: t("owner.alerts.whatsapp.label") });
const refocus = (page: Page) => page.evaluate(() => dispatchEvent(new Event("focus")));

type PushStub = {
  permission: "default" | "granted" | "denied";
  grant: boolean;
  ios: boolean;
  /** The browser already holds a subscription. */
  held: boolean;
};

/** A stand-in for the browser's Notification and PushManager, so no real prompt or service is used. */
const stubPush = (page: Page, opts: Partial<PushStub> = {}) =>
  page.addInitScript(
    (o) => {
      const log = { asked: 0, subscribed: 0, unsubscribed: 0, key: [] as number[] };
      Object.assign(window, { __push: log });
      if (o.ios) {
        Object.defineProperty(navigator, "userAgent", {
          get: () => "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15",
        });
      }
      let permission = o.permission;
      class FakeNotification {
        static get permission() {
          return permission;
        }
        static async requestPermission() {
          log.asked++;
          permission = o.grant ? "granted" : "denied";
          return permission;
        }
      }
      let sub: unknown = null;
      const make = () => {
        const endpoint = "https://push.example/send/1";
        return {
          endpoint,
          toJSON: () => ({ endpoint, keys: { p256dh: "PUBKEY", auth: "AUTHKEY" } }),
          unsubscribe: async () => {
            sub = null;
            log.unsubscribed++;
            return true;
          },
        };
      };
      if (o.held) sub = make();
      const manager = {
        getSubscription: async () => sub,
        subscribe: async (options: { applicationServerKey: Uint8Array }) => {
          log.subscribed++;
          log.key = Array.from(options.applicationServerKey);
          sub = make();
          return sub;
        },
      };
      Object.defineProperty(window, "Notification", {
        value: FakeNotification,
        configurable: true,
      });
      Object.defineProperty(window, "PushManager", { value: class {}, configurable: true });
      Object.defineProperty(navigator.serviceWorker, "ready", {
        get: () => Promise.resolve({ pushManager: manager }),
        configurable: true,
      });
    },
    { permission: "default", grant: true, ios: false, held: false, ...opts } satisfies PushStub,
  );

const log = (page: Page) =>
  page.evaluate(
    () =>
      (window as unknown as { __push: { asked: number; subscribed: number; unsubscribed: number } })
        .__push,
  );

test("the tab lists alerts newest first with the branch, the line and the time", async ({
  page,
}) => {
  await stubCloud(page, ONE, { alerts: [OFFLINE, CASH, STOCK] });
  await page.goto("/alerts");
  await expect(rows(page)).toHaveCount(3);
  await expect(rows(page).nth(0)).toContainText(OFFLINE.text);
  await expect(rows(page).nth(1)).toContainText(CASH.text);
  await expect(rows(page).nth(2)).toContainText(STOCK.text);
  const first = rows(page).first();
  await expect(first).toContainText(PAID[0].name);
  await expect(first.locator("time")).toHaveText(/\d{1,2}:\d{2}/);
  await expect(page.getByText(t("owner.alerts.empty"))).toHaveCount(0);
  await expect(page.locator("h1")).toHaveText(t("owner.nav.alerts"));
});

test("with no alerts the tab reads All quiet.", async ({ page }) => {
  await stubCloud(page, ONE);
  await page.goto("/alerts");
  await expect(page.getByText("All quiet.", { exact: true })).toBeVisible();
  await expect(rows(page)).toHaveCount(0);
});

test("a new alert is announced once in a polite live region, and opening the tab announces nothing", async ({
  page,
}) => {
  const calls = await stubCloud(page, ONE, { alerts: [STOCK] });
  await page.goto("/alerts");
  await expect(rows(page)).toHaveCount(1);
  await expect(live(page)).toHaveText("");

  calls.alerts.unshift(CASH);
  await refocus(page);
  await expect(rows(page)).toHaveCount(2);
  await expect(live(page)).toHaveText(
    t("owner.alerts.announce", { branch: PAID[0].name, text: CASH.text }),
  );

  // The same alert is not said again, and the next one replaces it.
  await refocus(page);
  await expect(live(page)).toHaveText(
    t("owner.alerts.announce", { branch: PAID[0].name, text: CASH.text }),
  );
  calls.alerts.unshift(OFFLINE);
  await refocus(page);
  await expect(rows(page)).toHaveCount(3);
  await expect(live(page)).toHaveText(
    t("owner.alerts.announce", { branch: PAID[0].name, text: OFFLINE.text }),
  );
});

test("the list is read again every 30 s while the tab is visible", async ({ page }) => {
  await page.clock.install();
  const calls = await stubCloud(page, ONE, { alerts: [STOCK] });
  await page.goto("/alerts");
  await expect(rows(page)).toHaveCount(1);
  calls.alerts.unshift(CASH);
  await page.clock.runFor(29_000);
  await expect(rows(page)).toHaveCount(1);
  await page.clock.runFor(2_000);
  await expect(rows(page)).toHaveCount(2);
});

test("a push message that reaches the page reads the list at once", async ({ page }) => {
  await page.clock.install();
  const calls = await stubCloud(page, ONE, { alerts: [STOCK] });
  await page.goto("/alerts");
  await expect(rows(page)).toHaveCount(1);
  calls.alerts.unshift(CASH);
  await page.evaluate(() =>
    navigator.serviceWorker.dispatchEvent(
      new MessageEvent("message", { data: { type: "owner-alert" } }),
    ),
  );
  await expect(rows(page)).toHaveCount(2);
});

test("older alerts load a page at a time", async ({ page }) => {
  const many = Array.from({ length: 35 }, (_, i) =>
    alertOf({ id: `n${i}`, text: `Alert number ${i}.` }, 60_000 + i * 1000),
  );
  await stubCloud(page, ONE, { alerts: many });
  await page.goto("/alerts");
  await expect(rows(page)).toHaveCount(30);
  await page.getByRole("button", { name: t("owner.alerts.more") }).click();
  await expect(rows(page)).toHaveCount(35);
  await expect(page.getByRole("button", { name: t("owner.alerts.more") })).toHaveCount(0);
});

test("an alert opens the report on its tab and branch", async ({ page }) => {
  await stubCloud(page, ONE, { alerts: [CASH] });
  await page.goto("/alerts");
  await rows(page).first().getByRole("link").click();
  await expect(page).toHaveURL(/\/reports\?tab=cash&branch=b1$/);
  await expect(page.getByRole("tab", { name: t("owner.reports.tab.cash") })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  await expect(page.getByLabel(t("owner.reports.branch"))).toHaveValue("b1");
});

test("Reports opens on a named tab and ignores an unknown tab or branch", async ({ page }) => {
  await stubCloud(page, [PAID[0], { ...PAID[0], id: "b3", name: "Pixel Arena Dubai" }]);
  await page.goto("/reports?tab=stock&branch=b3");
  await expect(page.getByRole("tab", { name: t("owner.reports.tab.stock") })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  await expect(page.getByLabel(t("owner.reports.branch"))).toHaveValue("b3");
  await page.goto("/reports?tab=nope&branch=nope");
  await expect(page.getByRole("tab", { name: t("owner.reports.tab.revenue") })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  await expect(page.getByLabel(t("owner.reports.branch"))).toHaveValue("all");
});

test("an offline alert opens Today at the branch's card", async ({ page }) => {
  await stubCloud(page, [PAID[0], { ...PAID[0], id: "b3", name: "Pixel Arena Dubai" }], {
    alerts: [{ ...OFFLINE, branch_id: "b3", branch_name: "Pixel Arena Dubai", url: "/#today-b3" }],
  });
  await page.goto("/alerts");
  await rows(page).first().getByRole("link").click();
  await expect(page).toHaveURL(/\/#today-b3$/);
  const card = page.getByRole("heading", { name: "Pixel Arena Dubai", level: 2 });
  await expect(card).toBeFocused();
  await expect(card).toBeInViewport();
});

test("a Free café sees only the lock and the page reads nothing", async ({ page }) => {
  const calls = await stubCloud(page, FREE);
  await page.goto("/alerts");
  await expect(page.getByText(t("staff.tier.available_on", { tier: "Basic" }))).toBeVisible();
  await page.waitForLoadState("networkidle");
  expect(calls.filter((c) => c.includes("/v1/alerts"))).toEqual([]);
});

test("the WhatsApp switch is on by default, saves, and stays off after a reload", async ({
  page,
}) => {
  const calls = await stubCloud(page, ONE);
  await page.goto("/alerts");
  await expect(whatsappSwitch(page)).toBeChecked();
  await whatsappSwitch(page).click();
  await expect(whatsappSwitch(page)).not.toBeChecked();
  expect(calls.bodies["PATCH /v1/alerts/settings"]).toEqual([{ whatsapp: false }]);
  await page.reload();
  await expect(whatsappSwitch(page)).not.toBeChecked();
  await whatsappSwitch(page).click();
  await expect(whatsappSwitch(page)).toBeChecked();
});

test("a WhatsApp save that fails says so and the switch goes back", async ({ page }) => {
  await stubCloud(page, ONE);
  await page.route("**/v1/alerts/settings", (route) =>
    route.request().method() === "PATCH"
      ? route.fulfill({
          status: 500,
          contentType: "application/json",
          body: JSON.stringify({ code: "INTERNAL", message: "" }),
        })
      : route.fallback(),
  );
  await page.goto("/alerts");
  await whatsappSwitch(page).click();
  await expect(
    page.getByRole("alert").filter({ hasText: t("owner.alerts.whatsapp.failed") }),
  ).toBeVisible();
  await expect(whatsappSwitch(page)).toBeChecked();
});

test("turning push on asks for permission, subscribes with the server's key and tells the server", async ({
  page,
}) => {
  await stubPush(page);
  const calls = await stubCloud(page, ONE);
  await page.goto("/alerts");
  await expect(pushSwitch(page)).not.toBeChecked();
  await pushSwitch(page).click();
  await expect(pushSwitch(page)).toBeChecked();
  await expect(page.getByText(t("owner.alerts.push.on"))).toBeVisible();

  expect(await log(page)).toMatchObject({ asked: 1, subscribed: 1 });
  const key = await page.evaluate(
    () => (window as unknown as { __push: { key: number[] } }).__push.key,
  );
  expect(key).toHaveLength(65);
  expect(key[0]).toBe(4);
  expect(calls.bodies["POST /v1/alerts/push"]).toEqual([
    { endpoint: "https://push.example/send/1", keys: { p256dh: "PUBKEY", auth: "AUTHKEY" } },
  ]);

  // Off again: the server forgets it, then the browser does.
  await pushSwitch(page).click();
  await expect.poll(async () => (await log(page)).unsubscribed).toBe(1);
  await expect(pushSwitch(page)).not.toBeChecked();
  expect(calls.bodies["POST /v1/alerts/push/remove"]).toEqual([
    { endpoint: "https://push.example/send/1" },
  ]);
});

test("a subscription the browser already holds shows the switch on after a reload", async ({
  page,
}) => {
  await stubPush(page, { permission: "granted", held: true });
  await stubCloud(page, ONE);
  await page.goto("/alerts");
  await expect(pushSwitch(page)).toBeChecked();
});

test("denied permission shows how to allow it and leaves the switch off", async ({ page }) => {
  await stubPush(page, { grant: false });
  const calls = await stubCloud(page, ONE);
  await page.goto("/alerts");
  await pushSwitch(page).click();
  await expect(page.getByText(t("owner.alerts.push.denied"))).toBeVisible();
  await expect(pushSwitch(page)).not.toBeChecked();
  expect(await log(page)).toMatchObject({ asked: 1, subscribed: 0 });
  expect(calls.bodies["POST /v1/alerts/push"]).toBeUndefined();
});

test("a site already blocked in the browser says so at once and never asks again", async ({
  page,
}) => {
  await stubPush(page, { permission: "denied" });
  await stubCloud(page, ONE);
  await page.goto("/alerts");
  await expect(page.getByText(t("owner.alerts.push.denied"))).toBeVisible();
  await pushSwitch(page).click();
  await expect(pushSwitch(page)).not.toBeChecked();
  expect(await log(page)).toMatchObject({ asked: 0, subscribed: 0 });
});

test("on iOS in a tab the push switch shows the Home Screen line and never asks", async ({
  page,
}) => {
  await stubPush(page, { ios: true });
  await stubCloud(page, ONE);
  await page.goto("/alerts");
  await pushSwitch(page).click();
  await expect(
    page.getByText("Add Arena 360 to your Home Screen to get alerts here.", { exact: true }),
  ).toBeVisible();
  await expect(pushSwitch(page)).not.toBeChecked();
  expect(await log(page)).toMatchObject({ asked: 0, subscribed: 0 });
});

test("a browser without push says so", async ({ page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(window, "PushManager", { value: undefined, configurable: true });
    delete (window as { PushManager?: unknown }).PushManager;
  });
  await stubCloud(page, ONE);
  await page.goto("/alerts");
  await expect(page.getByText(t("owner.alerts.push.unsupported"))).toBeVisible();
  await expect(pushSwitch(page)).toBeDisabled();
});

test("without a VAPID key on the server push is unavailable and WhatsApp still works", async ({
  page,
}) => {
  await stubPush(page);
  await stubCloud(page, ONE, { alertSettings: { vapid_public_key: null } });
  await page.goto("/alerts");
  await expect(page.getByText(t("owner.alerts.push.unavailable"))).toBeVisible();
  await expect(pushSwitch(page)).toBeDisabled();
  await expect(whatsappSwitch(page)).toBeEnabled();
});

test("the service worker shows a notification on push and opens the alert on a tap", async ({
  request,
}) => {
  const src = await (await request.get("/service-worker.js")).text();
  expect(src).toContain("push");
  expect(src).toContain("showNotification");
  expect(src).toContain("notificationclick");
  expect(src).toContain("openWindow");
  expect(src).toContain("navigate");
});

for (const theme of ["light", "dark"] as const) {
  test(`the Alerts tab with alerts and settings passes axe (${theme})`, async ({ page }) => {
    await stubPush(page);
    await stubCloud(page, ONE, { alerts: [OFFLINE, CASH, STOCK] });
    await page.addInitScript((v) => localStorage.setItem("arena.theme", v), theme);
    await page.goto("/alerts");
    await expect(rows(page)).toHaveCount(3);
    await expect(pushSwitch(page)).toBeVisible();
    await page.evaluate(() => document.fonts.ready);
    const result = await new AxeBuilder({ page }).analyze();
    expect(
      result.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target).join(" ")}`),
    ).toEqual([]);
  });
}
