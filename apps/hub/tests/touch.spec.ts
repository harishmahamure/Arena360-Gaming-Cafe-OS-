import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type {
  SessionStaff,
  SnapshotAlert,
  SnapshotBill,
  SnapshotDevice,
  SnapshotSession,
} from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { CORS, HUB, device, json, snapshot } from "./floor-fixture";

// The Hub by touch (Story 4.5): the 64px phone header with its chip row, the full-height tile
// sheet with its settle bar pinned, the Android Back stack (the Kotlin shell sends `arena:back`),
// state kept across the 840px breakpoint, and an automated 44 x 44 px target and axe audit on a
// phone (412 x 915) and a tablet (1280 x 800). The Hub's loopback API is stubbed with `page.route`.
// Both projects run the same explicit viewports, so the file runs once, in the desktop project.

const PHONE = { width: 412, height: 915 };
const TABLET = { width: 1280, height: 800 };
const MIN = 60_000;

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
const ALERT = {
  id: "A1",
  kind: "fixture",
  device_id: PC1.device_id,
  raised_at: Date.now() - 2 * MIN,
} as SnapshotAlert;

type Hub = {
  devices: SnapshotDevice[];
  offline: boolean;
  rule: boolean;
  alerts: SnapshotAlert[];
  /** PC 2 has a running session and no unpaid bill (so no settle bar, but Transfer). */
  live: boolean;
};

async function open(page: Page, path = "/floor", over: Partial<Hub> = {}) {
  const hub: Hub = {
    devices: [PC1, PC2, device(3, { status: "offline", online: false, floor_x: 2 })],
    offline: false,
    rule: false,
    alerts: [ALERT],
    live: false,
    ...over,
  };
  await page.addInitScript(() => {
    sessionStorage.setItem("arena.hub.token", "tok");
    class FakeEventSource {
      static CLOSED = 2;
      readyState = 1;
      addEventListener() {}
      close() {}
    }
    (window as unknown as Record<string, unknown>).EventSource = FakeEventSource;
  });
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot": {
        const snap = snapshot(hub.devices);
        snap.sessions = hub.live ? [SESSION] : [];
        snap.bills = hub.live ? [] : [BILL];
        snap.alerts = hub.alerts;
        if (hub.offline) snap.connectivity = { state: "internet_offline", checked_at: 1 };
        if (hub.rule) {
          snap.groups[0] = {
            ...snap.groups[0],
            active_rule: {
              rule_id: "R1",
              name: "Happy hour",
              adjust_kind: "percent_bp",
              adjust_value: -2000,
              rate_minor: 8000,
              until: Date.now() + 3600_000,
            },
          };
        }
        return json(route, snap);
      }
      case "/ui/me":
        return json(route, STAFF);
      case "/ui/settings":
        return json(route, []);
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto(path);
  return hub;
}

const tile = (page: Page, label: string) =>
  page.locator("[data-component=floor-tile]", { hasText: label });
const face = (page: Page, label: string) => tile(page, label).locator("[data-tile-face]");
const sheet = (page: Page) => page.getByRole("complementary");
const back = (page: Page) => page.evaluate(() => window.dispatchEvent(new Event("arena:back")));

async function openSheet(page: Page, label: string) {
  await face(page, label).focus();
  await page.keyboard.press("Enter");
  await expect(sheet(page)).toBeVisible();
}

