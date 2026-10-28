import AxeBuilder from "@axe-core/playwright";
import { expect, test, type BrowserContext, type Page, type Route } from "@playwright/test";
import type { HubSnapshot, SessionStaff, SnapshotLocale } from "@arena/ui/generated";
import currencies from "../../../packages/ui/src/generated/currencies.json" with { type: "json" };
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { ORIGIN } from "./origin";

// Settings → Branch: the locale form, the live preview, save in sequence with base_rev, inline
// field errors, STALE, the deferred tz note, live updates through
// `/ui/events` across windows, and the header clock in the branch tz. (A Counter's save, which
// needs a manager's PIN, is in elevation.spec.ts.) The Hub's loopback API
// is stubbed with `page.route`; one stub state is shared by every window of a test.

const HUB = "http://127.0.0.1:7409";
const CORS = {
  "access-control-allow-origin": ORIGIN,
  "access-control-allow-methods": "GET, POST",
  "access-control-allow-headers": "authorization, content-type",
};
const STAFF: Record<string, SessionStaff> = {
  "manager-token": {
    staff_id: "01JMANAGER0000000000000000",
    display_name: "Manager",
    role_name: "Manager",
    permissions: ["settings.edit", "user.manage"],
  },
  "counter-token": {
    staff_id: "01JCOUNTER0000000000000000",
    display_name: "Counter",
    role_name: "Counter",
    permissions: ["session.start", "pos.sell"],
  },
};
const AUTH_FAILED = { code: "AUTH_FAILED", message: "sign-in failed" };
const BAD = { code: "VALIDATION_FAILED", message: "value is not valid", field: "value" };

type Row = { value: string; rev: number };
type Hub = {
  settings: Map<string, Row>;
  /** Deferred keys: their value before the change (the Hub reports the new one as pending). */
  prior: Map<string, string>;
  intents: { key: string; value: string; base_rev: number | undefined }[];
  /** One-shot answer replacing the next intent reply; `sideEffect` runs first. */
  nextIntent: { status: number; body: unknown; sideEffect?: () => void } | null;
  /** Events waiting for each window's stream. */
  queues: Set<string[]>;
  streams: number;
  snapshots: number;
  /** The reconnect delay each stream answer asks for, in ms. */
  retry: number;
  /** When set, `/ui/events` answers this status (and `/ui/me` 401 once it is 401). */
  streamStatus: number | null;
  /** A refusal for the first intent on this key. */
  failKey: { key: string; status: number; body: unknown; sideEffect?: () => void } | null;
};

function newHub(): Hub {
  return {
    settings: new Map([
      ["receipt.footer", { value: "", rev: 1 }],
      ["receipt.address", { value: "", rev: 1 }],
      ["receipt.phone", { value: "", rev: 1 }],
      ["receipt.default", { value: "print", rev: 1 }],
      ["printer.paper_mm", { value: "80", rev: 1 }],
      ["branch.currency", { value: "INR", rev: 1 }],
      ["branch.symbol_position", { value: "before", rev: 1 }],
      ["branch.number_format", { value: "indian", rev: 1 }],
      ["branch.tz", { value: "Asia/Kolkata", rev: 1 }],
      ["branch.day_start_min", { value: "360", rev: 1 }],
      ["branch.fy_start_month", { value: "4", rev: 1 }],
      ["idle_lock_minutes", { value: "5", rev: 1 }],
    ]),
    prior: new Map(),
    intents: [],
    nextIntent: null,
    queues: new Set(),
    streams: 0,
    snapshots: 0,
    retry: 400,
    streamStatus: null,
    failKey: null,
  };
}

const DEFERRED = ["branch.tz", "branch.day_start_min", "branch.fy_start_month"];

