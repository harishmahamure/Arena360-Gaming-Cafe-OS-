import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Locator, type Page, type Route } from "@playwright/test";
import type { AdConsentState, HubSnapshot, SessionStaff } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { ORIGIN } from "./origin";

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
    permissions: ["settings.edit"],
  },
  "counter-token": {
    staff_id: "01JCOUNTER0000000000000000",
    display_name: "Counter",
    role_name: "Counter",
    permissions: ["session.start", "pos.sell"],
  },
};
const AUTH_FAILED = { code: "AUTH_FAILED", message: "sign-in failed" };
const CLOSED_WEEK = JSON.stringify(Array.from({ length: 7 }, () => ({ closed: true })));
const NO_CONSENT = JSON.stringify({ given: false, at: null, staff_id: null });

type Sent = {
  kind: string;
  key?: string;
  value?: string;
  given?: boolean;
  policy_version?: number;
  base_rev?: number;
};
type Hub = {
  settings: Map<string, { value: string; rev: number }>;
  intents: Sent[];
  adConsent: AdConsentState;
  hc: boolean;
  refuse: { status: number; body: unknown } | null;
  queues: Set<string[]>;
};

function newHub(): Hub {
  return {
    settings: new Map([
      ["idle_lock_minutes", { value: "5", rev: 1 }],
      ["branch.opening_hours", { value: CLOSED_WEEK, rev: 1 }],
      ["branch.opening_hours_set", { value: "false", rev: 1 }],
      ["branch.fulfilment", { value: "counter", rev: 1 }],
      ["kiosk.volume_default", { value: "50", rev: 1 }],
      ["system.backup_dir", { value: "", rev: 1 }],
      ["branch.ads_consent", { value: NO_CONSENT, rev: 1 }],
    ]),
    intents: [],
    adConsent: "missing",
    hc: false,
    refuse: null,
    queues: new Set(),
  };
}

function snapshot(hub: Hub): HubSnapshot {
  return {
    branch: { name: "Dev Branch", branch_code: "DEV01" },
    groups: [],
    zones: [],
    devices: [],
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
    ui: { idle_lock_minutes: 5, hc: hub.hc },
    connectivity: { state: "online", checked_at: null },
    setup: {
      opening_hours_set: hub.settings.get("branch.opening_hours_set")!.value === "true",
      backup_location_set: hub.settings.get("system.backup_dir")!.value !== "",
      ad_consent: hub.adConsent,
      printer: "unset",
      products_count: 0,
    },
    lan: null,
    paired: true,
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

function changed(hub: Hub, keys: string[]) {
  const data = JSON.stringify({
    locale: snapshot(hub).locale,
    keys,
    ui: snapshot(hub).ui,
  });
  for (const q of hub.queues) q.push(`event: settings.changed\ndata: ${data}\n\n`);
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
        return json(route, 200, snapshot(hub));
      case "/ui/me":
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
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: "retry: 400\n\n" + queue.splice(0).join(""),
        });
      }
      case "/ui/intent": {
        if (!STAFF[auth]) return json(route, 401, AUTH_FAILED);
        const { intent } = req.postDataJSON() as {
          intent: {
            base_rev?: number;
            payload: { kind: string; key?: string; value?: string; given?: boolean };
          };
        };
        const p = intent.payload;
        hub.intents.push({ ...p, base_rev: intent.base_rev });
        if (hub.refuse) {
          const answer = hub.refuse;
          hub.refuse = null;
          return json(route, answer.status, answer.body);
        }
        if (!STAFF[auth].permissions.includes("settings.edit")) {
          return json(route, 403, { code: "PERMISSION_DENIED", message: "denied" });
        }
        const key = p.kind === "ads.consent" ? "branch.ads_consent" : p.key!;
        const row = hub.settings.get(key)!;
        if (intent.base_rev !== row.rev) {
          return json(route, 409, { code: "STALE", message: "stale", field: "base_rev" });
        }
        const keys = [key];
        if (p.kind === "ads.consent") {
          hub.settings.set(key, {
            value: JSON.stringify({
              given: p.given,
              at: Date.UTC(2027, 0, 15, 6, 30),
              staff_id: STAFF[auth].staff_id,
            }),
            rev: row.rev + 1,
          });
          hub.adConsent = p.given ? "given" : "missing";
        } else {
          hub.settings.set(key, { value: p.value!, rev: row.rev + 1 });
          if (key === "branch.opening_hours") {
            const flag = hub.settings.get("branch.opening_hours_set")!;
            if (flag.value !== "true") {
              hub.settings.set("branch.opening_hours_set", { value: "true", rev: flag.rev + 1 });
              keys.push("branch.opening_hours_set");
            }
          }
        }
        changed(hub, keys);
        return json(route, 200, { applied: { intent_id: "I", changes: [] } });
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
}