async function serious(page: Page) {
  const found = (await new AxeBuilder({ page }).analyze()).violations.filter(
    (v) => v.impact === "serious" || v.impact === "critical",
  );
  expect(found.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual(
    [],
  );
}

/** The visible controls, and those of them smaller than 44 x 44 px. */
async function smallTargets(page: Page) {
  return page.evaluate(() => {
    const MIN = 44;
    const controls = document.querySelectorAll<HTMLElement>(
      'a[href], button, select, textarea, summary, [role="button"], [role="switch"], [role="tab"], [role="menuitem"], input:not([type="hidden"])',
    );
    const small: string[] = [];
    let checked = 0;
    for (const el of controls) {
      const style = getComputedStyle(el);
      if (style.visibility === "hidden" || style.display === "none") continue;
      if (el.closest("[inert]")) continue;
      if (el.matches(".visually-hidden, .visually-hidden *")) continue;
      // A native checkbox or radio is drawn by its label or wrapper, which is the target.
      const target = el.matches('input[type="checkbox"], input[type="radio"]')
        ? (el.closest("label") ?? el)
        : el;
      const box = target.getBoundingClientRect();
      if (box.width === 0 || box.height === 0) continue;
      checked++;
      if (box.width < MIN - 0.5 || box.height < MIN - 0.5) {
        const name = el.getAttribute("data-testid") ?? el.textContent?.trim().slice(0, 30) ?? "";
        small.push(
          `${el.tagName.toLowerCase()}[${name}] ${Math.round(box.width)}x${Math.round(box.height)}`,
        );
      }
    }
    return { small, checked };
  });
}

async function audit(page: Page) {
  const { small, checked } = await smallTargets(page);
  // The audit looked at real controls (the bottom bar or rail, at least).
  expect(checked).toBeGreaterThan(4);
  expect(small).toEqual([]);
  await serious(page);
}

test.beforeEach(({}, info) => {
  test.skip(info.project.name === "phone", "runs at its own viewports in the desktop project");
});

test.describe("phone header", () => {
  test.use({ viewport: PHONE });

  test("is 64px: branch, alert count and staff, no clock; chips sit in a row below", async ({
    page,
  }) => {
    await open(page, "/floor", { offline: true, rule: true });
    const header = page.locator("header.hub-header");
    expect((await header.boundingBox())?.height).toBe(64);
    await expect(header.getByTestId("hub-branch")).toHaveText("Dev Branch");
    await expect(header.getByTestId("hub-staff")).toContainText("Manager");
    await expect(header.getByTestId("hub-alerts")).toContainText("1");
    await expect(header.getByTestId("hub-clock")).toHaveCount(0);
    await expect(header.getByTestId("hub-chip-offline")).toHaveCount(0);
    await expect(header.getByTestId("chip-rule")).toHaveCount(0);
    const row = page.getByTestId("hub-chip-row");
    await expect(row.getByTestId("hub-chip-offline")).toBeVisible();
    await expect(row.getByTestId("chip-rule")).toBeVisible();
    const h = (await header.boundingBox())!;
    const r = (await row.boundingBox())!;
    expect(r.y).toBeGreaterThanOrEqual(h.y + h.height - 1);
    // Nothing is clipped sideways.
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(
      PHONE.width,
    );
  });

  test("without chips there is no chip row, and desktop keeps them in the header", async ({
    page,
  }) => {
    await open(page);
    await expect(page.getByTestId("hub-chip-row")).toHaveCount(0);
    await page.setViewportSize(TABLET);
    await expect(page.getByTestId("hub-clock")).toBeVisible();
    expect((await page.locator("header.hub-header").boundingBox())?.height).toBe(72);
  });

  test("the alert list stays inside the screen", async ({ page }) => {
    await open(page);
    await page.getByTestId("hub-alerts").click();
    const list = page.getByTestId("alert-list");
    await expect(list).toBeVisible();
    const box = (await list.boundingBox())!;
    expect(box.x).toBeGreaterThanOrEqual(0);
    expect(box.x + box.width).toBeLessThanOrEqual(PHONE.width);
  });
});

test.describe("tile sheet on a phone", () => {
  test.use({ viewport: PHONE });

  test("opens full height over an inert floor with the settle bar pinned at the bottom", async ({
    page,
  }) => {
    await open(page);
    await openSheet(page, "PC 2");
    const box = (await sheet(page).boundingBox())!;
    expect(box.x).toBe(0);
    expect(box.y).toBe(0);
    expect(box.width).toBe(PHONE.width);
    expect(box.height).toBe(PHONE.height);
    await expect(page.locator(".floor")).toHaveAttribute("inert", "");
    await expect(page.locator("header.hub-header")).toHaveAttribute("inert", "");
    await expect(page.locator("nav.hub-bottom-bar")).toHaveAttribute("inert", "");
    const region = page.getByTestId("settle-region");
    await expect(page.getByTestId("settle-bar")).toBeVisible();
    const pinned = async () => {
      const r = (await region.boundingBox())!;
      return Math.round(r.y + r.height);
    };
    expect(await pinned()).toBe(PHONE.height);
    // Scrolling the sheet's content leaves the bar where it is.
    await sheet(page).evaluate((el) => el.scrollTo(0, el.scrollHeight));
    expect(await pinned()).toBe(PHONE.height);
  });

  test("the settle bar stays pinned when the sheet scrolls in a short window", async ({ page }) => {
    await page.setViewportSize({ width: 412, height: 520 });
    await open(page);
    await openSheet(page, "PC 2");
    const region = page.getByTestId("settle-region");
    const bottom = async () => {
      const r = (await region.boundingBox())!;
      return Math.round(r.y + r.height);
    };
    expect(await bottom()).toBe(520);
    await sheet(page).evaluate((el) => el.scrollTo(0, 0));
    expect(await bottom()).toBe(520);
    await sheet(page).evaluate((el) => el.scrollTo(0, el.scrollHeight));
    expect(await bottom()).toBe(520);
    // The scroll padding of the sheet equals the pinned region's height.
    const [padding, height] = await Promise.all([
      sheet(page).evaluate((el) => parseFloat(getComputedStyle(el).scrollPaddingBottom)),
      region.evaluate((el) => el.getBoundingClientRect().height),
    ]);
    expect(Math.abs(padding - height)).toBeLessThan(1);
  });

  test("a focused field scrolls clear of the pinned settle bar, and again when the keyboard opens", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 412, height: 520 });
    await open(page);
    await openSheet(page, "PC 2");
    const field = page.getByLabel(en["hub.sheet.message_label"]);
    const clear = async () => {
      const f = (await field.boundingBox())!;
      const bar = (await page.getByTestId("settle-region").boundingBox())!;
      expect(f.y + f.height).toBeLessThanOrEqual(bar.y + 1);
      expect(f.y).toBeGreaterThanOrEqual(0);
    };
    await field.focus();
    await clear();
    // The keyboard opens: the window shrinks and the field stays in view above the bar.
    await page.setViewportSize({ width: 412, height: 330 });
    await expect
      .poll(async () => {
        const f = (await field.boundingBox())!;
        const bar = (await page.getByTestId("settle-region").boundingBox())!;
        return f.y + f.height <= bar.y + 1 && f.y >= 0;
      })
      .toBe(true);
  });

  test("Back and Esc close the sheet and return focus to the tile", async ({ page }) => {
    await open(page);
    await openSheet(page, "PC 2");
    await back(page);
    await expect(sheet(page)).toHaveCount(0);
    await expect(face(page, "PC 2")).toBeFocused();
    await expect(page.locator(".floor")).not.toHaveAttribute("inert", "");
    await openSheet(page, "PC 1");
    await page.keyboard.press("Escape");
    await expect(sheet(page)).toHaveCount(0);
    await expect(face(page, "PC 1")).toBeFocused();
  });

  test("Back on the floor with nothing open does nothing", async ({ page }) => {
    await open(page);
    const url = page.url();
    await back(page);
    await back(page);
    await expect(page).toHaveURL(url);
    await expect(page.locator("main h1")).toHaveText(en["hub.nav.floor"]);
    await expect(sheet(page)).toHaveCount(0);
  });

  test("Back closes the More sheet and returns focus to More, then does nothing", async ({
    page,
  }) => {
    await open(page);
    const more = page.getByRole("button", { name: en["hub.nav.more"] });
    await more.click();
    await expect(page.getByRole("dialog", { name: en["hub.more.title"] })).toBeVisible();
    await back(page);
    await expect(page.getByTestId("more-sheet-layer")).toHaveCount(0);
    await expect(more).toBeFocused();
    await back(page);
    await expect(page).toHaveURL(/\/floor/);
  });

  test("Back closes the alert list and returns focus to its button", async ({ page }) => {
    await open(page);
    const button = page.getByTestId("hub-alerts");
    await button.click();
    await expect(page.getByTestId("alert-list")).toBeVisible();
    await back(page);
    await expect(page.getByTestId("alert-list")).toHaveCount(0);
    await expect(button).toBeFocused();
  });

  test("Back is swallowed on the lock screen: the sheet under it stays", async ({ page }) => {
    await page.clock.install();
    await page.route(`${HUB}/ui/lock`, (route) =>
      route.request().method() === "OPTIONS"
        ? route.fulfill({ status: 204, headers: CORS })
        : json(route, {}),
    );
    await open(page);
    await openSheet(page, "PC 2");
    await page.clock.runFor(5 * MIN + 1000);
    await expect(page.getByTestId("hub-lock")).toBeVisible();
    await back(page);
    await expect(page.getByTestId("hub-lock")).toBeVisible();
    // (Hidden from the accessibility tree under the lock, so found by test id.)
    await expect(page.getByTestId("tile-sheet")).toHaveCount(1);
  });

  test("a transfer pick keeps the floor in reach: Back ends the pick, then closes the sheet", async ({
    page,
  }) => {
    await open(page, "/floor", { live: true });
    await openSheet(page, "PC 2");
    await expect(page.locator(".floor")).toHaveAttribute("inert", "");
    await sheet(page).getByRole("button", { name: en["hub.session.transfer"] }).click();
    await expect(page.getByTestId("transfer-hint")).toBeVisible();
    // The floor is in reach: the sheet is a card at the bottom and the free PC is a target.
    await expect(page.locator(".floor")).not.toHaveAttribute("inert", "");
    const card = (await sheet(page).boundingBox())!;
    expect(card.height).toBeLessThan(PHONE.height);
    await expect(tile(page, "PC 1").getByTestId("transfer-move-here")).toBeVisible();
    await back(page);
    await expect(page.getByTestId("transfer-hint")).toHaveCount(0);
    await expect(sheet(page)).toBeVisible();
    await back(page);
    await expect(sheet(page)).toHaveCount(0);
    await expect(face(page, "PC 2")).toBeFocused();
  });
});

