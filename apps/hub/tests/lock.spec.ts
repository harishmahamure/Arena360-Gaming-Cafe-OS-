import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type { HubSnapshot, SessionStaff } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };

// The Hub UI idle lock: after `idle_lock_minutes` without input an overlay covers the
// (still mounted) shell; a PIN unlocks it in place, and another member's PIN switches the
// session to them. The Hub's loopback API is stubbed with `page.route` (PINs: Counter 3333,
// Manager 2222); time is `page.clock`.

const HUB = "http://127.0.0.1:7409";
const CORS = {
  "access-control-allow-origin": "http://localhost:5173",
  "access-control-allow-methods": "GET, POST",
  "access-control-allow-headers": "authorization, content-type",
};
const MANAGER: SessionStaff = {
  staff_id: "01JMANAGER0000000000000000",
  display_name: "Mira",
  role_name: "Manager",
  permissions: ["settings.edit", "user.manage"],
};
const COUNTER: SessionStaff = {
  staff_id: "01JCOUNTER0000000000000000",
  display_name: "Cody",
  role_name: "Counter",
  permissions: ["session.start", "pos.sell"],
};
const PINS: Record<string, SessionStaff> = { "3333": COUNTER, "2222": MANAGER };
const AUTH_FAILED = { code: "AUTH_FAILED", message: "sign-in failed" };
const MIN = 60_000;

type Hub = {
  /** Who each token acts as now (an unlock with another PIN re-binds it). */
  sessions: Map<string, { staff: SessionStaff; locked: boolean }>;
  settings: Map<string, { value: string; rev: number }>;
  devices: HubSnapshot["devices"];
  idleLockMinutes: number;
  locks: number;
  unlocks: string[];
  /** Consecutive wrong unlock PINs, per locked staff member. */
  failures: Map<string, number>;
  /** Staff ids without a PIN. */
  noPin: Set<string>;
  intents: { actor: string; key: string; value: string }[];
  /** Events waiting for each open window's stream. */
  queues: Set<string[]>;
  snapshots: number;
  /** Idle-lock values the stub had to refuse (the UI should catch them first). */
  rejectedOnHub: number;
  /** How soon an idle stream reconnects (ms, browser time). A delivered event quiets it. */
  retry: number;
};

function newHub(): Hub {
  return {
    sessions: new Map([
      ["counter-token", { staff: COUNTER, locked: false }],
      ["manager-token", { staff: MANAGER, locked: false }],
    ]),
    settings: new Map([
      ["receipt.footer", { value: "", rev: 1 }],
      ["branch.currency", { value: "INR", rev: 1 }],
      ["branch.symbol_position", { value: "before", rev: 1 }],
      ["branch.number_format", { value: "indian", rev: 1 }],
      ["branch.tz", { value: "Asia/Kolkata", rev: 1 }],
      ["branch.day_start_min", { value: "360", rev: 1 }],
      ["branch.fy_start_month", { value: "4", rev: 1 }],
      ["idle_lock_minutes", { value: "5", rev: 1 }],
    ]),
    devices: [
      {
        device_id: "01JDEVKIOSK0000000000000001",
        label: "Kiosk 1",
        class: "paired",
        zone: null,
        online: false,
      },
      {
        device_id: "01JDEVKIOSK0000000000000002",
        label: "Kiosk 2",
        class: "paired",
        zone: null,
        online: false,
      },
    ],
    idleLockMinutes: 5,
    locks: 0,
    unlocks: [],
    failures: new Map(),
    noPin: new Set(),
    intents: [],
    queues: new Set(),
    snapshots: 0,
    retry: 60_000,
    rejectedOnHub: 0,
  };
}

function snapshot(hub: Hub): HubSnapshot {
  return {
    branch: { name: "Dev Branch", branch_code: "DEV01" },
    devices: hub.devices,
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
    ui: { idle_lock_minutes: hub.idleLockMinutes, hc: false },
    connectivity: { state: "online", checked_at: null },
  };
}

function json(route: Route, status: number, body: unknown) {
  return route.fulfill({
    status,
    contentType: "application/json",
    headers: CORS,
    body: JSON.stringify(body),
  });
}

