import { expect, test, type Page, type Route } from "@playwright/test";
import type { HubSnapshot, Tier } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { AdsClient, adsEnabled, isAdEvent, type AdEvent, type AdsBridge } from "../src/lib/ads";
import { isEntitlementChanged } from "../src/lib/events";
import { ORIGIN } from "./origin";

// The ads gate (Story 4.10): ads run only on the Android Hub on the Free tier. The Hub's loopback
// API is stubbed with `page.route`, and a fake `ArenaAds` records what the page asks of the plugin.

const HUB = "http://127.0.0.1:7409";
const CORS = {
  "access-control-allow-origin": ORIGIN,
  "access-control-allow-methods": "GET, POST",
  "access-control-allow-headers": "authorization, content-type",
};
const STAFF = {
  staff_id: "01JMANAGER0000000000000000",
  display_name: "Manager",
  role_name: "Manager",
  permissions: ["settings.edit"],
};
const ANDROID_UA =
  "Mozilla/5.0 (Linux; Android 14; SM-X110) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

type Hub = { tier: Tier | null; queue: string[] };

function json(route: Route, status: number, body: unknown) {
  return route.fulfill({
    status,
    contentType: "application/json",
    headers: CORS,
    body: JSON.stringify(body),
  });
}

async function open(page: Page, tier: Tier | null, { debug = true, path = "/floor" } = {}) {
  const hub: Hub = { tier, queue: [] };
  await page.addInitScript((debugBuild) => {
    sessionStorage.setItem("arena.hub.token", "manager-token");
    const calls: string[] = [];
    (window as unknown as { __ads: string[] }).__ads = calls;
    (window as unknown as { ArenaAds: unknown }).ArenaAds = {
      init: (config: string) => {
        calls.push(`init ${config}`);
        return JSON.stringify({ debug: debugBuild });
      },
      bannerPlace: (rect: string) => calls.push(`bannerPlace ${rect}`),
      bannerHide: () => calls.push("bannerHide"),
      bannerDestroy: () => calls.push("bannerDestroy"),
      interstitialPreload: () => calls.push("interstitialPreload"),
      interstitialShow: () => calls.push("interstitialShow"),
    };
  }, debug);
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot": {
        const s: Record<string, unknown> = {
          branch: { name: "Dev Branch", branch_code: "DEV01" },
          devices: [],
          locale: null,
          ui: { idle_lock_minutes: 5, hc: false },
          connectivity: { state: "online", checked_at: null },
        };
        if (hub.tier) s.tier = hub.tier;
        return json(route, 200, s as unknown as HubSnapshot);
      }
      case "/ui/me":
        return json(route, 200, STAFF);
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
  await page.goto(path);
  await expect(page.locator("h1")).toHaveCount(1);
  return hub;
}

const calls = (page: Page) => page.evaluate(() => (window as unknown as { __ads: string[] }).__ads);
/** The plugin calls without banner placements (the permanent slot places it on every resize). */
const lifecycle = async (page: Page) =>
  (await calls(page)).filter((c) => !c.startsWith("bannerPlace"));
/** The Hub's tier moved: the snapshot says so and the stream pushes the change. */
const entitlement = (hub: Hub, tier: Tier | null, { snapshot = true } = {}) => {
  if (snapshot) hub.tier = tier;
  hub.queue.push(`event: entitlement.changed\ndata: ${JSON.stringify({ tier })}\n\n`);
};

test.describe("the gate", () => {
  test("is on only for Android on Free", () => {
    expect(adsEnabled(true, "free")).toBe(true);
    for (const tier of ["basic", "premium", null, undefined] as const) {
      expect(adsEnabled(true, tier)).toBe(false);
    }
    for (const tier of ["free", "basic", null] as const)
      expect(adsEnabled(false, tier)).toBe(false);
  });

  test("the entitlement event and the plugin events are checked", () => {
    expect(isEntitlementChanged({ tier: "free" })).toBe(true);
    expect(isEntitlementChanged({ tier: null })).toBe(true);
    expect(isEntitlementChanged({ tier: "gold" })).toBe(false);
    expect(isEntitlementChanged({})).toBe(false);
    expect(isAdEvent({ event: "ad.loaded", slot: "banner" })).toBe(true);
    expect(isAdEvent({ event: "ad.clicked", slot: "side" })).toBe(false);
    expect(isAdEvent({ event: "ad.unknown", slot: "banner" })).toBe(false);
  });

  function client(android: boolean) {
    const log: string[] = [];
    const listeners = new Set<(e: AdEvent) => void>();
    const bridge: AdsBridge = {
      init: (c) => (log.push(`init ${c}`), '{"debug":false}'),
      bannerPlace: (r) => void log.push(`place ${r}`),
      bannerHide: () => void log.push("hide"),
      bannerDestroy: () => void log.push("destroy"),
      interstitialPreload: () => void log.push("preload"),
      interstitialShow: () => void log.push("show"),
    };
    const ads = new AdsClient({
      android,
      bridge: () => bridge,
      listen: (f) => (listeners.add(f), () => listeners.delete(f)),
    });
    return { ads, log, emit: (e: AdEvent) => listeners.forEach((f) => f(e)), listeners };
  }

  test("starts the plugin once on Free, stops it when the tier moves, starts it again on Free", () => {
    const { ads, log, listeners } = client(true);
    expect(ads.evaluate(undefined)).toBe(false);
    expect(ads.evaluate("free")).toBe(true);
    expect(ads.evaluate("free")).toBe(true);
    expect(log).toEqual(['init {"rating":"PG"}']);
    ads.banner.place({ x: 0, y: 800, width: 412, height: 50 });
    ads.interstitial.preload();
    expect(ads.evaluate("basic")).toBe(false);
    expect(listeners.size).toBe(0);
    ads.banner.place({ x: 1, y: 1, width: 1, height: 1 });
    ads.interstitial.show();
    expect(ads.evaluate(null)).toBe(false);
    expect(ads.evaluate("free")).toBe(true);
    expect(log).toEqual([
      'init {"rating":"PG"}',
      'place {"x":0,"y":800,"width":412,"height":50}',
      "preload",
      "destroy",
      'init {"rating":"PG"}',
    ]);
  });

  test("is a no-op off Android", () => {
    const { ads, log } = client(false);
    expect(ads.evaluate("free")).toBe(false);
    ads.banner.place({ x: 0, y: 0, width: 1, height: 1 });
    ads.banner.hide();
    ads.interstitial.preload();
    ads.interstitial.show();
    expect(log).toEqual([]);
  });

  test("hands plugin events to listeners only while on", () => {
    const { ads, emit } = client(true);
    const seen: AdEvent[] = [];
    ads.onEvent((e) => seen.push(e));
    emit({ event: "ad.loaded", slot: "banner" });
    ads.evaluate("free");
    emit({ event: "ad.loaded", slot: "banner" });
    ads.evaluate("basic");
    emit({ event: "ad.shown", slot: "banner" });
    expect(seen).toEqual([{ event: "ad.loaded", slot: "banner" }]);
  });
});

