import { expect, test, type Page } from "@playwright/test";
import type {
  SessionStaff,
  SnapshotBill,
  SnapshotDevice,
  SnapshotSession,
  Tier,
} from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { CORS, HUB, device, json, snapshot } from "./floor-fixture";

// The permanent ad-banner slot (Story 4.11): on a Free Android Hub the layout reserves a fixed
// strip that nothing covers and that covers nothing, at every supported size. The Hub's loopback
// API is stubbed with `page.route`, and a fake `ArenaAds` records the rectangle the page hands the
// plugin. Each size is its own describe block, so the file runs once, in the desktop project.

const ANDROID_UA =
  "Mozilla/5.0 (Linux; Android 14; SM-X110) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";
const SIZES = [
  { name: "phone", width: 412, height: 915 },
  { name: "tablet portrait", width: 800, height: 1280 },
  { name: "tablet landscape", width: 1280, height: 800 },
] as const;
/** The clear space between the slot and a docked settle bar. */
const GAP = 24;

const STAFF: SessionStaff = {
  staff_id: "01JMANAGER0000000000000000",
  display_name: "Manager",
  role_name: "Manager",
  permissions: ["device.remote", "device.manage", "device.screen", "session.start", "player.view"],
};
const PC1 = device(1, { status: "online" });
const PC2 = device(2, { status: "in_session", floor_x: 1 });
const BILL: SnapshotBill = {
  session_id: "S2",
  device_id: PC2.device_id,
  order_id: "O1",
  order_rev: 4,
  lines: [],
  subtotal_minor: 7500,
  paid_minor: 0,
  remaining_minor: 7500,
};
const SESSION: SnapshotSession = {
  id: "S2",
  device_id: PC2.device_id,
  state: "active",
  started_at: Date.now() - 3_725_000,
  paused_total_ms: 0,
  rev: 1,
  player_id: "P1",
  username: "asha",
};

type Options = { tier?: Tier; locked?: boolean; android?: boolean };

async function open(page: Page, path: string, { tier = "free", locked = false }: Options = {}) {
  const devices: SnapshotDevice[] = [PC1, PC2, device(3, { status: "offline", online: false })];
  await page.addInitScript(() => {
    sessionStorage.setItem("arena.hub.token", "tok");
    class FakeEventSource {
      static CLOSED = 2;
      readyState = 1;
      addEventListener() {}
      close() {}
    }
    (window as unknown as Record<string, unknown>).EventSource = FakeEventSource;
    const calls: string[] = [];
    (window as unknown as { __ads: string[] }).__ads = calls;
    (window as unknown as { ArenaAds: unknown }).ArenaAds = {
      init: (config: string) => (calls.push(`init ${config}`), JSON.stringify({ debug: false })),
      bannerPlace: (rect: string) => calls.push(`bannerPlace ${rect}`),
      bannerHide: () => calls.push("bannerHide"),
      bannerDestroy: () => calls.push("bannerDestroy"),
      interstitialPreload: () => calls.push("interstitialPreload"),
      interstitialShow: () => calls.push("interstitialShow"),
    };
  });
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot": {
        const snap = snapshot(devices);
        snap.sessions = [SESSION];
        snap.bills = [BILL];
        snap.tier = tier;
        return json(route, snap);
      }
      case "/ui/me":
        return json(route, { ...STAFF, locked, has_pin: true });
      case "/ui/settings":
        return json(route, []);
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto(path);
  await expect(page.locator("h1").first()).toBeVisible();
}

type Box = { x: number; y: number; width: number; height: number };
/** Every visible box the banner must stay clear of, by what it is. */
const OBSTACLES = [
  ["hub-main", ".hub-main"],
  ["rail", "nav.hub-rail"],
  ["bottom bar", "nav.hub-bottom-bar"],
  ["header", "header.hub-header"],
  ["floor tile", "[data-component=floor-tile]"],
  ["tile sheet", "[data-component=tile-sheet]"],
  ["settle region", "[data-testid=settle-region]"],
  ["settle bar", "[data-component=settle-bar]"],
  ["dialog", "[role=dialog]"],
  ["dialog panel", "[role=dialog] > *"],
] as const;

type Layout = {
  banner: Box | null;
  viewport: { width: number; height: number };
  others: { what: string; box: Box }[];
};

async function layout(page: Page): Promise<Layout> {
  return page.evaluate(
    (obstacles) => {
      const box = (el: Element): Box => {
        const r = el.getBoundingClientRect();
        return { x: r.left, y: r.top, width: r.width, height: r.height };
      };
      const banner = document.querySelector("[data-component=ad-banner]");
      const others: { what: string; box: Box }[] = [];
      for (const [what, selector] of obstacles) {
        for (const el of document.querySelectorAll(selector)) {
          const b = box(el);
          const style = getComputedStyle(el);
          if (b.width < 1 || b.height < 1 || style.visibility === "hidden") continue;
          others.push({ what, box: b });
        }
      }
      return {
        banner: banner ? box(banner) : null,
        viewport: { width: window.innerWidth, height: window.innerHeight },
        others,
      };
    },
    OBSTACLES as unknown as [string, string][],
  );
}