function locale(hub: Hub): SnapshotLocale {
  const v = (k: string) => hub.prior.get(k) ?? hub.settings.get(k)!.value;
  const later = (k: string) =>
    hub.prior.has(k)
      ? { value: hub.settings.get(k)!.value, effective_from: "2027-01-16" }
      : undefined;
  const tz = later("branch.tz");
  const ds = later("branch.day_start_min");
  const fy = later("branch.fy_start_month");
  const pending =
    tz || ds || fy
      ? {
          tz,
          day_start_min: ds && { ...ds, value: Number(ds.value) },
          fy_start_month: fy && { ...fy, value: Number(fy.value) },
        }
      : null;
  const c = currencies.find((x) => x.code === v("branch.currency"))!;
  return {
    currency: c.code,
    exponent: c.exponent,
    symbol: c.symbol,
    symbol_position: v("branch.symbol_position") as SnapshotLocale["symbol_position"],
    number_format: v("branch.number_format") as SnapshotLocale["number_format"],
    tz: v("branch.tz"),
    day_start_min: Number(v("branch.day_start_min")),
    fy_start_month: Number(v("branch.fy_start_month")),
    pending,
  };
}

function snapshot(hub: Hub): HubSnapshot {
  return {
    branch: { name: "Dev Branch", branch_code: "DEV01" },
    groups: [],
    zones: [],
    devices: [],
    locale: locale(hub),
    ui: { idle_lock_minutes: Number(hub.settings.get("idle_lock_minutes")?.value ?? 5), hc: false },
    connectivity: { state: "online", checked_at: null },
    lan: null,
    paired: true,
  };
}

/** Queues a raw event for every open window. */
function pushRaw(hub: Hub, text: string) {
  for (const q of hub.queues) q.push(text);
}

/** Queues `settings.changed` for every open window. */
function push(hub: Hub, keys: string[]) {
  const data = JSON.stringify({ locale: locale(hub), keys });
  for (const q of hub.queues) q.push(`event: settings.changed\ndata: ${data}\n\n`);
}

function json(route: Route, status: number, body: unknown) {
  return route.fulfill({
    status,
    contentType: "application/json",
    headers: CORS,
    body: JSON.stringify(body),
  });
}

async function stub(page: Page, hub: Hub, token: string) {
  await page.addInitScript((t) => sessionStorage.setItem("arena.hub.token", t), token);
  const queue: string[] = [];
  hub.queues.add(queue);
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    const url = new URL(req.url());
    const auth = (req.headers()["authorization"] ?? "").replace(/^Bearer /, "");
    switch (url.pathname) {
      case "/ui/snapshot":
        hub.snapshots++;
        return json(route, 200, snapshot(hub));
      case "/ui/me":
        if (hub.streamStatus === 401) return json(route, 401, AUTH_FAILED);
        return STAFF[auth] ? json(route, 200, STAFF[auth]) : json(route, 401, AUTH_FAILED);
      case "/ui/settings":
        if (!STAFF[auth]) return json(route, 401, AUTH_FAILED);
        return json(
          route,
          200,
          [...hub.settings].map(([key, row]) => ({ key, ...row })),
        );
      case "/ui/events": {
        const t = url.searchParams.get("token") ?? "";
        if (!STAFF[t]) return json(route, 401, AUTH_FAILED);
        if (hub.streamStatus !== null) return json(route, hub.streamStatus, AUTH_FAILED);
        hub.streams++;
        const body = `retry: ${hub.retry}\n\n` + queue.splice(0).join("");
        // The stub answers with what is queued and ends; the browser reconnects shortly.
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body,
        });
      }
      case "/ui/intent": {
        if (!STAFF[auth]) return json(route, 401, AUTH_FAILED);
        const { intent } = req.postDataJSON() as {
          intent: { base_rev?: number; payload: { key: string; value: string } };
        };
        const { key, value } = intent.payload;
        hub.intents.push({ key, value, base_rev: intent.base_rev });
        if (hub.nextIntent) {
          const answer = hub.nextIntent;
          hub.nextIntent = null;
          answer.sideEffect?.();
          return json(route, answer.status, answer.body);
        }
        if (hub.failKey?.key === key) {
          const answer = hub.failKey;
          hub.failKey = null;
          answer.sideEffect?.();
          return json(route, answer.status, answer.body);
        }
        if (!STAFF[auth].permissions.includes("settings.edit")) {
          return json(route, 403, { code: "PERMISSION_DENIED", message: "denied" });
        }
        const row = hub.settings.get(key)!;
        if (intent.base_rev !== row.rev) {
          return json(route, 409, { code: "STALE", message: "stale", field: "base_rev" });
        }
        if (DEFERRED.includes(key) && !hub.prior.has(key)) hub.prior.set(key, row.value);
        hub.settings.set(key, { value, rev: row.rev + 1 });
        push(hub, [key]);
        return json(route, 200, { applied: { intent_id: "I", changes: [] } });
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
}