async function open(page: Page, hub: Hub, path: string, token = "manager-token") {
  await stub(page, hub, token);
  await page.goto(path);
  await expect(page).toHaveURL(new RegExp(path.replace(/\//g, "\\/") + "$"));
}

const save = (page: Page) => page.getByRole("button", { name: en["hub.settings.save"] });
const day = (page: Page, i: number) => page.getByTestId(`hours-day-${i}`);

async function expectNoSeriousAxe(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  expect(bad.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual(
    [],
  );
}

async function expectTarget(target: Locator) {
  const box = await target.boundingBox();
  expect(box!.height).toBeGreaterThanOrEqual(44);
}

async function setDay(page: Page, i: number, from: string, to: string) {
  await day(page, i).getByLabel(en["hub.settings.hours.closed"]).uncheck();
  await day(page, i).getByLabel(en["hub.settings.hours.opens"]).fill(from);
  await day(page, i).getByLabel(en["hub.settings.hours.closes"]).fill(to);
}

test("Hours lists the seven days closed, saves a week with a past-midnight day, and marks it set", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub, "/settings/branch/hours");
  const sub = page.getByRole("navigation", { name: en["hub.settings.branch.sections"] });
  await expect(sub.getByRole("link")).toHaveText([
    en["hub.settings.branch.nav.locale"],
    en["hub.settings.branch.nav.hours"],
    en["hub.settings.branch.nav.service"],
    en["hub.settings.branch.nav.tax"],
    en["hub.settings.branch.nav.ads"],
  ]);
  await expect(
    sub.getByRole("link", { name: en["hub.settings.branch.nav.hours"] }),
  ).toHaveAttribute("aria-current", "page");
  for (let i = 0; i < 7; i++) {
    await expect(day(page, i)).toContainText(en[`hub.settings.hours.day.${i}` as keyof typeof en]);
    await expect(day(page, i).getByLabel(en["hub.settings.hours.closed"])).toBeChecked();
  }
  await expect(page.getByTestId("hours-not-set")).toBeVisible();
  await expect(save(page)).toHaveAttribute("aria-disabled", "true");
  await expect(page.getByText(en["hub.settings.hours.no_open_day"])).toBeVisible();
  await setDay(page, 0, "10:00", "23:00");
  await setDay(page, 4, "18:00", "02:00");
  await expect(page.getByTestId("hours-next-day-4")).toHaveText(en["hub.settings.hours.next_day"]);
  await expect(page.getByTestId("hours-next-day-0")).toHaveCount(0);
  await expectTarget(day(page, 0).getByLabel(en["hub.settings.hours.opens"]));
  await expectNoSeriousAxe(page);
  await save(page).click();
  await expect(page.getByTestId("hours-status")).toContainText(en["hub.settings.saved"]);
  const closed = { closed: true };
  expect(hub.intents).toEqual([
    {
      kind: "settings.set",
      key: "branch.opening_hours",
      value: JSON.stringify([
        { open_min: 600, close_min: 1380 },
        closed,
        closed,
        closed,
        { open_min: 1080, close_min: 120 },
        closed,
        closed,
      ]),
      base_rev: 1,
    },
  ]);
  await expect(page.getByTestId("hours-not-set")).toHaveCount(0);
  await expect(day(page, 4).getByLabel(en["hub.settings.hours.opens"])).toHaveValue("18:00");
});

test("Hours shows field errors inline: bad times, equal times and the Hub's day", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub, "/settings/branch/hours");
  await setDay(page, 0, "10:00", "10:00");
  await setDay(page, 1, "25:00", "11:00");
  await save(page).click();
  await expect(page.getByTestId("hours-error-0-row")).toContainText(
    en["hub.settings.hours.invalid.same"],
  );
  await expect(page.getByTestId("hours-error-1-open")).toContainText(
    en["hub.settings.hours.invalid.time"],
  );
  await expect(day(page, 1).getByLabel(en["hub.settings.hours.opens"])).toHaveAttribute(
    "aria-invalid",
    "true",
  );
  expect(hub.intents).toEqual([]);
  await expectNoSeriousAxe(page);
  await day(page, 0).getByLabel(en["hub.settings.hours.closed"]).check();
  await day(page, 1).getByLabel(en["hub.settings.hours.opens"]).fill("09:00");
  await setDay(page, 2, "08:00", "12:00");
  hub.refuse = {
    status: 422,
    body: { code: "VALIDATION_FAILED", message: "bad day", field: "branch.opening_hours[2]" },
  };
  await save(page).click();
  await expect(page.getByTestId("hours-error-2-row")).toContainText(
    en["hub.settings.hours.invalid.day"],
  );
  await expect(page.getByTestId("hours-status")).toContainText(
    en["hub.settings.hours.invalid.week"],
  );
  await expect(page.getByTestId("hours-error-1-open")).toHaveCount(0);
});