test.describe("rotation and the 840px breakpoint", () => {
  test.use({ viewport: PHONE });

  test("the screen, scroll, open sheet and typed text survive crossing 840px both ways", async ({
    page,
  }) => {
    await open(page);
    await openSheet(page, "PC 1");
    const field = page.getByLabel(en["hub.sheet.message_label"]);
    await field.fill("back in five");
    await page.setViewportSize({ width: 915, height: 412 });
    await expect(page.locator("nav.hub-rail")).toBeVisible();
    await expect(sheet(page)).toHaveAccessibleName("PC 1");
    await expect(field).toHaveValue("back in five");
    // Docked right at 840px and wider, as in Story 3.5.
    const docked = (await sheet(page).boundingBox())!;
    expect(docked.x).toBeGreaterThan(300);
    await expect(page.locator(".floor")).not.toHaveAttribute("inert", "");
    await page.setViewportSize(PHONE);
    await expect(sheet(page)).toHaveAccessibleName("PC 1");
    await expect(field).toHaveValue("back in five");
    expect((await sheet(page).boundingBox())?.width).toBe(PHONE.width);
    await expect(page.locator(".floor")).toHaveAttribute("inert", "");
  });

  test("the More sheet closes only when the bottom bar disappears", async ({ page }) => {
    await open(page);
    await page.getByRole("button", { name: en["hub.nav.more"] }).click();
    await page.setViewportSize({ width: 600, height: 915 });
    await expect(page.getByTestId("more-sheet-layer")).toBeVisible();
    await page.setViewportSize({ width: 1000, height: 600 });
    await expect(page.getByTestId("more-sheet-layer")).toHaveCount(0);
  });
});