async function openBranch(page: Page, hub: Hub, token = "manager-token") {
  await stub(page, hub, token);
  await page.goto("/settings");
  await expect(page).toHaveURL(/\/settings\/branch$/);
  await expect(page.getByLabel(en["hub.settings.branch.currency"])).toHaveValue("INR");
}

const preview = (page: Page) => page.getByTestId("branch-preview-amount");
const save = (page: Page) => page.getByRole("button", { name: en["hub.settings.save"] });
const status = (page: Page) => page.getByTestId("branch-status");

async function expectNoSeriousAxe(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  expect(bad.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual(
    [],
  );
}

test("Settings opens on Branch, first in its sections, with the current values", async ({
  page,
}) => {
  const hub = newHub();
  await openBranch(page, hub);
  await expect(page.locator("main h1")).toHaveText(en["hub.nav.settings"]);
  const sections = page.getByRole("navigation", { name: en["hub.settings.sections"] });
  await expect(sections.getByRole("link")).toHaveText([
    en["hub.settings.nav.branch"],
    en["hub.settings.nav.receipt"],
    en["hub.settings.nav.notifications"],
    en["hub.settings.nav.screen_lock"],
    en["hub.settings.nav.devices_display"],
    en["hub.settings.nav.devices"],
    en["hub.settings.nav.staff"],
    en["hub.settings.nav.layout"],
    en["hub.settings.nav.plans"],
    en["hub.settings.nav.rules"],
    en["hub.settings.nav.printer"],
    en["hub.settings.nav.kitchen"],
    en["hub.settings.nav.inventory"],
    en["hub.settings.nav.system"],
    en["hub.settings.nav.setup"],
  ]);
  await expect(sections.getByRole("link").first()).toHaveAttribute("aria-current", "page");
  await expect(page.getByLabel(en["hub.settings.branch.symbol_before"])).toBeChecked();
  await expect(page.getByLabel(en["hub.settings.branch.number_indian"])).toBeChecked();
  await expect(page.getByLabel(en["hub.settings.branch.tz"])).toHaveValue("Asia/Kolkata");
  await expect(page.getByLabel(en["hub.settings.branch.day_start"])).toHaveValue("06:00");
  await expect(page.getByLabel(en["hub.settings.branch.fy_start"])).toHaveValue("4");
  // Indian format: 12345678 minor units of INR.
  await expect(preview(page)).toHaveText("₹1,23,456.78");
  await expect(preview(page)).toHaveClass(/tabular-nums/);
  await expectNoSeriousAxe(page);
  // The Receipt section keeps the 1.11 control.
  await sections.getByRole("link", { name: en["hub.settings.nav.receipt"] }).click();
  await expect(page).toHaveURL(/\/settings\/receipt$/);
  await expect(page.getByTestId("settings-receipt-footer")).toBeVisible();
});

test("the preview follows the choices before saving", async ({ page }) => {
  const hub = newHub();
  await openBranch(page, hub);
  const currency = page.getByLabel(en["hub.settings.branch.currency"]);
  await currency.selectOption("USD");
  await page.getByLabel(en["hub.settings.branch.number_international"]).check();
  await expect(preview(page)).toHaveText("$123,456.78");
  await currency.selectOption("EUR");
  await page.getByLabel(en["hub.settings.branch.symbol_after"]).check();
  await expect(preview(page)).toHaveText("123,456.78 €");
  await page.getByLabel(en["hub.settings.branch.symbol_before"]).check();
  await currency.selectOption("JPY");
  // The sample is 123456.78 in major units, scaled to the currency's exponent.
  await expect(preview(page)).toHaveText("¥123,456");
  await currency.selectOption("KWD");
  await expect(preview(page)).toHaveText("KD123,456.780");
  // The currency options show the code and the symbol.
  await expect(currency.locator("option[value='INR']")).toHaveText("INR · ₹");
  expect(hub.intents).toEqual([]);
});

test("Save sends one settings.set per changed field, in order, each with its base_rev", async ({
  page,
}) => {
  const hub = newHub();
  hub.settings.set("branch.number_format", { value: "indian", rev: 4 });
  await openBranch(page, hub);
  await expect(save(page)).toHaveAttribute("aria-disabled", "true");
  await page.getByLabel(en["hub.settings.branch.currency"]).selectOption("USD");
  await page.getByLabel(en["hub.settings.branch.number_international"]).check();
  await page.getByLabel(en["hub.settings.branch.fy_start"]).selectOption("1");
  await save(page).click();
  await expect(status(page)).toContainText(en["hub.settings.saved"]);
  expect(hub.intents).toEqual([
    { key: "branch.currency", value: "USD", base_rev: 1 },
    { key: "branch.number_format", value: "international", base_rev: 4 },
    { key: "branch.fy_start_month", value: "1", base_rev: 1 },
  ]);
  // Nothing left to save; the next edit uses the Hub's new rev.
  await expect(save(page)).toHaveAttribute("aria-disabled", "true");
  await page.getByLabel(en["hub.settings.branch.currency"]).selectOption("GBP");
  await save(page).click();
  await expect(status(page)).toContainText(en["hub.settings.saved"]);
  expect(hub.intents.at(-1)).toEqual({ key: "branch.currency", value: "GBP", base_rev: 2 });
});

test("a refused currency shows the field error inline and nothing changes", async ({ page }) => {
  const hub = newHub();
  await openBranch(page, hub);
  await page.getByLabel(en["hub.settings.branch.currency"]).selectOption("USD");
  hub.nextIntent = { status: 422, body: BAD };
  await save(page).click();
  const field = page.getByLabel(en["hub.settings.branch.currency"]);
  await expect(field).toHaveAttribute("aria-invalid", "true");
  await expect(page.locator("#branch-currency-error")).toContainText(
    en["hub.settings.branch.invalid.currency"],
  );
  await expect(field).toBeFocused();
  // The typed choice is kept; the Hub's value is unchanged.
  await expect(field).toHaveValue("USD");
  expect(hub.settings.get("branch.currency")).toEqual({ value: "INR", rev: 1 });
});

test("a refused time zone shows the field error inline", async ({ page }) => {
  const hub = newHub();
  await openBranch(page, hub);
  const tz = page.getByLabel(en["hub.settings.branch.tz"]);
  await tz.selectOption("Asia/Dubai");
  hub.nextIntent = { status: 422, body: BAD };
  await save(page).click();
  await expect(tz).toHaveAttribute("aria-invalid", "true");
  await expect(page.locator("#branch-tz-error")).toContainText(
    en["hub.settings.branch.invalid.tz"],
  );
  expect(hub.intents).toEqual([{ key: "branch.tz", value: "Asia/Dubai", base_rev: 1 }]);
});

test("STALE shows the current value and the retry message", async ({ page }) => {
  const hub = newHub();
  await openBranch(page, hub);
  await page.getByLabel(en["hub.settings.branch.currency"]).selectOption("USD");
  // Another window saved EUR in the meantime.
  hub.nextIntent = {
    status: 409,
    body: { code: "STALE", message: "stale", field: "base_rev" },
    sideEffect: () => hub.settings.set("branch.currency", { value: "EUR", rev: 2 }),
  };
  await save(page).click();
  await expect(status(page)).toContainText(en["hub.settings.branch.stale"]);
  // The typed choice is kept, the field is marked and the Hub's value is shown.
  const field = page.getByLabel(en["hub.settings.branch.currency"]);
  await expect(field).toHaveValue("USD");
  await expect(field).toHaveAttribute("aria-invalid", "true");
  await expect(page.locator("#branch-currency-error")).toContainText(
    en["hub.settings.branch.stale"],
  );
  await expect(page.getByTestId("branch-currency-hub-value")).toHaveText(
    en["hub.settings.server_value"].replace("{value}", "EUR"),
  );
  // Retrying sends the Hub's rev.
  await save(page).click();
  await expect(status(page)).toContainText(en["hub.settings.saved"]);
  expect(hub.intents.at(-1)).toEqual({ key: "branch.currency", value: "USD", base_rev: 2 });
});

test("a tz change says when it takes effect, in the branch's own date", async ({ page }) => {
  const hub = newHub();
  await openBranch(page, hub);
  await expect(page.getByTestId("branch-tz-pending")).toHaveCount(0);
  await page.getByLabel(en["hub.settings.branch.tz"]).selectOption("Asia/Dubai");
  await save(page).click();
  await expect(status(page)).toContainText(en["hub.settings.saved"]);
  const date = await page.evaluate(() =>
    new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeZone: "UTC" }).format(
      Date.UTC(2027, 0, 16, 12),
    ),
  );
  await expect(page.getByTestId("branch-tz-pending")).toHaveText(
    en["hub.settings.branch.takes_effect"].replace("{date}", date),
  );
  await expect(page.getByLabel(en["hub.settings.branch.tz"])).toHaveAttribute(
    "aria-describedby",
    /branch-tz-pending/,
  );
});

