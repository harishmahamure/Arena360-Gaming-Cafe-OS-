import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type TestInfo } from "@playwright/test";
import type { HubSnapshot } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };

// The Hub app shell: landmarks, rail and bottom bar, placeholder screens, the More sheet,
// Ctrl+1–4 and the unreachable-Hub state. The snapshot is served with `page.route`, and a
// signed-in Manager session is stubbed the same way (sign-in itself is in auth.spec.ts).

const SNAPSHOT_URL = "http://127.0.0.1:7409/ui/snapshot";
const ME_URL = "http://127.0.0.1:7409/ui/me";
const SETTINGS_URL = "http://127.0.0.1:7409/ui/settings";
const CORS = {
  "access-control-allow-origin": "http://localhost:5173",
  "access-control-allow-methods": "GET, POST",
  "access-control-allow-headers": "authorization, content-type",
};
const SEEDED: HubSnapshot = {
  branch: { name: "Dev Branch", branch_code: "DEV01" },
  devices: [
    { device_id: "01JDEVKIOSK0000000000000000", label: "Dev Kiosk", class: "paired", zone: null },
  ],
  locale: {
    currency: "INR",
    exponent: 2,
    symbol: "₹",
    symbol_position: "before",
    number_format: "indian",
    tz: "Asia/Kolkata",
    day_start_min: 360,
    fy_start_month: 4,
    pending: null,
  },
  ui: { idle_lock_minutes: 5 },
};
const NOT_SEEDED: HubSnapshot = {
  branch: null,
  devices: [],
  locale: null,
  ui: { idle_lock_minutes: 5 },
};

const RAIL = [
  ["floor", en["hub.nav.floor"]],
  ["pos", en["hub.nav.pos"]],
  ["players", en["hub.nav.players"]],
  ["sessions", en["hub.nav.sessions"]],
  ["inventory", en["hub.nav.inventory"]],
  ["shift", en["hub.nav.shift"]],
  ["reports", en["hub.nav.reports"]],
  ["settings", en["hub.nav.settings"]],
  ["system", en["hub.nav.system"]],
] as const;
// Settings (settings.spec.ts) and System (audit.spec.ts) have their own sections; every other
// unbuilt screen is a placeholder.
const PLACEHOLDERS = RAIL.filter(([id]) => id !== "floor" && id !== "settings" && id !== "system");
const BAR = [
  en["hub.nav.floor_short"],
  en["hub.nav.pos"],
  en["hub.nav.players"],
  en["hub.nav.shift"],
  en["hub.nav.more"],
];
const MORE = ["sessions", "inventory", "reports", "settings", "system"] as const;

const isPhone = (info: TestInfo) => info.project.name === "phone";

/** A snapshot, a refused connection, or a raw HTTP answer. */
type Serve = HubSnapshot | "down" | { status: number; body: string };

/** Serves `state.snapshot` for every request; returns the mutable state and a request counter. */
async function serve(page: Page, initial: Serve) {
  const state = { snapshot: initial, requests: 0, delayMs: 0 };
  await page.addInitScript(() => sessionStorage.setItem("arena.hub.token", "shell-token"));
  for (const [url, body] of [
    [
      ME_URL,
      { staff_id: "01JMANAGER0000000000000000", display_name: "Manager", role_name: "Manager" },
    ],
    [SETTINGS_URL, [{ key: "receipt.footer", value: "", rev: 1 }]],
  ] as const) {
    await page.route(url, (route) =>
      route.request().method() === "OPTIONS"
        ? route.fulfill({ status: 204, headers: CORS })
        : route.fulfill({
            status: 200,
            contentType: "application/json",
            headers: CORS,
            body: JSON.stringify(body),
          }),
    );
  }
  await page.route(SNAPSHOT_URL, async (route) => {
    state.requests++;
    if (state.delayMs) await new Promise((r) => setTimeout(r, state.delayMs));
    const answer = state.snapshot;
    if (answer === "down") return route.abort("connectionrefused");
    const raw =
      "status" in answer && "body" in answer
        ? (answer as { status: number; body: string })
        : { status: 200, body: JSON.stringify(answer) };
    return route.fulfill({
      status: raw.status,
      contentType: "application/json",
      headers: { "access-control-allow-origin": "http://localhost:5173" },
      body: raw.body,
    });
  });
  return state;
}

