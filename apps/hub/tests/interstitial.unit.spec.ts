import { expect, test } from "@playwright/test";
import type { AdEvent } from "../src/lib/ads";
import {
  INTERSTITIAL_GAP_MS,
  INTERSTITIAL_MAX_MS,
  InterstitialPolicy,
  type InterstitialAds,
  type InterstitialClock,
} from "../src/lib/interstitial";

// When a full-screen ad may follow a PIN unlock (Story 4.13), with a fake plugin and clock.

const MIN = 60_000;

function setup(enabled = true) {
  const log: string[] = [];
  const handlers = new Set<(e: AdEvent) => void>();
  const ads: InterstitialAds = {
    state: { enabled },
    onEvent: (f) => (handlers.add(f), () => handlers.delete(f)),
    interstitial: {
      preload: () => void log.push("preload"),
      show: () => void log.push("show"),
    },
  };
  let now = 1_000_000;
  let timer: { fn: () => void; at: number } | null = null;
  const clock: InterstitialClock = {
    now: () => now,
    setTimeout: (fn, ms) => ((timer = { fn, at: now + ms }), timer),
    clearTimeout: () => (timer = null),
  };
  const policy = new InterstitialPolicy(ads, clock);
  const emit = (event: AdEvent["event"], slot: AdEvent["slot"] = "interstitial") =>
    handlers.forEach((f) => f({ event, slot }));
  const advance = (ms: number) => {
    now += ms;
    if (timer && timer.at <= now) {
      const t = timer;
      timer = null;
      t.fn();
    }
  };
  return { log, policy, emit, advance };
}

test("the lock preloads one interstitial and an unlock shows it", () => {
  const { log, policy, emit } = setup();
  policy.lock([]);
  expect(log).toEqual(["preload"]);
  emit("ad.loaded");
  expect(policy.unlock()).toBe(true);
  expect(log).toEqual(["preload", "show"]);
  expect(policy.showing).toBe(true);
  emit("ad.shown");
  emit("ad.dismissed");
  expect(policy.showing).toBe(false);
});

test("an unlock with nothing loaded returns at once and never waits", () => {
  const { log, policy } = setup();
  policy.lock([]);
  expect(policy.unlock()).toBe(false);
  expect(policy.showing).toBe(false);
  expect(log).toEqual(["preload"]);
});

test("a failed load shows nothing", () => {
  const { policy, emit, log } = setup();
  policy.lock([]);
  emit("ad.failed");
  expect(policy.unlock()).toBe(false);
  expect(log).toEqual(["preload"]);
});

test("an ad-exclusion surface open at lock time skips the ad: nothing is requested or shown", () => {
  const { log, policy, emit } = setup();
  policy.lock(["tile-sheet"]);
  expect(log).toEqual([]);
  emit("ad.loaded");
  expect(policy.unlock()).toBe(false);
  expect(log).toEqual([]);
  // Skipped, not delayed: the next lock decides afresh.
  policy.lock([]);
  expect(policy.unlock()).toBe(true);
  expect(log).toEqual(["preload", "show"]);
});

test("one interstitial per 15 minutes", () => {
  const { log, policy, emit, advance } = setup();
  policy.lock([]);
  emit("ad.loaded");
  expect(policy.unlock()).toBe(true);
  emit("ad.shown");
  emit("ad.dismissed");
  advance(14 * MIN + 59_000);
  policy.lock([]);
  emit("ad.loaded");
  expect(policy.unlock()).toBe(false);
  expect(log).toEqual(["preload", "show"]);
  advance(INTERSTITIAL_GAP_MS - (14 * MIN + 59_000));
  policy.lock([]);
  emit("ad.loaded");
  expect(policy.unlock()).toBe(true);
  expect(log).toEqual(["preload", "show", "preload", "show"]);
});

test("a wall clock that went backwards does not hold an ad back", () => {
  const { policy, emit, advance } = setup();
  policy.lock([]);
  emit("ad.loaded");
  policy.unlock();
  emit("ad.shown");
  emit("ad.dismissed");
  advance(-2 * 60 * MIN);
  policy.lock([]);
  emit("ad.loaded");
  expect(policy.unlock()).toBe(true);
});

test("a gate that is off requests and shows nothing", () => {
  const { log, policy, emit } = setup(false);
  policy.lock([]);
  emit("ad.loaded");
  expect(policy.unlock()).toBe(false);
  expect(log).toEqual([]);
});

test("a failed show, the plugin saying nothing and the page coming back all end the wait", () => {
  const a = setup();
  a.policy.lock([]);
  a.emit("ad.loaded");
  a.policy.unlock();
  a.emit("ad.failed");
  expect(a.policy.showing).toBe(false);

  const b = setup();
  b.policy.lock([]);
  b.emit("ad.loaded");
  b.policy.unlock();
  b.advance(INTERSTITIAL_MAX_MS);
  expect(b.policy.showing).toBe(false);

  const c = setup();
  c.policy.lock([]);
  c.emit("ad.loaded");
  c.policy.unlock();
  c.policy.ended();
  expect(c.policy.showing).toBe(false);
});

test("only a lock followed by an unlock shows: a stray load does not", () => {
  const { log, policy, emit } = setup();
  emit("ad.loaded");
  expect(policy.unlock()).toBe(false);
  expect(log).toEqual([]);
});

test("banner events are not interstitial events", () => {
  const { policy, emit } = setup();
  policy.lock([]);
  emit("ad.loaded", "banner");
  expect(policy.unlock()).toBe(false);
});

test("a lock that ends without a PIN here forgets its exclusion", () => {
  const { policy, emit } = setup();
  policy.lock(["inline-pin"]);
  policy.cancel();
  emit("ad.loaded");
  expect(policy.unlock()).toBe(false);
});