test("a save in one window updates another without a reload", async ({ browser }) => {
  const hub = newHub();
  const context: BrowserContext = await browser.newContext();
  try {
    const a = await context.newPage();
    const b = await context.newPage();
    await openBranch(a, hub);
    await openBranch(b, hub);
    // B listens before A saves.
    await expect.poll(() => hub.streams).toBeGreaterThanOrEqual(2);
    let reloads = 0;
    b.on("framenavigated", () => reloads++);
    await a.getByLabel(en["hub.settings.branch.currency"]).selectOption("USD");
    await a.getByLabel(en["hub.settings.branch.number_international"]).check();
    await a.getByRole("button", { name: en["hub.settings.save"] }).click();
    await expect(a.getByTestId("branch-status")).toContainText(en["hub.settings.saved"]);
    // B: the form (no unsaved edits) and the preview follow, with no navigation.
    await expect(b.getByLabel(en["hub.settings.branch.currency"])).toHaveValue("USD");
    await expect(b.getByTestId("branch-preview-amount")).toHaveText("$123,456.78");
    expect(reloads).toBe(0);
  } finally {
    await context.close();
  }
});

test("another window's save refreshes untouched fields and keeps edits", async ({ browser }) => {
  const hub = newHub();
  const context = await browser.newContext();
  try {
    const a = await context.newPage();
    const b = await context.newPage();
    await openBranch(a, hub);
    await openBranch(b, hub);
    await b.getByLabel(en["hub.settings.branch.fy_start"]).selectOption("7");
    await a.getByLabel(en["hub.settings.branch.currency"]).selectOption("GBP");
    await a.getByRole("button", { name: en["hub.settings.save"] }).click();
    await expect(a.getByTestId("branch-status")).toContainText(en["hub.settings.saved"]);
    // Both windows have taken the event.
    await expect.poll(() => [...hub.queues].every((q) => q.length === 0)).toBe(true);
    // B refreshes the field it had not touched and keeps its own edit.
    await expect(b.getByLabel(en["hub.settings.branch.currency"])).toHaveValue("GBP");
    await expect(b.getByLabel(en["hub.settings.branch.fy_start"])).toHaveValue("7");
  } finally {
    await context.close();
  }
});