test("Service saves fulfilment and the kiosk volume slider, which the keyboard moves", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub, "/settings/branch/service");
  await expect(page.getByLabel(en["hub.settings.service.fulfilment_counter"])).toBeChecked();
  const slider = page.getByLabel(en["hub.settings.service.volume"]);
  await expect(slider).toHaveValue("50");
  await expect(page.getByTestId("service-volume-value")).toHaveText("50");
  await expectTarget(slider);
  await expectTarget(
    page.locator("label").filter({ hasText: en["hub.settings.service.fulfilment_seat"] }),
  );
  await page.getByLabel(en["hub.settings.service.fulfilment_seat"]).check();
  await slider.focus();
  await page.keyboard.press("ArrowRight");
  await expect(page.getByTestId("service-volume-value")).toHaveText("51");
  await page.keyboard.press("End");
  await expect(page.getByTestId("service-volume-value")).toHaveText("100");
  await page.keyboard.press("ArrowLeft");
  await expect(slider).toHaveValue("99");
  await expectNoSeriousAxe(page);
  await save(page).click();
  await expect(page.getByTestId("service-status")).toContainText(en["hub.settings.saved"]);
  expect(hub.intents).toEqual([
    { kind: "settings.set", key: "branch.fulfilment", value: "seat", base_rev: 1 },
    { kind: "settings.set", key: "kiosk.volume_default", value: "99", base_rev: 1 },
  ]);
  await slider.focus();
  await page.keyboard.press("Home");
  hub.refuse = {
    status: 422,
    body: { code: "VALIDATION_FAILED", message: "bad", field: "kiosk.volume_default" },
  };
  await save(page).click();
  await expect(page.locator("#service-volume-error")).toContainText(
    en["hub.settings.service.invalid.volume"],
  );
});

test("System saves a backup location and shows the Hub's writable check inline", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub, "/settings/system");
  const sections = page.getByRole("navigation", { name: en["hub.settings.sections"] });
  await expect(sections.getByRole("link", { name: en["hub.settings.nav.system"] })).toHaveAttribute(
    "aria-current",
    "page",
  );
  await expect(page.getByTestId("system-backup-unset")).toBeVisible();
  await expect(page.getByRole("button", { name: en["hub.settings.system.browse"] })).toHaveCount(0);
  const field = page.getByLabel(en["hub.settings.system.backup_dir"]);
  await expectTarget(field);
  for (const [reason, key] of [
    ["not_found", "hub.settings.system.invalid.not_found"],
    ["not_dir", "hub.settings.system.invalid.not_dir"],
    ["not_writable", "hub.settings.system.invalid.not_writable"],
  ] as const) {
    await field.fill("E:\\Backups");
    hub.refuse = {
      status: 422,
      body: { code: "VALIDATION_FAILED", message: reason, field: "system.backup_dir" },
    };
    await save(page).click();
    await expect(page.getByTestId("system-backup-error")).toContainText(en[key]);
    await expect(field).toHaveAttribute("aria-invalid", "true");
    await expect(field).toBeFocused();
  }
  await expectNoSeriousAxe(page);
  await save(page).click();
  await expect(page.getByTestId("system-status")).toContainText(en["hub.settings.saved"]);
  await expect(page.getByTestId("system-backup-error")).toHaveText("");
  expect(hub.intents.at(-1)).toEqual({
    kind: "settings.set",
    key: "system.backup_dir",
    value: "E:\\Backups",
    base_rev: 1,
  });
  await expect(page.getByTestId("system-backup-unset")).toHaveCount(0);
});