async function stub(page: Page, hub: Hub, token: string | null) {
  if (token) await page.addInitScript((t) => sessionStorage.setItem("arena.hub.token", t), token);
  const queue: string[] = [];
  hub.queues.add(queue);
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    const url = new URL(req.url());
    const auth = (req.headers()["authorization"] ?? "").replace(/^Bearer /, "");
    const session = hub.sessions.get(auth);
    switch (url.pathname) {
      case "/ui/snapshot":
        hub.snapshots++;
        return json(route, 200, snapshot(hub));
      case "/ui/me":
        return session
          ? json(route, 200, {
              ...session.staff,
              locked: session.locked,
              has_pin: !hub.noPin.has(session.staff.staff_id),
            })
          : json(route, 401, AUTH_FAILED);
      case "/ui/auth/sign-out":
        hub.sessions.delete(auth);
        return route.fulfill({ status: 204, headers: CORS });
      case "/ui/settings":
        if (!session) return json(route, 401, AUTH_FAILED);
        return json(
          route,
          200,
          [...hub.settings].map(([key, row]) => ({ key, ...row })),
        );
      case "/ui/events": {
        const t = url.searchParams.get("token") ?? "";
        if (!hub.sessions.has(t)) return json(route, 401, AUTH_FAILED);
        // The stub answers with what is queued and ends; the browser reconnects shortly.
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: `retry: ${queue.length ? 60_000 : hub.retry}\n\n` + queue.splice(0).join(""),
        });
      }
      case "/ui/lock":
        if (!session) return json(route, 401, AUTH_FAILED);
        session.locked = true;
        hub.locks++;
        return route.fulfill({ status: 204, headers: CORS });
      case "/ui/unlock": {
        if (!session) return json(route, 401, AUTH_FAILED);
        const { pin } = req.postDataJSON() as { pin: string };
        if (!session.locked) {
          return json(route, 409, {
            code: "VALIDATION_FAILED",
            message: "this session is not locked",
            field: "session",
          });
        }
        hub.unlocks.push(pin);
        // The pause belongs to the locked member, whatever the session.
        const who = session.staff.staff_id;
        const paused = {
          code: "AUTH_LOCKED",
          message: "too many wrong PINs; try again later",
          retry_after_s: 60,
        };
        if ((hub.failures.get(who) ?? 0) >= 5) return json(route, 429, paused);
        const match = PINS[pin];
        if (!match) {
          const n = (hub.failures.get(who) ?? 0) + 1;
          hub.failures.set(who, n);
          if (n >= 5) return json(route, 429, paused);
          return json(route, 403, {
            code: "PIN_REJECTED",
            message: "that PIN can't unlock this screen",
          });
        }
        hub.failures.delete(who);
        const switched = match.staff_id !== session.staff.staff_id;
        session.staff = match;
        session.locked = false;
        return json(route, 200, { staff: match, switched });
      }
      case "/ui/intent": {
        if (!session) return json(route, 401, AUTH_FAILED);
        if (session.locked) {
          return json(route, 423, { code: "SESSION_LOCKED", message: "this screen is locked" });
        }
        const { intent } = req.postDataJSON() as {
          intent: { base_rev?: number; payload: { key: string; value: string } };
        };
        const { key, value } = intent.payload;
        hub.intents.push({ actor: session.staff.staff_id, key, value });
        if (!session.staff.permissions.includes("settings.edit")) {
          return json(route, 403, { code: "PERMISSION_DENIED", message: "denied" });
        }
        if (key === "idle_lock_minutes" && !/^([1-9]|[12]\d|30)$/.test(value)) {
          hub.rejectedOnHub++;
          return json(route, 422, {
            code: "VALIDATION_FAILED",
            message: "out of range",
            field: "value",
          });
        }
        const row = hub.settings.get(key)!;
        hub.settings.set(key, { value, rev: row.rev + 1 });
        if (key === "idle_lock_minutes") hub.idleLockMinutes = Number(value);
        pushSettingsChanged(hub, [key]);
        return json(route, 200, { applied: { intent_id: "I", changes: [] } });
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
}

function pushSettingsChanged(hub: Hub, keys: string[]) {
  const data = JSON.stringify({
    locale: snapshot(hub).locale,
    keys,
    ui: { idle_lock_minutes: hub.idleLockMinutes, hc: false },
    connectivity: { state: "online", checked_at: null },
  });
  for (const q of hub.queues) q.push(`event: settings.changed\ndata: ${data}\n\n`);
}