test("the header clock shows the branch time zone", async ({ page }, info) => {
  test.skip(info.project.name === "phone", "the phone header has no clock");
  // 2027-01-15 08:30 UTC: 14:00 in Kolkata, 12:30 in Dubai.
  await page.clock.setFixedTime(new Date(Date.UTC(2027, 0, 15, 8, 30)));
  const hub = newHub();
  await openBranch(page, hub);
  const expected = (tz: string) =>
    page.evaluate(
      (zone) =>
        new Intl.DateTimeFormat(undefined, {
          hour: "numeric",
          minute: "2-digit",
          timeZone: zone,
        }).format(Date.UTC(2027, 0, 15, 8, 30)),
      tz,
    );
  await expect(page.locator("header time")).toHaveText(await expected("Asia/Kolkata"));
  await page.unroute(`${HUB}/**`);
  hub.settings.set("branch.tz", { value: "Asia/Dubai", rev: 2 });
  await stub(page, hub, "manager-token");
  await page.reload();
  await expect(page.locator("header time")).toHaveText(await expected("Asia/Dubai"));
});

const businessDate = (page: Page) =>
  page.evaluate(() =>
    new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeZone: "UTC" }).format(
      Date.UTC(2027, 0, 16, 12),
    ),
  );

test("a day start change says when it takes effect", async ({ page }) => {
  const hub = newHub();
  await openBranch(page, hub);
  await expect(page.getByTestId("branch-day-start-pending")).toHaveCount(0);
  await page.getByLabel(en["hub.settings.branch.day_start"]).fill("05:00");
  await save(page).click();
  await expect(status(page)).toContainText(en["hub.settings.saved"]);
  expect(hub.intents).toEqual([{ key: "branch.day_start_min", value: "300", base_rev: 1 }]);
  await expect(page.getByTestId("branch-day-start-pending")).toHaveText(
    en["hub.settings.branch.takes_effect"].replace("{date}", await businessDate(page)),
  );
  await expect(page.getByTestId("branch-tz-pending")).toHaveCount(0);
});