test.describe("android", () => {
  test.use({ userAgent: ANDROID_UA });

  test("a Free Hub starts the plugin once", async ({ page }) => {
    await open(page, "free");
    await expect.poll(() => lifecycle(page)).toEqual(['init {"rating":"PG"}']);
    await page.waitForTimeout(800);
    expect(await lifecycle(page)).toEqual(['init {"rating":"PG"}']);
  });

  test("a paid Hub never starts it and renders no ad slot", async ({ page }) => {
    await open(page, "basic", { path: "/dev/ads" });
    await expect(page.getByTestId("dev-ads-unavailable")).toBeVisible();
    await expect(page.getByTestId("ad-banner")).toHaveCount(0);
    expect(await calls(page)).toEqual([]);
  });

  test("a Hub that does not know its tier yet starts nothing", async ({ page }) => {
    await open(page, null);
    await page.waitForTimeout(800);
    expect(await calls(page)).toEqual([]);
  });

  test("an upgrade pushed on the stream stops ads without a restart, and a lapse brings them back", async ({
    page,
  }) => {
    const hub = await open(page, "free");
    await expect.poll(() => lifecycle(page)).toEqual(['init {"rating":"PG"}']);
    await expect(page.getByTestId("ad-banner")).toBeVisible();
    entitlement(hub, "basic");
    await expect.poll(() => lifecycle(page)).toEqual(['init {"rating":"PG"}', "bannerDestroy"]);
    await expect(page.getByTestId("ad-banner")).toHaveCount(0);
    entitlement(hub, "free");
    await expect
      .poll(() => lifecycle(page))
      .toEqual(['init {"rating":"PG"}', "bannerDestroy", 'init {"rating":"PG"}']);
    await expect(page.getByTestId("ad-banner")).toBeVisible();
  });

  test("the stream event alone stops ads, with the snapshot still saying Free", async ({
    page,
  }) => {
    const hub = await open(page, "free");
    await expect.poll(() => lifecycle(page)).toEqual(['init {"rating":"PG"}']);
    // The snapshot keeps its old tier, so only the `entitlement.changed` event can destroy the banner.
    entitlement(hub, "premium", { snapshot: false });
    await expect.poll(() => calls(page)).toContain("bannerDestroy");
  });

  test("the dev gallery loads a banner and an interstitial and lists each event", async ({
    page,
  }) => {
    await open(page, "free", { path: "/dev/ads" });
    await expect(page.getByTestId("ad-banner")).toBeVisible();
    // The permanent slot placed the banner on its own; the button puts it there again.
    await expect.poll(async () => (await calls(page)).length).toBe(2);
    await page.getByRole("button", { name: en["hub.dev_ads.banner_place"] }).click();
    await page.getByRole("button", { name: en["hub.dev_ads.interstitial_preload"] }).click();
    await page.getByRole("button", { name: en["hub.dev_ads.interstitial_show"] }).click();
    await page.getByRole("button", { name: en["hub.dev_ads.banner_hide"] }).click();
    const log = await calls(page);
    for (const i of [1, 2]) {
      expect(log[i]).toMatch(
        /^bannerPlace \{"x":[\d.]+,"y":[\d.]+,"width":[\d.]+,"height":[\d.]+\}$/,
      );
    }
    expect(log.slice(3)).toEqual(["interstitialPreload", "interstitialShow", "bannerHide"]);
    await page.evaluate(() => {
      for (const [event, slot] of [
        ["ad.requested", "banner"],
        ["ad.loaded", "banner"],
        ["ad.shown", "interstitial"],
        ["ad.dismissed", "interstitial"],
      ]) {
        window.dispatchEvent(new CustomEvent("arena:ad", { detail: { event, slot } }));
      }
    });
    await expect(page.getByTestId("dev-ads-events").locator("li")).toHaveText([
      "ad.dismissed · interstitial",
      "ad.shown · interstitial",
      "ad.loaded · banner",
      "ad.requested · banner",
    ]);
  });

  test("the dev gallery stays closed on a release build", async ({ page }) => {
    await open(page, "free", { debug: false, path: "/dev/ads" });
    await expect(page.getByTestId("dev-ads-unavailable")).toHaveText(en["hub.dev_ads.unavailable"]);
  });
});

test("the Windows Hub never starts the plugin, even on Free", async ({ page }) => {
  await open(page, "free");
  await page.waitForTimeout(800);
  expect(await calls(page)).toEqual([]);
});