const overlay = (page: Page) => page.getByRole("dialog", { name: en["hub.lock.title"] });
const pinInput = (page: Page) =>
  overlay(page).getByRole("textbox", { name: en["hub.lock.pin_label"], exact: true });
const signOutButton = (page: Page) =>
  overlay(page).getByRole("button", { name: en["hub.header.sign_out"] });
const shell = (page: Page) => page.locator(".shell");
const header = (page: Page) => page.getByTestId("hub-staff");

async function open(
  page: Page,
  hub: Hub,
  path = "/floor",
  token: string | null = "counter-token",
): Promise<string[]> {
  await page.clock.install();
  await stub(page, hub, token);
  await page.goto(path);
  return [...hub.queues].at(-1)!;
}

async function typePin(page: Page, pin: string) {
  await page.keyboard.type(pin);
  if (pin.length < 6) await page.keyboard.press("Enter");
}

async function expectNoSeriousAxe(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  expect(bad.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual(
    [],
  );
}

test("five idle minutes lock the screen: the overlay names the staff member, the shell is inert", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub);
  await expect(header(page)).toContainText("Cody");
  // (The page's clock also moves in real time: the margins cover that.)
  await page.clock.runFor(5 * MIN - 10_000);
  await expect(overlay(page)).toHaveCount(0);
  await page.clock.runFor(10_000);
  await expect(overlay(page)).toBeVisible();
  await expect(overlay(page)).toHaveAttribute("aria-modal", "true");
  await expect(overlay(page).getByRole("heading", { level: 1 })).toHaveText(en["hub.lock.title"]);
  await expect(page.getByTestId("hub-lock-staff")).toHaveText(
    en["hub.lock.signed_in"].replace("{name}", "Cody"),
  );
  await expect(shell(page)).toHaveAttribute("inert", "");
  await expect(shell(page)).toHaveAttribute("aria-hidden", "true");
  await expect(pinInput(page)).toBeFocused();
  expect(hub.locks).toBe(1);
  // The Ctrl+ shortcuts do nothing behind the lock.
  await page.keyboard.press("Control+2");
  await expect(page).toHaveURL(/\/floor$/);
});

test("input at 4:59 resets the countdown", async ({ page }) => {
  const hub = newHub();
  await open(page, hub);
  await expect(header(page)).toBeVisible();
  // (The page's clock also moves in real time: the margins cover that.)
  await page.clock.runFor(4 * MIN + 50_000);
  await page.keyboard.press("Shift");
  await page.clock.runFor(4 * MIN + 50_000);
  await expect(overlay(page)).toHaveCount(0);
  await page.mouse.move(10, 10);
  await page.clock.runFor(4 * MIN + 50_000);
  await expect(overlay(page)).toHaveCount(0);
  await page.clock.runFor(15_000);
  await expect(overlay(page)).toBeVisible();
});

test("the sign-in screen never locks", async ({ page }) => {
  const hub = newHub();
  await open(page, hub, "/sign-in", null);
  await expect(page.getByLabel(en["hub.sign_in.login"], { exact: true })).toBeVisible();
  await page.clock.runFor(31 * MIN);
  await expect(overlay(page)).toHaveCount(0);
  expect(hub.locks).toBe(0);
});

test("the own PIN unlocks in place: same route, scroll position, unsaved text and focus", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 260 });
  const hub = newHub();
  await open(page, hub, "/settings/receipt");
  const field = page.getByLabel(en["hub.settings.receipt_footer"]);
  await expect(field).toHaveValue("");
  await field.fill("Thanks, see you soon");
  const main = page.locator("main.hub-main");
  // Part way down (not the very end, which shifts with any 1px reflow).
  await main.evaluate((el) => el.scrollTo(0, Math.floor((el.scrollHeight - el.clientHeight) / 2)));
  const scrolled = await main.evaluate((el) => el.scrollTop);
  expect(scrolled).toBeGreaterThan(0);
  await field.focus();
  await page.clock.runFor(5 * MIN);
  await expect(overlay(page)).toBeVisible();
  await expect(pinInput(page)).toBeFocused();
  await typePin(page, "3333");
  await expect(overlay(page)).toHaveCount(0);
  await expect(shell(page)).not.toHaveAttribute("inert", "");
  await expect(page).toHaveURL(/\/settings\/receipt$/);
  await expect(field).toHaveValue("Thanks, see you soon");
  await expect(field).toBeFocused();
  expect(await main.evaluate((el) => el.scrollTop)).toBe(scrolled);
  await expect(header(page)).toContainText("Cody");
  expect(hub.unlocks).toEqual(["3333"]);
  expect(hub.settings.get("receipt.footer")?.value).toBe("");
});