test("a partial save lists what was saved and what was not", async ({ page }) => {
  const hub = newHub();
  await openBranch(page, hub);
  await page.getByLabel(en["hub.settings.branch.currency"]).selectOption("USD");
  await page.getByLabel(en["hub.settings.branch.fy_start"]).selectOption("1");
  hub.failKey = { key: "branch.fy_start_month", status: 422, body: BAD };
  await save(page).click();
  await expect(status(page)).toContainText(
    en["hub.settings.branch.partial"]
      .replace("{saved}", en["hub.settings.branch.currency"])
      .replace("{failed}", en["hub.settings.branch.fy_start"]),
  );
  await expect(page.getByLabel(en["hub.settings.branch.fy_start"])).toHaveValue("1");
  expect(hub.settings.get("branch.currency")?.value).toBe("USD");
});

test("a dropped stream that reopens refetches the snapshot", async ({ page }) => {
  const hub = newHub();
  hub.retry = 150;
  await openBranch(page, hub);
  await expect.poll(() => hub.streams).toBeGreaterThanOrEqual(1);
  const before = hub.snapshots;
  // The stub ends every stream; the browser reopens it and the view catches up.
  await expect.poll(() => hub.snapshots, { timeout: 5000 }).toBeGreaterThan(before);
  expect(hub.streams).toBeGreaterThanOrEqual(2);
});

test("event: resync refetches the snapshot", async ({ page }) => {
  const hub = newHub();
  // No reopen within the test: only the resync can cause a second snapshot read.
  hub.retry = 60_000;
  await stub(page, hub, "manager-token");
  pushRaw(hub, "event: resync\ndata: {}\n\n");
  await page.goto("/settings/branch");
  await expect(page.getByLabel(en["hub.settings.branch.currency"])).toHaveValue("INR");
  await expect.poll(() => hub.streams).toBe(1);
  await expect.poll(() => hub.snapshots).toBeGreaterThanOrEqual(2);
  expect(hub.streams).toBe(1);
});

test("a refused stream (401) goes to sign-in without any action", async ({ page }) => {
  const hub = newHub();
  hub.retry = 150;
  await openBranch(page, hub);
  // The session is revoked: the stream and /ui/me now answer 401.
  hub.streamStatus = 401;
  await expect(page).toHaveURL(/\/sign-in(\?|$)/, { timeout: 5000 });
});

test("the header clock follows a live tz change", async ({ page }, info) => {
  test.skip(info.project.name === "phone", "the phone header has no clock");
  await page.clock.setFixedTime(new Date(Date.UTC(2027, 0, 15, 8, 30)));
  const hub = newHub();
  hub.retry = 150;
  await openBranch(page, hub);
  const expected = (tz: string) =>
    page.evaluate(
      (zone) =>
        new Intl.DateTimeFormat(undefined, {
          hour: "numeric",
          minute: "2-digit",
          timeZone: zone,
        }).format(Date.UTC(2027, 0, 15, 8, 30)),
      tz,
    );
  await expect(page.locator("header time")).toHaveText(await expected("Asia/Kolkata"));
  let navigations = 0;
  page.on("framenavigated", () => navigations++);
  // The deferred change came due: the Hub pushes the resolved locale.
  hub.settings.set("branch.tz", { value: "Asia/Dubai", rev: 2 });
  push(hub, ["branch.tz"]);
  await expect(page.locator("header time")).toHaveText(await expected("Asia/Dubai"));
  expect(navigations).toBe(0);
});