const overlap = (a: Box, b: Box) =>
  a.x < b.x + b.width - 0.5 &&
  b.x < a.x + a.width - 0.5 &&
  a.y < b.y + b.height - 0.5 &&
  b.y < a.y + a.height - 0.5;

/** The banner is fully visible and no tile, sheet, dialog, settle bar or bar meets it. */
async function expectClear(page: Page, what: string) {
  const { banner, viewport, others } = await layout(page);
  expect(banner, `${what}: the banner slot is drawn`).not.toBeNull();
  const b = banner!;
  expect(b.x, what).toBeGreaterThanOrEqual(0);
  expect(b.y, what).toBeGreaterThanOrEqual(0);
  expect(b.x + b.width, what).toBeLessThanOrEqual(viewport.width + 0.5);
  expect(b.y + b.height, what).toBeLessThanOrEqual(viewport.height + 0.5);
  expect(b.height, `${what}: fixed height`).toBe(60);
  const hits = others.filter((o) => overlap(o.box, b)).map((o) => o.what);
  expect(hits, `${what}: nothing meets the banner`).toEqual([]);
  // The last thing the slot is handed to the plugin: exactly this rect.
  const placed = await page.evaluate(() => {
    const calls = (window as unknown as { __ads: string[] }).__ads;
    const last = calls.filter((c) => c.startsWith("bannerPlace ")).at(-1);
    return last ? (JSON.parse(last.slice("bannerPlace ".length)) as Box) : null;
  });
  expect(placed, `${what}: placed with the plugin`).not.toBeNull();
  for (const k of ["x", "y", "width", "height"] as const) {
    expect(Math.abs(placed![k] - b[k]), `${what}: placed ${k}`).toBeLessThanOrEqual(1);
  }
  return { b, others, viewport };
}

const face = (page: Page, label: string) =>
  page.locator("[data-component=floor-tile]", { hasText: label }).locator("[data-tile-face]");

async function openSheet(page: Page, label: string) {
  await face(page, label).focus();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("complementary")).toBeVisible();
  await expect(page.getByTestId("settle-region")).toBeVisible();
}

test.beforeEach(({}, info) => {
  test.skip(info.project.name === "phone", "runs at its own viewports in the desktop project");
});

for (const size of SIZES) {
  test.describe(`${size.name} ${size.width} x ${size.height}`, () => {
    test.use({ viewport: { width: size.width, height: size.height }, userAgent: ANDROID_UA });
    const wide = size.width >= 840;

    test("floor: the slot sits under the main content, above the bottom bar, never under the rail", async ({
      page,
    }) => {
      await open(page, "/floor");
      const { b, others } = await expectClear(page, "floor");
      const main = others.find((o) => o.what === "hub-main")!.box;
      expect(main.y + main.height).toBeLessThanOrEqual(b.y + 0.5);
      if (wide) {
        const rail = others.find((o) => o.what === "rail")!.box;
        expect(b.x).toBeGreaterThanOrEqual(rail.x + rail.width - 0.5);
        expect(b.y - (main.y + main.height)).toBeGreaterThanOrEqual(GAP - 0.5);
        expect(b.y + b.height).toBeCloseTo(size.height, 0);
      } else {
        const bar = others.find((o) => o.what === "bottom bar")!.box;
        expect(b.y + b.height).toBeLessThanOrEqual(bar.y + 0.5);
        expect(bar.y - (b.y + b.height)).toBeLessThanOrEqual(1);
        expect(b.width).toBeCloseTo(size.width, 0);
      }
    });

    test("a tile with the settle bar open keeps its settle region clear of the slot", async ({
      page,
    }) => {
      await open(page, "/floor");
      await openSheet(page, "PC 2");
      const { b, others } = await expectClear(page, "tile sheet");
      const settle = others.find((o) => o.what === "settle region")!.box;
      expect(b.y - (settle.y + settle.height), "gap to the settle bar").toBeGreaterThanOrEqual(
        GAP - 0.5,
      );
      if (!wide) {
        // A full-height phone sheet ends at the slot's top edge.
        const sheet = others.find((o) => o.what === "tile sheet")!.box;
        expect(Math.abs(sheet.y + sheet.height - b.y)).toBeLessThanOrEqual(1);
      }
    });

    if (!wide) {
      test("the More sheet stops above the slot", async ({ page }) => {
        await open(page, "/floor");
        await page.getByRole("button", { name: en["hub.nav.more"] }).click();
        await expect(page.getByRole("dialog")).toBeVisible();
        const { b, others } = await expectClear(page, "more sheet");
        const sheet = others.filter((o) => o.what === "dialog panel").at(-1)!.box;
        expect(sheet.y + sheet.height).toBeLessThanOrEqual(b.y + 0.5);
      });
    }

    test("the idle lock screen stops above the slot", async ({ page }) => {
      await open(page, "/floor", { locked: true });
      await expect(page.getByTestId("hub-lock")).toBeVisible();
      const { b, others } = await expectClear(page, "lock screen");
      const lock = others.find((o) => o.what === "dialog")!.box;
      expect(lock.y + lock.height).toBeLessThanOrEqual(b.y + 0.5);
    });

    test("Settings scrolls above the slot", async ({ page }) => {
      await open(page, "/settings");
      const { b, others } = await expectClear(page, "settings");
      const main = others.find((o) => o.what === "hub-main")!.box;
      expect(main.y + main.height).toBeLessThanOrEqual(b.y + 0.5);
    });
  });
}