test("the manager's PIN switches the screen to the manager, and the next intent acts as them", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub, "/settings/branch");
  await expect(page.getByTestId("branch-needs-approval")).toBeVisible();
  await page.getByLabel(en["hub.settings.branch.currency"]).selectOption("USD");
  await page.clock.runFor(5 * MIN);
  await expect(overlay(page)).toBeVisible();
  await typePin(page, "2222");
  await expect(overlay(page)).toHaveCount(0);
  await expect(header(page)).toContainText("Mira · Manager");
  await expect(page.getByTestId("hub-announce")).toHaveText(
    en["hub.lock.switched"].replace("{name}", "Mira"),
  );
  // The unsaved choice survived; the manager saves it without a PIN.
  await expect(page.getByLabel(en["hub.settings.branch.currency"])).toHaveValue("USD");
  await expect(page.getByTestId("branch-needs-approval")).toHaveCount(0);
  await page.getByRole("button", { name: en["hub.settings.save"] }).click();
  await expect(page.getByTestId("branch-status")).toContainText(en["hub.settings.saved"]);
  expect(hub.intents).toEqual([{ actor: MANAGER.staff_id, key: "branch.currency", value: "USD" }]);
});

test("five wrong PINs pause unlocking with a countdown, and Sign out still works", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub);
  await expect(header(page)).toBeVisible();
  await page.clock.runFor(5 * MIN);
  await expect(overlay(page)).toBeVisible();
  for (let i = 0; i < 5; i++) {
    await pinInput(page).focus();
    await typePin(page, "9999");
    await expect.poll(() => hub.unlocks.length).toBe(i + 1);
    if (i < 4) {
      await expect(overlay(page)).toContainText(en["hub.lock.rejected"]);
      await expect(pinInput(page)).not.toHaveAttribute("readonly");
    }
  }
  const locked = (s: number) => en["ui.pin.locked"].replace("{seconds}", String(s));
  await expect(overlay(page)).toContainText(locked(60));
  await page.clock.runFor(1000);
  await expect(overlay(page)).toContainText(locked(59));
  // Typing does nothing while paused.
  await pinInput(page).focus();
  await page.keyboard.type("3333");
  await expect(overlay(page).locator(".cell.filled")).toHaveCount(0);
  expect(hub.unlocks).toHaveLength(5);
  await signOutButton(page).click();
  await expect(page).toHaveURL(/\/sign-in(\?|$)/);
  await expect(overlay(page)).toHaveCount(0);
});

test("the unlock pause belongs to the member: a new session of theirs is still paused", async ({
  page,
}) => {
  const hub = newHub();
  hub.failures.set(COUNTER.staff_id, 5);
  hub.sessions.set("counter-token-2", { staff: COUNTER, locked: true });
  await open(page, hub, "/floor", "counter-token-2");
  await expect(overlay(page)).toBeVisible();
  await typePin(page, "3333");
  await expect(overlay(page)).toContainText(en["ui.pin.locked"].replace("{seconds}", "60"));
  await expect(overlay(page)).toBeVisible();
});

test("keyboard only: digits, Backspace, Esc and Enter go to the PIN; Tab reaches Sign out", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub);
  await expect(header(page)).toBeVisible();
  await page.clock.runFor(5 * MIN);
  await expect(pinInput(page)).toBeFocused();
  const filled = overlay(page).locator(".cell.filled");
  await page.keyboard.type("12");
  await expect(filled).toHaveCount(2);
  await page.keyboard.press("Backspace");
  await expect(filled).toHaveCount(1);
  // Esc clears the PIN and does not unlock.
  await page.keyboard.press("Escape");
  await expect(filled).toHaveCount(0);
  await expect(overlay(page)).toBeVisible();
  expect(hub.unlocks).toEqual([]);
  // Tab moves through the keypad to Sign out, never behind the overlay.
  let reached = false;
  for (let i = 0; i < 20 && !reached; i++) {
    await page.keyboard.press("Tab");
    const inOverlay = await page.evaluate(
      () => !!document.activeElement?.closest('[role="dialog"]'),
    );
    expect(inOverlay).toBe(true);
    reached = await signOutButton(page).evaluate((el) => el === document.activeElement);
  }
  expect(reached).toBe(true);
  await pinInput(page).focus();
  await page.keyboard.type("3333");
  await page.keyboard.press("Enter");
  await expect(overlay(page)).toHaveCount(0);
  expect(hub.unlocks).toEqual(["3333"]);
});

