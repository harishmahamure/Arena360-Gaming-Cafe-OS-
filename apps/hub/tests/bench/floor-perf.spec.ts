import { expect, test, type Page } from "@playwright/test";
import { CORS, HUB, MANAGER, device, json, snapshot } from "../floor-fixture";

const vars = (globalThis as { process?: { env: Record<string, string | undefined> } }).process?.env;
const env = (k: string, d: number) => Number(vars?.[k] ?? d);
const CHURN_S = env("FLOOR_BENCH_SECONDS", 60);
const SOAK_MIN = env("FLOOR_BENCH_SOAK_MINUTES", 0);
const COLD_MS = env("FLOOR_BENCH_COLD_MS", 1000);
const PAINT_MS = env("FLOOR_BENCH_PAINT_MS", 2000);
const FRAME_MS = env("FLOOR_BENCH_FRAME_MS", 100);
const INPUT_MS = env("FLOOR_BENCH_INPUT_MS", 100);
const HEAP_MB = env("FLOOR_BENCH_HEAP_MB", 250);
const N = 60;

type Bench = {
  push: (id: string, online: boolean) => void;
  latencies: number[];
  frames: number[];
  inputs: number[];
  coldMs: number | null;
};

declare global {
  interface Window {
    __bench: Bench;
  }
}

let states: boolean[] = [];
const id = (i: number) => device(i + 1, {}).device_id;
const snap = () => ({
  ...snapshot(
    states.map((online, i) =>
      device(i + 1, {
        zone: `Z${Math.floor(i / 15) + 1}`,
        floor_x: i % 5,
        floor_y: Math.floor((i % 15) / 5),
        online,
        status: online ? "online" : "offline",
      }),
    ),
  ),
  zones: [1, 2, 3, 4].map((n) => ({ id: `Z${n}`, name: `Zone ${n}`, sort_order: n, rev: 1 })),
});

const flip = (page: Page, i: number) => {
  states[i] = !states[i];
  return page.evaluate(([d, on]) => window.__bench.push(d, on), [id(i), states[i]] as const);
};

async function open(page: Page) {
  states = Array.from({ length: N }, () => true);
  await page.addInitScript(() => {
    sessionStorage.setItem("arena.hub.token", "manager-token");
    const listeners: (() => void)[] = [];
    class FakeEventSource {
      static CLOSED = 2;
      readyState = 1;
      addEventListener(type: string, fn: () => void) {
        if (type === "devices.changed") listeners.push(fn);
      }
      close() {}
    }
    Object.assign(window, { EventSource: FakeEventSource });
    const pending = new Map<string, [number, boolean]>();
    const b: Bench = {
      latencies: [],
      frames: [],
      inputs: [],
      coldMs: null,
      push: (d, online) => {
        pending.set(d, [performance.now(), online]);
        listeners.forEach((fn) => fn());
      },
    };
    window.__bench = b;
    new MutationObserver(() => {
      if (
        b.coldMs === null &&
        document.querySelectorAll("[data-component=floor-tile]").length >= 60
      )
        requestAnimationFrame(() => (b.coldMs ??= performance.now()));
      for (const [d, [t0, online]] of pending) {
        const el = document.querySelector(`[data-device-id="${d}"]`);
        if (el?.classList.contains("state-offline") === online) continue;
        pending.delete(d);
        requestAnimationFrame(() => b.latencies.push(performance.now() - t0));
      }
    }).observe(document, {
      subtree: true,
      childList: true,
      attributes: true,
      attributeFilter: ["class"],
    });
    let last = 0;
    const frame = (t: number) => {
      if (last) b.frames.push(t - last);
      last = t;
      requestAnimationFrame(frame);
    };
    requestAnimationFrame(frame);
    let t0: number | null = null;
    const start = (e: Event) => (t0 = e.timeStamp);
    const done = () => {
      const from = t0;
      t0 = null;
      if (from !== null) requestAnimationFrame(() => b.inputs.push(performance.now() - from));
    };
    addEventListener("keydown", start, true);
    addEventListener("wheel", start, true);
    addEventListener("focusin", done, true);
    addEventListener("scroll", done, true);
  });
  await page.route(`${HUB}/**`, async (route) => {
    if (route.request().method() === "OPTIONS")
      return route.fulfill({ status: 204, headers: CORS });
    switch (new URL(route.request().url()).pathname) {
      case "/ui/snapshot":
        return json(route, snap());
      case "/ui/me":
        return json(route, MANAGER);
      case "/ui/settings":
        return json(route, []);
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/floor");
  await page.waitForFunction(() => window.__bench.coldMs !== null);
}

const max = (xs: number[]) => Math.max(0, ...xs);

test("floor with 60 devices meets the performance budgets", async ({ page }) => {
  test.setTimeout((CHURN_S + SOAK_MIN * 60) * 1000 + 120_000);
  const misses: string[] = [];
  const check = (name: string, value: number, limit: number, unit: string) => {
    const line = `${name}: ${value.toFixed(1)} ${unit} (budget ${limit} ${unit})`;
    console.log(line);
    if (value > limit) misses.push(line);
  };
  await open(page);
  await expect(page.locator("[data-component=floor-tile]")).toHaveCount(N);
  check(
    "cold open, 60 tiles painted",
    (await page.evaluate(() => window.__bench.coldMs)) ?? 1e9,
    COLD_MS,
    "ms",
  );

  const ticks = CHURN_S * 10;
  for (let i = 0; i < ticks; i++) {
    await flip(page, i % N);
    await page.waitForTimeout(100);
  }
  await page.waitForTimeout(PAINT_MS);
  const { latencies, frames } = await page.evaluate(() => window.__bench);
  if (latencies.length < ticks) misses.push(`changes painted: ${latencies.length} of ${ticks}`);
  check("changes painted late (max)", max(latencies), PAINT_MS, "ms");
  check("longest frame", max(frames), FRAME_MS, "ms");

  await page.evaluate(() => (window.__bench.inputs.length = 0));
  await page.locator("[data-tile-face]").first().focus();
  for (let i = 1; i < N; i++) {
    await page.keyboard.press("ArrowRight");
    await page.mouse.wheel(0, 120);
  }
  await page.waitForTimeout(500);
  check(
    "input to paint (max)",
    max(await page.evaluate(() => window.__bench.inputs)),
    INPUT_MS,
    "ms",
  );

  if (SOAK_MIN > 0) {
    const cdp = await page.context().newCDPSession(page);
    await cdp.send("Performance.enable");
    let peak = 0;
    for (let m = 0; m < SOAK_MIN; m++) {
      for (let i = 0; i < 6; i++) {
        await flip(page, (m * 6 + i) % N);
        await page.waitForTimeout(10_000);
      }
      const { metrics } = await cdp.send("Performance.getMetrics");
      peak = Math.max(
        peak,
        (metrics.find((x) => x.name === "JSHeapUsedSize")?.value ?? 0) / 2 ** 20,
      );
    }
    check("floor JS heap after soak (peak)", peak, HEAP_MB, "MB");
  }
  expect(misses, misses.join("\n")).toEqual([]);
});