async function open(page: Page, path: string, snapshot: Serve = SEEDED) {
  const state = await serve(page, snapshot);
  await page.goto(path);
  await expect(page.locator("h1")).toHaveCount(1);
  return state;
}

/** The navigation landmark that is visible at this width. */
const visibleNav = (page: Page) => page.locator("nav:visible");

async function expectNoSeriousAxe(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  expect(bad.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual(
    [],
  );
}

test("/ redirects to /floor", async ({ page }) => {
  await open(page, "/");
  await expect(page).toHaveURL(/\/floor$/);
});

test("seeded: the header names the branch and the floor lists Dev Kiosk as unassigned", async ({
  page,
}) => {
  await open(page, "/floor");
  await expect(page.locator("header")).toContainText("Dev Branch");
  await expect(page.locator("header time")).toHaveText(/\d{1,2}:\d{2}/);
  await expect(page.locator("main h1")).toHaveText(en["hub.nav.floor"]);
  await expect(page.locator("main")).toContainText(en["hub.floor.no_zones"]);
  const section = page.locator("section", {
    has: page.getByRole("heading", { level: 2, name: en["hub.floor.unassigned"] }),
  });
  const item = section.getByRole("listitem");
  await expect(item).toHaveCount(1);
  await expect(item).toContainText("Dev Kiosk");
  await expect(item).toContainText("Unassigned");
  const border = await item.evaluate((el) => {
    const s = getComputedStyle(el);
    const probe = document.createElement("span");
    probe.style.color = "var(--state-unassigned)";
    document.body.append(probe);
    const expected = getComputedStyle(probe).color;
    probe.remove();
    return { style: s.borderTopStyle, width: s.borderTopWidth, color: s.borderTopColor, expected };
  });
  expect(border.style).toBe("dashed");
  expect(border.width).toBe("2px");
  expect(border.color).toBe(border.expected);
});

test("not seeded: the floor says the branch is not set up", async ({ page }) => {
  await open(page, "/floor", NOT_SEEDED);
  await expect(page.getByTestId("floor-not-set-up")).toContainText(en["hub.floor.not_set_up"]);
  await expect(page.locator("header")).toContainText(en["hub.header.no_branch"]);
  await expect(page.getByRole("heading", { level: 2 })).toHaveCount(0);
});

test("hub down: the error state offers Retry, and Retry refetches", async ({ page }) => {
  const state = await open(page, "/floor", "down");
  await expect(page.getByTestId("hub-unreachable")).toBeVisible();
  await expect(page.locator("main h1")).toHaveText(en["hub.error.unreachable.title"]);
  await expect(page.locator("header")).toBeVisible();
  await expect(visibleNav(page)).toHaveCount(1);
  await expectNoSeriousAxe(page);
  const before = state.requests;
  const retry = page.getByRole("button", { name: en["common.retry"] });
  const handle = await retry.elementHandle();
  // While the refetch is in flight the same Retry button is disabled with a reason.
  state.delayMs = 500;
  await retry.click();
  await expect(retry).toHaveAttribute("aria-disabled", "true");
  await expect(page.getByText(en["hub.error.retrying"])).toBeVisible();
  await expect(page.getByRole("status")).toHaveText(en["hub.error.retrying_status"]);
  expect(await handle?.evaluate((el) => el.isConnected)).toBe(true);
  // Still down: Retry is enabled again.
  await expect(retry).not.toHaveAttribute("aria-disabled", "true");
  state.snapshot = SEEDED;
  await retry.click();
  await expect(page.locator("header")).toContainText("Dev Branch");
  await expect(page.locator("main")).toContainText("Dev Kiosk");
  expect(state.requests).toBeGreaterThan(before);
  // Focus lands on the recovered screen's heading.
  await expect(page.locator("main h1")).toBeFocused();
});

test("hub down: retries on its own with backoff, then recovers", async ({ page }) => {
  await page.clock.install();
  const state = await open(page, "/floor", "down");
  const status = page.getByTestId("hub-retry-status");
  await expect(status).toHaveAttribute("role", "status");
  await expect(status).toHaveText(en["hub.error.next_retry"].replace("{seconds}", "2"));
  const first = state.requests;
  await page.clock.runFor(2_000);
  await expect.poll(() => state.requests).toBe(first + 1);
  await expect(status).toHaveText(en["hub.error.next_retry"].replace("{seconds}", "4"));
  await page.clock.runFor(4_000);
  await expect.poll(() => state.requests).toBe(first + 2);
  await expect(status).toHaveText(en["hub.error.next_retry"].replace("{seconds}", "8"));
  state.snapshot = SEEDED;
  await page.clock.runFor(8_000);
  await expect(page.locator("main")).toContainText("Dev Kiosk");
  await expect(page.locator("main h1")).toBeFocused();
  // Recovered: no more automatic retries.
  const settled = state.requests;
  await page.clock.runFor(60_000);
  expect(state.requests).toBe(settled);
});

test("hub down: the shell's ready event and window focus re-read the snapshot", async ({
  page,
}) => {
  const state = await open(page, "/floor", "down");
  state.snapshot = SEEDED;
  await page.evaluate(() => window.dispatchEvent(new Event("arena:hub-ready")));
  await expect(page.locator("main")).toContainText("Dev Kiosk");
  const before = state.requests;
  state.snapshot = { ...SEEDED, branch: { name: "Renamed Branch", branch_code: "DEV01" } };
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await expect(page.locator("header")).toContainText("Renamed Branch");
  expect(state.requests).toBeGreaterThan(before);
});

for (const [name, answer] of [
  ["a 500", { status: 500, body: JSON.stringify({ code: "INTERNAL" }) }],
  ["a 403", { status: 403, body: "" }],
  ["a malformed body", { status: 200, body: "not json" }],
  ["a wrong shape", { status: 200, body: JSON.stringify({ branch: { name: 7 }, devices: [] }) }],
  [
    "a bad device",
    {
      status: 200,
      body: JSON.stringify({ branch: null, devices: [{ device_id: "D", label: "x", zone: 1 }] }),
    },
  ],
] as const) {
  test(`hub answers ${name}: the unreachable state`, async ({ page }) => {
    await open(page, "/floor", answer);
    await expect(page.getByTestId("hub-unreachable")).toBeVisible();
    await expect(page.locator("main")).not.toContainText("Dev Kiosk");
  });
}

test("header clock advances across a minute boundary", async ({ page }) => {
  await page.clock.install({ time: new Date(2026, 0, 1, 10, 59, 50) });
  await open(page, "/floor");
  const clock = page.locator("header time");
  await expect(clock).toHaveText(/10:59/);
  await page.clock.runFor(15_000);
  await expect(clock).toHaveText(/11:00/);
});

test("landmarks, one h1 and no serious axe violations on every screen", async ({ page }) => {
  await open(page, "/floor");
  for (const [id] of RAIL) {
    await page.goto(`/${id}`);
    await expect(page.locator("header")).toHaveCount(1);
    await expect(page.locator("main")).toHaveCount(1);
    // One visible Hub navigation (Settings adds its own section list beside it).
    const hubNav = visibleNav(page).and(page.locator(`[aria-label="${en["hub.nav.label"]}"]`));
    await expect(hubNav).toHaveCount(1);
    await expect(visibleNav(page)).toHaveCount(id === "settings" || id === "system" ? 2 : 1);
    await expect(page.locator("h1")).toHaveCount(1);
    await expectNoSeriousAxe(page);
  }
});

test("rail: every screen in order, icon plus label, current item marked", async ({
  page,
}, info) => {
  test.skip(isPhone(info), "the rail is desktop-only");
  await open(page, "/floor");
  await expect(page.locator(".hub-bottom-bar")).toBeHidden();
  const rail = page.getByRole("navigation", { name: en["hub.nav.label"] });
  const links = rail.getByRole("link");
  await expect(links).toHaveText(RAIL.map(([, label]) => label));
  for (const [i, [id]] of RAIL.entries()) {
    await expect(links.nth(i)).toHaveAttribute("href", `/${id}`);
    await expect(links.nth(i).locator("svg[aria-hidden='true']")).toHaveCount(1);
  }
  const current = links.nth(0);
  await expect(current).toHaveAttribute("aria-current", "page");
  await expect(rail.locator("[aria-current]")).toHaveCount(1);
  // Not colour alone: the current item has a fill and a heavier label.
  const look = (i: number) =>
    links.nth(i).evaluate((el) => ({
      bg: getComputedStyle(el).backgroundColor,
      weight: Number(getComputedStyle(el.querySelector("span") as Element).fontWeight),
    }));
  const on = await look(0);
  const off = await look(1);
  expect(on.bg).not.toBe(off.bg);
  expect(on.bg).not.toBe("rgba(0, 0, 0, 0)");
  expect(on.weight).toBeGreaterThan(off.weight);
});

test("placeholder screens: clicking each rail item opens it with a single h1", async ({
  page,
}, info) => {
  test.skip(isPhone(info), "the rail is desktop-only");
  await open(page, "/floor");
  const rail = page.getByRole("navigation", { name: en["hub.nav.label"] });
  for (const [id, label] of PLACEHOLDERS) {
    await rail.getByRole("link", { name: label, exact: true }).click();
    await expect(page).toHaveURL(new RegExp(`/${id}$`));
    await expect(page.locator("h1")).toHaveCount(1);
    await expect(page.locator("main h1")).toHaveText(label);
    await expect(page.locator("[data-screen-placeholder]")).toContainText(
      en["hub.placeholder.body"],
    );
    await expect(rail.getByRole("link", { name: label, exact: true })).toHaveAttribute(
      "aria-current",
      "page",
    );
  }
});

test("bottom bar: Floor · POS · Players · Shift · More replace the rail", async ({
  page,
}, info) => {
  test.skip(!isPhone(info), "the bottom bar is phone-only");
  await open(page, "/floor");
  await expect(page.locator(".hub-rail")).toBeHidden();
  const bar = page.locator(".hub-bottom-bar");
  await expect(bar).toBeVisible();
  const box = await bar.boundingBox();
  expect(box?.height).toBe(64);
  await expect(bar.locator("a, button")).toHaveText(BAR);
  await expect(bar.getByRole("link", { name: en["hub.nav.floor_short"] })).toHaveAttribute(
    "aria-current",
    "page",
  );
  await bar.getByRole("link", { name: en["hub.nav.players"] }).click();
  await expect(page).toHaveURL(/\/players$/);
});

test("More sheet: lists the rest, traps focus, Esc and backdrop close it back to More", async ({
  page,
}, info) => {
  test.skip(!isPhone(info), "the More sheet is phone-only");
  await open(page, "/pos");
  const more = page.getByRole("button", { name: en["hub.nav.more"] });
  await more.click();
  const sheet = page.getByRole("dialog", { name: en["hub.more.title"] });
  await expect(sheet).toBeVisible();
  await expect(sheet).toHaveAttribute("aria-modal", "true");
  await expect(more).toHaveAttribute("aria-expanded", "true");
  await expect(sheet.getByRole("link")).toHaveText(MORE.map((id) => en[`hub.nav.${id}`]));
  await expectNoSeriousAxe(page);

  // Focus starts inside and stays inside.
  await expect(sheet.getByRole("link").first()).toBeFocused();
  const stops = (await sheet.locator("a, button").count()) + 2;
  for (let i = 0; i < stops; i++) {
    await page.keyboard.press("Tab");
    expect(await sheet.evaluate((el) => el.contains(document.activeElement))).toBe(true);
  }
  await page.keyboard.press("Shift+Tab");
  expect(await sheet.evaluate((el) => el.contains(document.activeElement))).toBe(true);

  // Esc: closed, focus back on More, same screen.
  await page.keyboard.press("Escape");
  await expect(sheet).toHaveCount(0);
  await expect(more).toBeFocused();
  await expect(page).toHaveURL(/\/pos$/);

  // Backdrop: same.
  await more.click();
  await expect(sheet).toBeVisible();
  await page.getByTestId("more-sheet-backdrop").click({ position: { x: 20, y: 20 } });
  await expect(sheet).toHaveCount(0);
  await expect(more).toBeFocused();
  await expect(page).toHaveURL(/\/pos$/);

  // Picking the current screen just closes the sheet, back to More.
  await page.goto("/sessions");
  await more.click();
  await sheet.getByRole("link", { name: en["hub.nav.sessions"] }).click();
  await expect(sheet).toHaveCount(0);
  await expect(more).toBeFocused();
  await expect(page).toHaveURL(/\/sessions$/);

  // Picking a row opens that screen.
  await more.click();
  await sheet.getByRole("link", { name: en["hub.nav.reports"] }).click();
  await expect(sheet).toHaveCount(0);
  await expect(page).toHaveURL(/\/reports$/);
  await expect(page.locator("main h1")).toHaveText(en["hub.nav.reports"]);
  await expect(more).toHaveAttribute("data-active", "true");
});

test("More sheet closes when the window grows past 840px, focus on main", async ({
  page,
}, info) => {
  test.skip(!isPhone(info), "the More sheet is phone-only");
  await open(page, "/pos");
  await page.getByRole("button", { name: en["hub.nav.more"] }).click();
  await expect(page.getByRole("dialog")).toBeVisible();
  await page.setViewportSize({ width: 1280, height: 800 });
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(page.locator("main")).toBeFocused();
  await expect(page).toHaveURL(/\/pos$/);
  // The rail is usable again (the shell is no longer inert).
  await page.getByRole("navigation").getByRole("link", { name: en["hub.nav.players"] }).click();
  await expect(page).toHaveURL(/\/players$/);
});

test("Ctrl+1–4 open Floor, POS, Players and Shift", async ({ page }) => {
  await open(page, "/reports");
  await page.locator("body").focus();
  for (const [key, id] of [
    ["3", "players"],
    ["2", "pos"],
    ["4", "shift"],
    ["1", "floor"],
  ]) {
    await page.keyboard.press(`Control+${key}`);
    await expect(page).toHaveURL(new RegExp(`/${id}$`));
  }
  // A held key's auto-repeat: ignored.
  await page.evaluate(() =>
    document.body.dispatchEvent(
      new KeyboardEvent("keydown", { key: "3", ctrlKey: true, repeat: true, bubbles: true }),
    ),
  );
  await expect(page).toHaveURL(/\/floor$/);
  // Alt or Meta held: ignored.
  await page.keyboard.press("Control+Alt+3");
  await page.keyboard.press("Control+Meta+3");
  await expect(page).toHaveURL(/\/floor$/);
});

test("Ctrl+3 is ignored while a text field has focus", async ({ page }) => {
  await open(page, "/reports");
  // A test-only field on the placeholder screen.
  for (const markup of [
    '<input data-test-field aria-label="x">',
    '<textarea data-test-field aria-label="x"></textarea>',
    '<select data-test-field aria-label="x"><option>a</option></select>',
    '<div data-test-field contenteditable="true" tabindex="0">x</div>',
  ]) {
    await page.locator("[data-screen-placeholder]").evaluate((el, html) => {
      el.querySelector("[data-test-field]")?.remove();
      el.insertAdjacentHTML("beforeend", html);
    }, markup);
    await page.locator("[data-test-field]").focus();
    await page.keyboard.press("Control+3");
    await expect(page).toHaveURL(/\/reports$/);
  }
  await page.locator("[data-test-field]").evaluate((el) => (el as HTMLElement).blur());
  await page.keyboard.press("Control+3");
  await expect(page).toHaveURL(/\/players$/);
});

test("targets: every link and button is at least 44 × 44 px", async ({ page }, info) => {
  const check = async (where: string) => {
    const small = await page.evaluate(() =>
      [...document.querySelectorAll<HTMLElement>('a, button, [tabindex]:not([tabindex="-1"])')]
        .filter((el) => el.offsetParent !== null || getComputedStyle(el).position === "fixed")
        .map((el) => ({ el, r: el.getBoundingClientRect() }))
        .filter(({ r }) => r.width > 0 && (r.width < 44 || r.height < 44))
        .map(({ el, r }) => `${el.tagName} "${el.textContent?.trim()}" ${r.width}×${r.height}`),
    );
    expect(small, where).toEqual([]);
  };
  await open(page, "/floor");
  await check("floor");
  if (isPhone(info)) {
    await page.getByRole("button", { name: en["hub.nav.more"] }).click();
    await expect(page.getByRole("dialog")).toBeVisible();
    await check("more sheet");
    await page.keyboard.press("Escape");
  }
  await page.unroute(SNAPSHOT_URL);
  await serve(page, "down");
  await page.reload();
  await expect(page.getByTestId("hub-unreachable")).toBeVisible();
  await check("unreachable");
});