test("a new idle setting applies from the next period", async ({ browser }) => {
  const hub = newHub();
  // The counter's stream reconnects until the event reaches it, then stays quiet.
  hub.retry = 300;
  const counter = await browser.newPage();
  const manager = await browser.newPage();
  const counterQueue = await open(counter, hub, "/floor", "counter-token");
  await expect(header(counter)).toBeVisible();
  await open(manager, hub, "/settings/screen-lock", "manager-token");
  // The manager sets 10 while the counter's window counts down 5.
  const field = manager.getByLabel(en["hub.settings.screen_lock.minutes"]);
  await expect(field).toHaveValue("5");
  await field.fill("10");
  await manager.getByRole("button", { name: en["hub.settings.save"] }).click();
  await expect(manager.getByTestId("screen-lock-status")).toContainText(en["hub.settings.saved"]);
  // The counter's window hears of it over its event stream (no reload).
  await expect.poll(() => counterQueue.length).toBe(0);
  hub.retry = 60_000;
  // Let the reopened stream's catch-up read finish before time jumps.
  await counter.waitForLoadState("networkidle");
  await counter.clock.runFor(5 * MIN);
  await expect(overlay(counter)).toBeVisible();
  await typePin(counter, "3333");
  await expect(overlay(counter)).toHaveCount(0);
  await counter.clock.runFor(10 * MIN - 10_000);
  await expect(overlay(counter)).toHaveCount(0);
  await counter.clock.runFor(10_000);
  await expect(overlay(counter)).toBeVisible();
  await counter.close();
  await manager.close();
});

test("Screen lock: an out-of-range idle lock is refused inline without a round trip", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub, "/settings/screen-lock", "manager-token");
  const sections = page.getByRole("navigation", { name: en["hub.settings.sections"] });
  await expect(
    sections.getByRole("link", { name: en["hub.settings.nav.screen_lock"] }),
  ).toHaveAttribute("aria-current", "page");
  await expect(page.getByTestId("screen-lock-range")).toHaveText(
    en["hub.settings.screen_lock.range"],
  );
  const field = page.getByLabel(en["hub.settings.screen_lock.minutes"]);
  await expect(field).toHaveValue("5");
  for (const bad of ["0", "31", "2.5"]) {
    await field.fill(bad);
    await page.getByRole("button", { name: en["hub.settings.save"] }).click();
    await expect(page.getByText(en["hub.settings.screen_lock.invalid"])).toBeVisible();
    await expect(field).toHaveAttribute("aria-invalid", "true");
    await expect(field).toBeFocused();
  }
  expect(hub.intents).toEqual([]);
  expect(hub.rejectedOnHub).toBe(0);
  await field.fill("12");
  await expect(field).not.toHaveAttribute("aria-invalid", "true");
  await page.getByRole("button", { name: en["hub.settings.save"] }).click();
  await expect(page.getByTestId("screen-lock-status")).toContainText(en["hub.settings.saved"]);
  expect(hub.settings.get("idle_lock_minutes")?.value).toBe("12");
  await expectNoSeriousAxe(page);
});

test("a pushed settings.changed ui block applies without a snapshot reload", async ({ page }) => {
  const hub = newHub();
  // Queued before the first stream opens (a first open never reloads the snapshot); the
  // snapshot itself keeps saying 5.
  const queue: string[] = [];
  const data = JSON.stringify({
    locale: snapshot(hub).locale,
    keys: ["idle_lock_minutes"],
    ui: { idle_lock_minutes: 1, hc: false },
    connectivity: { state: "online", checked_at: null },
  });
  queue.push(`event: settings.changed\ndata: ${data}\n\n`);
  await page.clock.install();
  await stub(page, hub, "counter-token");
  const mine = [...hub.queues].at(-1)!;
  mine.push(...queue);
  await page.goto("/floor");
  await expect(header(page)).toBeVisible();
  await expect.poll(() => mine.length).toBe(0);
  await page.waitForLoadState("networkidle");
  const snapshots = hub.snapshots;
  // Past the one-second input throttle, then input starts a new period.
  await page.clock.runFor(2_000);
  await page.keyboard.press("Shift");
  await page.clock.runFor(MIN + 5_000);
  await expect(overlay(page)).toBeVisible();
  expect(hub.snapshots).toBe(snapshots);
});