for (const [name, viewport] of [
  ["phone 412 x 915", PHONE],
  ["tablet 1280 x 800", TABLET],
] as const) {
  test.describe(`target size and axe: ${name}`, () => {
    test.use({ viewport });

    test("Floor", async ({ page }) => {
      await open(page, "/floor", { offline: true, rule: true });
      await expect(tile(page, "PC 1")).toBeVisible();
      await audit(page);
    });

    test("tile sheet with settle bar", async ({ page }) => {
      await open(page);
      await openSheet(page, "PC 2");
      await expect(page.getByTestId("settle-bar")).toBeVisible();
      await audit(page);
    });

    test("tile sheet of a free PC", async ({ page }) => {
      await open(page);
      await openSheet(page, "PC 1");
      await expect(sheet(page).getByRole("button", { name: "Unlock", exact: true })).toBeVisible();
      await audit(page);
    });

    test("Players", async ({ page }) => {
      await open(page, "/players");
      await expect(
        page.getByRole("heading", { level: 1, name: en["hub.nav.players"] }),
      ).toBeVisible();
      await audit(page);
    });

    test("Shift", async ({ page }) => {
      await open(page, "/shift");
      await expect(
        page.getByRole("heading", { level: 1, name: en["hub.nav.shift"] }),
      ).toBeVisible();
      await audit(page);
    });

    test("More sheet", async ({ page }, info) => {
      test.skip(viewport.width >= 840, "the More sheet is the bottom bar's");
      void info;
      await open(page);
      await page.getByRole("button", { name: en["hub.nav.more"] }).click();
      await expect(page.getByRole("dialog", { name: en["hub.more.title"] })).toBeVisible();
      await audit(page);
    });
  });
}