test("Browse fills the backup location where the Tauri dialog is available", async ({ page }) => {
  await page.addInitScript(() => {
    (window as unknown as { __TAURI__: unknown }).__TAURI__ = {
      dialog: { open: async () => "/media/usb/backups" },
    };
  });
  const hub = newHub();
  await open(page, hub, "/settings/system");
  const browse = page.getByRole("button", { name: en["hub.settings.system.browse"] });
  await expectTarget(browse);
  await browse.click();
  await expect(page.getByLabel(en["hub.settings.system.backup_dir"])).toHaveValue(
    "/media/usb/backups",
  );
});

test("Ads records the consent tick with the time and the session's staff", async ({ page }) => {
  const hub = newHub();
  await open(page, hub, "/settings/branch/ads");
  await expect(page.getByTestId("ads-text")).toHaveText(en["hub.settings.ads.text"]);
  const policy = page.getByRole("link", { name: en["hub.settings.ads.policy"] });
  await expect(policy).toHaveAttribute("href", /^https:\/\//);
  await expectTarget(policy);
  await expect(page.getByTestId("ads-recorded")).toHaveText(en["hub.settings.ads.missing"]);
  const tick = page.getByLabel(en["hub.settings.ads.consent"]);
  await expect(tick).not.toBeChecked();
  await expect(save(page)).toHaveAttribute("aria-disabled", "true");
  await expectNoSeriousAxe(page);
  await tick.check();
  await save(page).click();
  await expect(page.getByTestId("ads-status")).toContainText(en["hub.settings.saved"]);
  expect(hub.intents).toEqual([
    { kind: "ads.consent", given: true, policy_version: 1, base_rev: 1 },
  ]);
  await expect(page.getByTestId("ads-recorded")).toContainText(
    en["hub.settings.ads.given"].split("{date}")[0],
  );
  await expect(tick).toBeChecked();
});

test("Ads is left out when the branch's plan needs no consent", async ({ page }) => {
  const hub = newHub();
  hub.adConsent = "not_needed";
  await open(page, hub, "/settings/branch/hours");
  const sub = page.getByRole("navigation", { name: en["hub.settings.branch.sections"] });
  await expect(sub.getByRole("link", { name: en["hub.settings.branch.nav.ads"] })).toHaveCount(0);
  await page.goto("/settings/branch/ads");
  await expect(page.getByTestId("ads-not-needed")).toHaveText(en["hub.settings.ads.not_needed"]);
  await expect(page.getByLabel(en["hub.settings.ads.consent"])).toHaveCount(0);
});

test("a Counter sees every operating section read-only", async ({ page }) => {
  const hub = newHub();
  await open(page, hub, "/settings/branch/hours", "counter-token");
  await expect(day(page, 0).getByLabel(en["hub.settings.hours.closed"])).toBeDisabled();
  await expect(save(page)).toHaveAttribute("aria-disabled", "true");
  await expect(page.getByText(en["hub.settings.branch.manager_only"])).toBeVisible();
  await page.goto("/settings/branch/service");
  await expect(page.getByLabel(en["hub.settings.service.volume"])).toBeDisabled();
  await expect(page.getByLabel(en["hub.settings.service.fulfilment_seat"])).toBeDisabled();
  await expect(page.getByText(en["hub.settings.branch.manager_only"])).toBeVisible();
  await page.goto("/settings/system");
  await expect(page.getByLabel(en["hub.settings.system.backup_dir"])).toBeDisabled();
  await expect(page.getByText(en["hub.settings.branch.manager_only"])).toBeVisible();
  await page.goto("/settings/branch/ads");
  await expect(page.getByLabel(en["hub.settings.ads.consent"])).toBeDisabled();
  await expect(save(page)).toHaveAttribute("aria-disabled", "true");
  await save(page).click({ force: true });
  expect(hub.intents).toEqual([]);
});

for (const hc of [false, true]) {
  test(`every operating section passes axe in the ${hc ? "high-contrast" : "dark"} theme`, async ({
    page,
  }) => {
    const hub = newHub();
    hub.hc = hc;
    await open(page, hub, "/settings/branch/hours");
    await expect
      .poll(() => page.evaluate(() => document.documentElement.dataset.theme))
      .toBe(hc ? "hc" : "dark");
    for (const path of [
      "/settings/branch/hours",
      "/settings/branch/service",
      "/settings/branch/ads",
      "/settings/system",
    ]) {
      await page.goto(path);
      await expect(page.locator("section h2")).toBeVisible();
      await expect(save(page)).toBeVisible();
      await expectNoSeriousAxe(page);
    }
  });
}

test("a stale Hours save keeps the typed times and says it changed elsewhere", async ({ page }) => {
  const hub = newHub();
  await open(page, hub, "/settings/branch/hours");
  await setDay(page, 0, "10:00", "23:00");
  hub.refuse = { status: 409, body: { code: "STALE", message: "stale", field: "base_rev" } };
  await save(page).click();
  await expect(page.getByTestId("hours-status")).toContainText(en["hub.settings.branch.stale"]);
  await expect(day(page, 0).getByLabel(en["hub.settings.hours.opens"])).toHaveValue("10:00");
  await expect(day(page, 0).getByLabel(en["hub.settings.hours.closes"])).toHaveValue("23:00");
});

test("a pushed settings.changed refreshes a clean Service form and keeps a dirty field", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub, "/settings/branch/service");
  const slider = page.getByLabel(en["hub.settings.service.volume"]);
  await expect(slider).toHaveValue("50");
  hub.settings.set("branch.fulfilment", { value: "seat", rev: 2 });
  changed(hub, ["branch.fulfilment"]);
  await expect(page.getByLabel(en["hub.settings.service.fulfilment_seat"])).toBeChecked();
  await slider.focus();
  await page.keyboard.press("ArrowRight");
  await expect(slider).toHaveValue("51");
  hub.settings.set("kiosk.volume_default", { value: "10", rev: 2 });
  hub.settings.set("branch.fulfilment", { value: "counter", rev: 3 });
  changed(hub, ["kiosk.volume_default", "branch.fulfilment"]);
  await expect(page.getByLabel(en["hub.settings.service.fulfilment_counter"])).toBeChecked();
  await expect(slider).toHaveValue("51");
  await save(page).click();
  await expect(page.getByTestId("service-status")).toContainText(en["hub.settings.saved"]);
  expect(hub.intents).toEqual([
    { kind: "settings.set", key: "kiosk.volume_default", value: "51", base_rev: 2 },
  ]);
});