test("SESSION_LOCKED from the Hub locks the screen", async ({ page }) => {
  const hub = newHub();
  await open(page, hub, "/settings/screen-lock", "manager-token");
  const field = page.getByLabel(en["hub.settings.screen_lock.minutes"]);
  await expect(field).toHaveValue("5");
  // Another window sharing this session locked it.
  hub.sessions.get("manager-token")!.locked = true;
  await field.fill("7");
  await page.getByRole("button", { name: en["hub.settings.save"] }).click();
  await expect(overlay(page)).toBeVisible();
  await typePin(page, "2222");
  await expect(overlay(page)).toHaveCount(0);
  await expect(field).toHaveValue("7");
});

test("session.locked from another window locks this one too", async ({ page }) => {
  const hub = newHub();
  hub.retry = 300;
  const mine = await open(page, hub);
  await expect(header(page)).toBeVisible();
  hub.sessions.get("counter-token")!.locked = true;
  mine.push("event: session.locked\ndata: {}\n\n");
  await expect(overlay(page)).toBeVisible();
  expect(hub.locks).toBe(0);
});

test("a member without a PIN is told to sign out", async ({ page }) => {
  const hub = newHub();
  hub.noPin.add(COUNTER.staff_id);
  await open(page, hub);
  await expect(header(page)).toBeVisible();
  await page.clock.runFor(5 * MIN);
  await expect(page.getByTestId("hub-lock-no-pin")).toHaveText(en["hub.lock.no_pin"]);
  await expect(signOutButton(page)).toBeVisible();
});

test("a reload of a window that showed the lock shows it again", async ({ page }) => {
  const hub = newHub();
  await page.addInitScript(() => sessionStorage.setItem("arena.hub.locked", "1"));
  await open(page, hub);
  await expect(overlay(page)).toBeVisible();
  await expect.poll(() => hub.locks).toBe(1);
  await typePin(page, "3333");
  await expect(overlay(page)).toHaveCount(0);
  expect(await page.evaluate(() => sessionStorage.getItem("arena.hub.locked"))).toBeNull();
});

test("what changed while locked shows after unlocking", async ({ page }) => {
  const hub = newHub();
  await open(page, hub);
  const kiosk2 = page.locator('[data-device-id="01JDEVKIOSK0000000000000002"]');
  await expect(kiosk2).toHaveCount(1);
  await page.clock.runFor(5 * MIN);
  await expect(overlay(page)).toBeVisible();
  // The device is revoked while the screen is locked.
  hub.devices = hub.devices.filter((d) => d.device_id !== "01JDEVKIOSK0000000000000002");
  await typePin(page, "3333");
  await expect(overlay(page)).toHaveCount(0);
  await expect(kiosk2).toHaveCount(0);
  await expect(page.locator('[data-device-id="01JDEVKIOSK0000000000000001"]')).toHaveCount(1);
});

test("a reload while the Hub holds the session locked shows the lock again", async ({ page }) => {
  const hub = newHub();
  hub.sessions.get("counter-token")!.locked = true;
  await open(page, hub);
  await expect(overlay(page)).toBeVisible();
  await expect(pinInput(page)).toBeFocused();
  await typePin(page, "3333");
  await expect(overlay(page)).toHaveCount(0);
  expect(hub.sessions.get("counter-token")!.locked).toBe(false);
});

test("axe: the lock overlay", async ({ page }) => {
  const hub = newHub();
  await open(page, hub);
  await expect(header(page)).toBeVisible();
  await page.clock.runFor(5 * MIN);
  await expect(overlay(page)).toBeVisible();
  await expectNoSeriousAxe(page);
  await typePin(page, "9999");
  await expect(overlay(page)).toContainText(en["hub.lock.rejected"]);
  await expectNoSeriousAxe(page);
});