test.describe("the slot while there is no ad", () => {
  test.use({ viewport: { width: 412, height: 915 }, userAgent: ANDROID_UA });

  test("keeps its height and shows only the label whatever the plugin reports", async ({
    page,
  }) => {
    await open(page, "/floor");
    const slot = page.getByTestId("ad-banner");
    const before = await slot.boundingBox();
    expect(before?.height).toBe(60);
    await expect(slot).toHaveText(en["hub.ads.label"]);
    await page.evaluate(() => {
      for (const event of ["ad.requested", "ad.failed"]) {
        window.dispatchEvent(new CustomEvent("arena:ad", { detail: { event, slot: "banner" } }));
      }
    });
    await page.setViewportSize({ width: 360, height: 640 });
    const after = await slot.boundingBox();
    expect(after?.height).toBe(60);
    // Resized: the plugin is handed the new width.
    await expect
      .poll(() =>
        page.evaluate(() => {
          const calls = (window as unknown as { __ads: string[] }).__ads;
          const last = calls.filter((c) => c.startsWith("bannerPlace ")).at(-1)!;
          return (JSON.parse(last.slice("bannerPlace ".length)) as Box).width;
        }),
      )
      .toBe(360);
  });

  test("the label is hub-micro in muted ink on a surface strip with a line edge", async ({
    page,
  }) => {
    await open(page, "/floor");
    const slot = page.getByTestId("ad-banner");
    const styles = await slot.evaluate((el) => {
      const s = getComputedStyle(el);
      const probe = document.createElement("div");
      probe.style.cssText = "background-color: var(--surface); color: var(--text-muted)";
      document.body.append(probe);
      const want = getComputedStyle(probe);
      const label = getComputedStyle(el.querySelector(".label")!);
      const out = {
        bg: s.backgroundColor === want.backgroundColor,
        top: s.borderTopWidth,
        ink: label.color === want.color,
        micro: label.fontSize,
      };
      probe.remove();
      return out;
    });
    expect(styles.bg).toBe(true);
    expect(styles.top).toBe("1px");
    expect(styles.ink).toBe(true);
    const micro = await page.evaluate(() => {
      const p = document.createElement("p");
      p.className = "type-hub-micro";
      document.body.append(p);
      const size = getComputedStyle(p).fontSize;
      p.remove();
      return size;
    });
    expect(styles.micro).toBe(micro);
  });
});

test.describe("the gate off", () => {
  test.use({ viewport: { width: 412, height: 915 }, userAgent: ANDROID_UA });

  test("a paid Android Hub reserves nothing: no slot, no plugin call, the bottom bar at the edge", async ({
    page,
  }) => {
    await open(page, "/floor", { tier: "basic" });
    await expect(page.getByTestId("ad-banner")).toHaveCount(0);
    expect(await page.evaluate(() => document.documentElement.dataset.ads)).toBeUndefined();
    expect(await page.evaluate(() => (window as unknown as { __ads: string[] }).__ads)).toEqual([]);
    const bar = await page.locator("nav.hub-bottom-bar").boundingBox();
    expect(bar!.y + bar!.height).toBeCloseTo(915, 0);
  });
});

test.describe("not Android", () => {
  test.use({ viewport: { width: 1280, height: 800 } });

  test("the Windows Hub on Free draws no slot", async ({ page }) => {
    await open(page, "/floor");
    await expect(page.getByTestId("ad-banner")).toHaveCount(0);
    expect(await page.evaluate(() => (window as unknown as { __ads: string[] }).__ads)).toEqual([]);
  });
});
