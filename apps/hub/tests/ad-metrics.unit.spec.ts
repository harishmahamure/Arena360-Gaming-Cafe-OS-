import { expect, test } from "@playwright/test";
import type { UiAdEvent } from "@arena/ui/generated";
import type { AdEvent } from "../src/lib/ads";
import {
  AdMetrics,
  MAX_PENDING,
  RETRY_MS,
  metricKind,
  retryable,
  type AdMetricsEnv,
} from "../src/lib/ad-metrics";

// Forwarding the plugin's ad events to the Hub, once each (Story 4.14), with a fake source,
// a fake Hub and a fake clock.

const A = "01JADEVENT0000000000000001";
const B = "01JADEVENT0000000000000002";

function setup(answers: Array<number | "down"> = []) {
  const posted: UiAdEvent[] = [];
  const handlers = new Set<(e: AdEvent) => void>();
  let ids = 0;
  const timers: Array<{ fn: () => void; ms: number; live: boolean }> = [];
  const env: AdMetricsEnv = {
    post: async (body) => {
      posted.push(body);
      const next = answers.length > 1 ? answers.shift()! : (answers[0] ?? 204);
      if (next === "down") throw new Error("unreachable");
      return next;
    },
    setTimeout: (fn, ms) => {
      const t = { fn, ms, live: true };
      timers.push(t);
      return t;
    },
    clearTimeout: (id) => void ((id as { live: boolean }).live = false),
    newId: () => `01JPAGEMADE${String(++ids).padStart(15, "0")}`,
  };
  const source = {
    onEvent: (f: (e: AdEvent) => void) => (handlers.add(f), () => handlers.delete(f)),
  };
  const metrics = new AdMetrics(source, env);
  const emit = (e: AdEvent) => handlers.forEach((f) => f(e));
  const flush = () => new Promise((r) => setTimeout(r, 0));
  /** Runs the next waiting retry. */
  const retry = async () => {
    const t = timers.find((x) => x.live)!;
    t.live = false;
    t.fn();
    await flush();
  };
  return { metrics, posted, emit, flush, timers, retry, handlers };
}

test("a request and an impression are posted once each, with the plugin's id", async () => {
  const { posted, emit, flush } = setup();
  emit({ event: "ad.requested", slot: "banner", id: A });
  emit({ event: "ad.shown", slot: "interstitial", id: B });
  await flush();
  expect(posted).toEqual([
    { kind: "request", slot: "banner", id: A },
    { kind: "impression", slot: "interstitial", id: B },
  ]);
});

test("every other plugin event is left alone", async () => {
  const { posted, emit, flush } = setup();
  for (const event of ["ad.loaded", "ad.failed", "ad.clicked", "ad.dismissed"] as const) {
    emit({ event, slot: "banner", id: A });
  }
  await flush();
  expect(posted).toEqual([]);
  expect(metricKind("ad.requested")).toBe("request");
  expect(metricKind("ad.shown")).toBe("impression");
  expect(metricKind("ad.loaded")).toBeNull();
});

test("an event delivered twice is posted once", async () => {
  const { posted, emit, flush } = setup();
  emit({ event: "ad.shown", slot: "banner", id: A });
  emit({ event: "ad.shown", slot: "banner", id: A });
  emit({ event: "ad.shown", slot: "banner", id: A.toLowerCase() });
  await flush();
  expect(posted).toHaveLength(1);
});

test("an event without a usable id gets one from the page, and each is its own event", async () => {
  const { posted, emit, flush } = setup();
  emit({ event: "ad.requested", slot: "banner" });
  emit({ event: "ad.requested", slot: "banner", id: "not-a-ulid" });
  await flush();
  expect(posted).toHaveLength(2);
  expect(new Set(posted.map((p) => p.id)).size).toBe(2);
  expect(posted.every((p) => /^01JPAGEMADE/.test(p.id))).toBe(true);
});

test("the post holds a kind, a slot and an id and nothing else", async () => {
  const { posted, emit, flush } = setup();
  emit({ event: "ad.shown", slot: "banner", id: A, device: "pc-1", creative: "c-9" } as AdEvent);
  await flush();
  expect(Object.keys(posted[0]).sort()).toEqual(["id", "kind", "slot"]);
});

test("a post the Hub could not take is repeated with the same id, then given up", async () => {
  const { posted, emit, flush, retry, timers, metrics } = setup([503, "down", 500, 429, 503]);
  emit({ event: "ad.shown", slot: "banner", id: A });
  await flush();
  expect(metrics.pending).toBe(1);
  for (let i = 0; i < RETRY_MS.length; i++) {
    expect(timers.filter((t) => t.live).map((t) => t.ms)).toEqual([RETRY_MS[i]]);
    await retry();
  }
  expect(posted).toHaveLength(RETRY_MS.length + 1);
  expect(new Set(posted.map((p) => p.id))).toEqual(new Set([A]));
  expect(timers.some((t) => t.live)).toBe(false);
  expect(metrics.pending).toBe(0);
});

test("a repeat that gets through ends the wait", async () => {
  const { posted, emit, flush, retry, timers, metrics } = setup([503, 204]);
  emit({ event: "ad.requested", slot: "interstitial", id: A });
  await flush();
  await retry();
  expect(posted).toHaveLength(2);
  expect(timers.some((t) => t.live)).toBe(false);
  expect(metrics.pending).toBe(0);
});

test("a refusal that no repeat can fix is dropped at once", async () => {
  for (const status of [400, 404, 409, 422]) {
    const { posted, emit, flush, timers, metrics } = setup([status]);
    emit({ event: "ad.shown", slot: "banner", id: A });
    await flush();
    expect(posted).toHaveLength(1);
    expect(timers).toHaveLength(0);
    expect(metrics.pending).toBe(0);
  }
  expect(retryable(204)).toBe(false);
  expect(retryable(409)).toBe(false);
  expect([408, 429, 500, 503].every(retryable)).toBe(true);
});

test("waiting posts are bounded", async () => {
  const { posted, emit, flush, metrics } = setup([503]);
  for (let i = 0; i < MAX_PENDING + 20; i++) {
    emit({ event: "ad.requested", slot: "banner", id: `01JBLKAD${String(i).padStart(18, "0")}` });
  }
  await flush();
  expect(metrics.pending).toBe(MAX_PENDING);
  expect(posted).toHaveLength(MAX_PENDING);
});

test("after dispose nothing is listened to and no repeat fires", async () => {
  const { posted, emit, flush, timers, metrics, handlers } = setup([503]);
  emit({ event: "ad.shown", slot: "banner", id: A });
  await flush();
  metrics.dispose();
  expect(handlers.size).toBe(0);
  expect(timers.some((t) => t.live)).toBe(false);
  emit({ event: "ad.shown", slot: "banner", id: B });
  await flush();
  expect(posted).toHaveLength(1);
});
