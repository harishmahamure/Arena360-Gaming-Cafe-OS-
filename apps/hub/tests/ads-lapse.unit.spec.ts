import { expect, test } from "@playwright/test";
import { AdsClient, type AdEvent, type AdsBridge } from "../src/lib/ads";
import { InterstitialPolicy, type InterstitialClock } from "../src/lib/interstitial";

// Upgrading and lapsing (Story 4.15) at the page: when the gate turns off the plugin is told to
// stop (banner destroyed, preloaded interstitial dropped), the page forgets the ad it had loaded,
// and a lapse brings ads back only with a fresh preload.

function setup() {
  const log: string[] = [];
  const listeners = new Set<(e: AdEvent) => void>();
  const bridge: AdsBridge = {
    init: (c) => (log.push(`init ${c}`), '{"debug":false}'),
    configure: () => {},
    bannerPlace: () => {},
    bannerHide: () => {},
    bannerDestroy: () => void log.push("destroy"),
    interstitialPreload: () => void log.push("preload"),
    interstitialShow: () => void log.push("show"),
    stop: () => void log.push("stop"),
  };
  const ads = new AdsClient({
    android: true,
    bridge: () => bridge,
    listen: (f) => (listeners.add(f), () => listeners.delete(f)),
  });
  const clock: InterstitialClock = {
    now: () => 0,
    setTimeout: () => 0,
    clearTimeout: () => {},
  };
  const policy = new InterstitialPolicy(ads, clock);
  const emit = (e: AdEvent) => listeners.forEach((f) => f(e));
  return { ads, policy, log, emit };
}

test("the gate turning off tells the plugin to stop, once, and stops listening", () => {
  const { ads, log, emit } = setup();
  ads.evaluate("free");
  ads.evaluate("basic");
  ads.evaluate("basic");
  expect(log).toEqual(['init {"rating":"PG","personalised":false}', "destroy", "stop"]);
  // Nothing reaches the page's handlers or the plugin any more.
  const seen: AdEvent[] = [];
  ads.onEvent((e) => seen.push(e));
  emit({ event: "ad.loaded", slot: "interstitial" });
  ads.interstitial.preload();
  expect(seen).toEqual([]);
  expect(log).toHaveLength(3);
});

test("a plugin without stop() still has its banner destroyed", () => {
  const log: string[] = [];
  const ads = new AdsClient({
    android: true,
    bridge: () => ({
      init: () => '{"debug":false}',
      configure: () => {},
      bannerPlace: () => {},
      bannerHide: () => {},
      bannerDestroy: () => void log.push("destroy"),
      interstitialPreload: () => {},
      interstitialShow: () => {},
    }),
    listen: () => () => {},
  });
  ads.evaluate("free");
  ads.evaluate("premium");
  expect(log).toEqual(["destroy"]);
  expect(ads.state.enabled).toBe(false);
});

test("an interstitial loaded before an upgrade is not shown after a lapse without a new load", () => {
  const { ads, policy, log, emit } = setup();
  ads.evaluate("free");
  policy.lock([]);
  emit({ event: "ad.loaded", slot: "interstitial" });
  // Paid before the unlock: nothing is shown, and the lock ends.
  ads.evaluate("basic");
  expect(policy.unlock()).toBe(false);
  // Lapsed: the next lock preloads again; an unlock before it loads shows nothing.
  ads.evaluate("free");
  policy.lock([]);
  expect(policy.unlock()).toBe(false);
  expect(log.filter((c) => c === "show")).toEqual([]);
  expect(log.filter((c) => c === "preload")).toHaveLength(2);
});

test("a paid Hub's lock neither preloads nor shows", () => {
  const { ads, policy, log } = setup();
  ads.evaluate("basic");
  policy.lock([]);
  expect(policy.unlock()).toBe(false);
  expect(log).toEqual([]);
});