test("a save the Hub refuses with PERMISSION_DENIED says only a manager can change it", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub, "/settings/branch/service");
  await page.getByLabel(en["hub.settings.service.fulfilment_seat"]).check();
  hub.refuse = { status: 403, body: { code: "PERMISSION_DENIED", message: "denied" } };
  await save(page).click();
  await expect(page.getByTestId("service-status")).toContainText(
    en["hub.settings.branch.manager_only"],
  );
  await expect(page.getByLabel(en["hub.settings.service.fulfilment_seat"])).toBeChecked();
});

test("Ads withdraws a given consent and records it", async ({ page }) => {
  const hub = newHub();
  hub.settings.set("branch.ads_consent", {
    value: JSON.stringify({
      given: true,
      at: Date.UTC(2027, 0, 14, 6, 30),
      staff_id: "01JMANAGER0000000000000000",
      policy_version: 1,
    }),
    rev: 2,
  });
  hub.adConsent = "given";
  await open(page, hub, "/settings/branch/ads");
  const tick = page.getByLabel(en["hub.settings.ads.consent"]);
  await expect(tick).toBeChecked();
  await expect(page.getByTestId("ads-recorded")).toContainText(
    en["hub.settings.ads.given"].split("{date}")[0],
  );
  await tick.uncheck();
  await save(page).click();
  await expect(page.getByTestId("ads-status")).toContainText(en["hub.settings.saved"]);
  expect(hub.intents).toEqual([
    { kind: "ads.consent", given: false, policy_version: 1, base_rev: 2 },
  ]);
  await expect(page.getByTestId("ads-recorded")).toContainText(
    en["hub.settings.ads.withdrawn"].split("{date}")[0],
  );
  await expect(tick).not.toBeChecked();
});
