import { expect, test, type Page } from "@playwright/test";
import type { Tier, UiAdEvent } from "@arena/ui/generated";
import { CORS, HUB, MANAGER, device, json, snapshot } from "./floor-fixture";

// Counting ads (Story 4.14): on a Free Android Hub the page forwards the plugin's `ad.requested`
// and `ad.shown` to the Hub once each; anywhere else it forwards nothing. The Hub's loopback API
// is stubbed with `page.route`, a fake `ArenaAds` stands in for the plugin, and the test plays
// the plugin's events.

const ANDROID_UA =
  "Mozilla/5.0 (Linux; Android 14; SM-X110) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";
const A = "01JADEVENT0000000000000001";
const B = "01JADEVENT0000000000000002";
const C = "01JADEVENT0000000000000003";

type Stub = { posts: UiAdEvent[]; status: number };

async function open(page: Page, tier: Tier = "free"): Promise<Stub> {
  const hub: Stub = { posts: [], status: 204 };
  await page.addInitScript(() => {
    sessionStorage.setItem("arena.hub.token", "tok");
    (window as unknown as { ArenaAds: unknown }).ArenaAds = {
      init: () => JSON.stringify({ debug: false }),
      configure: () => {},
      bannerPlace: () => {},
      bannerHide: () => {},
      bannerDestroy: () => {},
      interstitialPreload: () => {},
      interstitialShow: () => {},
    };
  });
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot": {
        const snap = snapshot([device(1, { status: "online" })]);
        snap.tier = tier;
        return json(route, snap);
      }
      case "/ui/me":
        return json(route, { ...MANAGER, locked: false, has_pin: true });
      case "/ui/settings":
        return json(route, []);
      case "/ui/ads/event":
        hub.posts.push(req.postDataJSON() as UiAdEvent);
        return route.fulfill({ status: hub.status, headers: CORS });
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: `retry: 300\n\n`,
        });
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/floor");
  await expect(page.locator("[data-component=floor-tile]").first()).toBeVisible();
  return hub;
}

/** The plugin's event, as the Kotlin side delivers it (with its own id). */
const emit = (page: Page, event: string, slot: string, id?: string) =>
  page.evaluate(
    ([e, s, i]) =>
      window.dispatchEvent(
        new CustomEvent("arena:ad", { detail: { event: e, slot: s, ...(i ? { id: i } : {}) } }),
      ),
    [event, slot, id ?? ""],
  );

test.use({ viewport: { width: 1280, height: 800 }, userAgent: ANDROID_UA });

test("a Free Android Hub posts each request and impression once, with the plugin's id", async ({
  page,
}) => {
  const hub = await open(page);
  await expect(page.locator("html")).toHaveAttribute("data-ads", "on");
  await emit(page, "ad.requested", "banner", A);
  await emit(page, "ad.shown", "banner", B);
  await emit(page, "ad.requested", "interstitial", C);
  // The same events again, and events that are not counted.
  await emit(page, "ad.requested", "banner", A);
  await emit(page, "ad.shown", "banner", B);
  await emit(page, "ad.loaded", "interstitial", "01JADEVENT0000000000000009");
  await emit(page, "ad.dismissed", "interstitial", "01JADEVENT0000000000000008");
  await expect.poll(() => hub.posts.length).toBe(3);
  await page.waitForTimeout(200);
  expect(hub.posts).toEqual([
    { kind: "request", slot: "banner", id: A },
    { kind: "impression", slot: "banner", id: B },
    { kind: "request", slot: "interstitial", id: C },
  ]);
});

test("an event with no id of its own still counts once, under an id the page made", async ({
  page,
}) => {
  const hub = await open(page);
  await emit(page, "ad.shown", "interstitial");
  await expect.poll(() => hub.posts.length).toBe(1);
  expect(hub.posts[0]).toMatchObject({ kind: "impression", slot: "interstitial" });
  expect(hub.posts[0].id).toMatch(/^[0-9A-HJKMNP-TV-Z]{26}$/);
});

test("a post the Hub refuses to take now is repeated with the same id", async ({ page }) => {
  await page.clock.install();
  const hub = await open(page);
  hub.status = 503;
  await emit(page, "ad.requested", "banner", A);
  await expect.poll(() => hub.posts.length).toBe(1);
  hub.status = 204;
  await page.clock.runFor(2_500);
  await expect.poll(() => hub.posts.length).toBe(2);
  expect(hub.posts.map((p) => p.id)).toEqual([A, A]);
  await page.clock.runFor(60_000);
  expect(hub.posts).toHaveLength(2);
});

test("a paid Android Hub posts nothing", async ({ page }) => {
  const hub = await open(page, "basic");
  await expect(page.locator("html")).not.toHaveAttribute("data-ads", "on");
  await emit(page, "ad.requested", "banner", A);
  await emit(page, "ad.shown", "banner", B);
  await page.waitForTimeout(300);
  expect(hub.posts).toEqual([]);
});

test.describe("Windows", () => {
  test.use({ userAgent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/126.0.0.0" });

  test("the Windows Hub posts nothing", async ({ page }) => {
    const hub = await open(page);
    await emit(page, "ad.requested", "banner", A);
    await emit(page, "ad.shown", "banner", B);
    await page.waitForTimeout(300);
    expect(hub.posts).toEqual([]);
  });
});
