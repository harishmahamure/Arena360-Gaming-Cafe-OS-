import { expect, test, type Page } from "@playwright/test";
import type { SnapshotAlert, SnapshotDevice, Tier } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { CORS, HUB, MANAGER, device, json, snapshot } from "./floor-fixture";

// The interstitial (Story 4.13): on a Free Android Hub a standard full-screen ad may follow a PIN
// unlock after an idle lock, and nothing else. The Hub's loopback API is stubbed with
// `page.route`, a fake `ArenaAds` records what the page asks of the plugin, and the test plays
// the plugin's events. Time is `page.clock`.

const ANDROID_UA =
  "Mozilla/5.0 (Linux; Android 14; SM-X110) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";
const MIN = 60_000;
const PC1 = device(1, { status: "online" });

type Stub = { alerts: SnapshotAlert[]; queue: string[]; locks: number; unlocks: number };

async function open(page: Page, tier: Tier = "free"): Promise<Stub> {
  const hub: Stub = { alerts: [], queue: [], locks: 0, unlocks: 0 };
  const devices: SnapshotDevice[] = [PC1, device(2, { status: "online" })];
  await page.addInitScript(() => {
    sessionStorage.setItem("arena.hub.token", "tok");
    const w = window as unknown as { __ads: string[]; __plays: string[] };
    w.__ads = [];
    w.__plays = [];
    HTMLMediaElement.prototype.play = function (this: HTMLMediaElement) {
      w.__plays.push(this.src);
      return Promise.resolve();
    };
    (window as unknown as { ArenaAds: unknown }).ArenaAds = {
      init: (config: string) => (w.__ads.push(`init ${config}`), JSON.stringify({ debug: false })),
      configure: (config: string) => w.__ads.push(`configure ${config}`),
      bannerPlace: () => {},
      bannerHide: () => {},
      bannerDestroy: () => {},
      interstitialPreload: () => w.__ads.push("preload"),
      interstitialShow: () => w.__ads.push("show"),
    };
  });
  await page.clock.install();
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot": {
        const snap = snapshot(devices);
        snap.tier = tier;
        snap.alerts = hub.alerts;
        return json(route, snap);
      }
      case "/ui/me":
        return json(route, { ...MANAGER, locked: false, has_pin: true });
      case "/ui/settings":
        return json(route, []);
      case "/ui/lock":
        hub.locks++;
        return route.fulfill({ status: 204, headers: CORS });
      case "/ui/unlock":
        hub.unlocks++;
        return json(route, { staff: MANAGER, switched: false });
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: `retry: 300\n\n` + hub.queue.splice(0).join(""),
        });
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/floor");
  await expect(page.locator("[data-component=floor-tile]").first()).toBeVisible();
  return hub;
}

const calls = (page: Page) => page.evaluate(() => (window as unknown as { __ads: string[] }).__ads);
const interstitialCalls = async (page: Page) =>
  (await calls(page)).filter((c) => c === "preload" || c === "show");
const plays = (page: Page) =>
  page.evaluate(() => (window as unknown as { __plays: string[] }).__plays);
/** The plugin's event, as the Kotlin side delivers it. */
const emit = (page: Page, event: string, slot = "interstitial") =>
  page.evaluate(
    ([e, s]) =>
      window.dispatchEvent(new CustomEvent("arena:ad", { detail: { event: e, slot: s } })),
    [event, slot],
  );
const lockScreen = (page: Page) => page.getByRole("dialog", { name: en["hub.lock.title"] });

async function idleLock(page: Page) {
  await page.clock.runFor(5 * MIN + 1000);
  await expect(lockScreen(page)).toBeVisible();
}
async function unlock(page: Page) {
  await page.keyboard.type("2222");
  await page.keyboard.press("Enter");
  await expect(lockScreen(page)).toHaveCount(0);
}

test.use({ viewport: { width: 1280, height: 800 }, userAgent: ANDROID_UA });

test("the lock preloads once; a PIN unlock shows the ad; focus returns and the idle timer restarts", async ({
  page,
}) => {
  await open(page);
  const tile = page.locator("[data-component=floor-tile] [data-tile-face]").first();
  await tile.focus();
  await idleLock(page);
  await expect.poll(() => interstitialCalls(page)).toEqual(["preload"]);
  await emit(page, "ad.loaded");
  await unlock(page);
  await expect.poll(() => interstitialCalls(page)).toEqual(["preload", "show"]);
  await emit(page, "ad.shown");
  // While the ad is up the idle countdown does not run.
  await page.clock.runFor(90_000);
  await expect(lockScreen(page)).toHaveCount(0);
  await emit(page, "ad.dismissed");
  await expect(tile).toBeFocused();
  // The countdown restarted from zero when the ad closed.
  await page.clock.runFor(4 * MIN + 45_000);
  await expect(lockScreen(page)).toHaveCount(0);
  await page.clock.runFor(30_000);
  await expect(lockScreen(page)).toBeVisible();
});

test("with nothing loaded the unlock returns at once and shows nothing", async ({ page }) => {
  await open(page);
  await idleLock(page);
  await unlock(page);
  expect(await interstitialCalls(page)).toEqual(["preload"]);
  // The idle timer runs as normal.
  await page.clock.runFor(5 * MIN + 1000);
  await expect(lockScreen(page)).toBeVisible();
});

test("a second lock inside 15 minutes shows no ad", async ({ page }) => {
  await open(page);
  await idleLock(page);
  await emit(page, "ad.loaded");
  await unlock(page);
  await emit(page, "ad.shown");
  await emit(page, "ad.dismissed");
  await idleLock(page);
  await emit(page, "ad.loaded");
  await unlock(page);
  expect(await interstitialCalls(page)).toEqual(["preload", "show"]);
});

test("an open tile sheet at lock time skips the ad and unlock returns to the sheet", async ({
  page,
}) => {
  await open(page);
  await page.locator("[data-component=floor-tile] [data-tile-face]").first().click();
  await expect(page.locator("[data-component=tile-sheet]")).toBeVisible();
  await idleLock(page);
  await emit(page, "ad.loaded");
  await unlock(page);
  expect(await interstitialCalls(page)).toEqual([]);
  await expect(page.locator("[data-component=tile-sheet]")).toBeVisible();
});

test("an alert raised while the ad is up sounds, and its badge is waiting when the ad closes", async ({
  page,
}) => {
  const hub = await open(page);
  await idleLock(page);
  await emit(page, "ad.loaded");
  await unlock(page);
  await emit(page, "ad.shown");
  hub.alerts = [
    { id: "A1", kind: "time_up", device_id: PC1.device_id, raised_at: Date.now() } as SnapshotAlert,
  ];
  hub.queue.push("event: devices.changed\ndata: {}\n\n");
  await page.clock.runFor(1000);
  await expect.poll(async () => (await plays(page)).length).toBeGreaterThan(0);
  await emit(page, "ad.dismissed");
  await expect(
    page
      .locator("[data-component=floor-tile]", { hasText: "PC 1" })
      .locator("[data-component=alert-badge]"),
  ).toBeVisible();
});

test("a paid Android Hub never preloads or shows an ad", async ({ page }) => {
  await open(page, "basic");
  await idleLock(page);
  await emit(page, "ad.loaded");
  await unlock(page);
  expect(await calls(page)).toEqual([]);
});
