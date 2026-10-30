import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type { BackupExport, HubSnapshot, SessionStaff } from "@arena/ui/generated";
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
    permissions: ["settings.edit", "backup.export"],
  },
  "counter-token": {
    staff_id: "01JCOUNTER0000000000000000",
    display_name: "Counter",
    role_name: "Counter",
    permissions: ["session.start", "pos.sell"],
  },
};
const AUTH_FAILED = { code: "AUTH_FAILED", message: "sign-in failed" };
// 2027-01-15 08:00 UTC is 13:30 in the branch's Asia/Kolkata.
const AT = 1_800_000_000_000;
const EXPORTED: BackupExport = {
  file_name: "DEV01-2027-01-15-1330.a360",
  bytes: 3 * 1024 * 1024,
  epoch: "1:abc",
  last_seq: 41,
  exported_at: AT,
};

type Answer = { status: number; body: unknown };
type Hub = {
  settings: Map<string, { value: string; rev: number }>;
  /** What the next exports answer, one per call; the last repeats. */
  answers: Answer[];
  exports: { ticket?: string }[];
  elevations: number;
};

function newHub(): Hub {
  return {
    settings: new Map([
      ["idle_lock_minutes", { value: "5", rev: 1 }],
      ["system.backup_dir", { value: "D:\\Backups", rev: 1 }],
      ["system.media_cap_gb", { value: "10", rev: 1 }],
      ["backup.last_export", { value: "", rev: 1 }],
    ]),
    answers: [{ status: 200, body: EXPORTED }],
    exports: [],
    elevations: 0,
  };
}

const invalidFolder = (message: string): Answer => ({
  status: 422,
  body: { code: "VALIDATION_FAILED", message, field: "folder" },
});

function snapshot(): HubSnapshot {
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
    ui: { idle_lock_minutes: 5, hc: false },
    connectivity: { state: "online", checked_at: null },
    setup: {
      opening_hours_set: false,
      backup_location_set: true,
      ad_consent: "missing",
      ad_rating: "PG",
      printer: "unset",
      products_count: 0,
      groups_without_rate: 0,
      manager_exists: false,
      test_session: "unset",
      live: false,
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

async function open(page: Page, hub: Hub, token = "manager-token") {
  await page.addInitScript((t) => sessionStorage.setItem("arena.hub.token", t), token);
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    const url = new URL(req.url());
    const auth = (req.headers()["authorization"] ?? "").replace(/^Bearer /, "");
    switch (url.pathname) {
      case "/ui/snapshot":
        return json(route, 200, snapshot());
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
          body: "retry: 400\n\n",
        });
      }
      case "/ui/elevate": {
        if (!STAFF[auth]) return json(route, 401, AUTH_FAILED);
        const body = req.postDataJSON() as { items: { intent_kind: string }[]; pin: string };
        if (body.pin !== "2222") {
          return json(route, 403, { code: "PIN_REJECTED", message: "that PIN can't approve this" });
        }
        expect(body.items).toEqual([{ intent_kind: "backup.export" }]);
        hub.elevations++;
        return json(route, 200, { ticket: "T1", items: body.items, expires_in_s: 60 });
      }
      case "/ui/backup/export": {
        if (!STAFF[auth]) return json(route, 401, AUTH_FAILED);
        const sent = (req.postDataJSON() ?? {}) as { elevation_ticket?: string };
        hub.exports.push(sent.elevation_ticket ? { ticket: sent.elevation_ticket } : {});
        if (!sent.elevation_ticket && !STAFF[auth].permissions.includes("backup.export")) {
          return json(route, 403, { code: "PERMISSION_DENIED", message: "denied" });
        }
        const answer = hub.answers.length > 1 ? hub.answers.shift()! : hub.answers[0];
        if (answer.status === 200) {
          hub.settings.set("backup.last_export", {
            value: JSON.stringify(answer.body),
            rev: 2,
          });
        }
        return json(route, answer.status, answer.body);
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/settings/system");
  await expect(page.getByTestId("settings-backup")).toBeVisible();
}

const exportNow = (page: Page) => page.getByRole("button", { name: en["hub.backup.export_now"] });

async function expectNoSeriousAxe(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  expect(bad.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual(
    [],
  );
}

test("a manager exports in one tap and the panel shows the file's name, size and time", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub);
  await expect(page.getByTestId("backup-never")).toHaveText(en["hub.backup.never"]);
  await expectNoSeriousAxe(page);
  await exportNow(page).click();
  await expect(page.getByTestId("backup-saved")).toHaveText(
    en["hub.backup.saved"].replace("{name}", EXPORTED.file_name),
  );
  await expect(page.getByTestId("backup-last-value")).toHaveText(
    `${EXPORTED.file_name} · ${en["hub.settings.system.media.size_mb"].replace("{n}", "3")} · 15 Jan 2027, 13:30`,
  );
  expect(hub.exports).toEqual([{}]);
  expect(hub.elevations).toBe(0);
  await expect(page.locator('[data-component="inline-pin"]')).toHaveCount(0);
  await expectNoSeriousAxe(page);
});

test("a Counter's tap opens the PIN in place and the export carries the manager's ticket", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub, "counter-token");
  await exportNow(page).click();
  const approval = page.getByTestId("backup-approval");
  await expect(approval.locator('[data-component="inline-pin"]')).toBeVisible();
  await expect(approval).toContainText(en["hub.backup.pin_prompt"]);
  expect(hub.exports).toEqual([]);
  await approval.getByLabel(en["ui.pin.label"]).focus();
  await page.keyboard.type("9999");
  await page.keyboard.press("Enter");
  await expect(approval).toContainText(en["ui.pin.rejected"]);
  expect(hub.exports).toEqual([]);
  await page.keyboard.type("2222");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("backup-saved")).toBeVisible();
  expect(hub.elevations).toBe(1);
  expect(hub.exports).toEqual([{ ticket: "T1" }]);
  await expect(approval).toHaveCount(0);
});

test("with no location the panel names the cause and Choose location brings the field forward", async ({
  page,
}) => {
  const hub = newHub();
  hub.settings.set("system.backup_dir", { value: "", rev: 1 });
  hub.answers = [invalidFolder("not_set")];
  await open(page, hub);
  await exportNow(page).click();
  const problem = page.getByTestId("backup-problem");
  await expect(problem).toContainText(en["hub.backup.problem.not_set"]);
  await expect(problem.getByRole("button", { name: en["hub.backup.try_again"] })).toHaveCount(0);
  await expectNoSeriousAxe(page);
  await problem.getByRole("button", { name: en["hub.backup.choose_location"] }).click();
  await expect(page.locator("#system-backup-dir")).toBeFocused();
  await expect(page.getByTestId("backup-saved")).toHaveCount(0);
});

test("a removed drive shows the cause with Try again, which exports once it is back", async ({
  page,
}) => {
  const hub = newHub();
  hub.answers = [invalidFolder("not_found"), { status: 200, body: EXPORTED }];
  await open(page, hub);
  await exportNow(page).click();
  const problem = page.getByTestId("backup-problem");
  await expect(problem).toContainText(en["hub.backup.problem.not_found"]);
  await expect(
    problem.getByRole("button", { name: en["hub.backup.choose_location"] }),
  ).toBeVisible();
  await expectNoSeriousAxe(page);
  await problem.getByRole("button", { name: en["hub.backup.try_again"] }).click();
  await expect(page.getByTestId("backup-saved")).toBeVisible();
  await expect(problem).toHaveCount(0);
  expect(hub.exports).toEqual([{}, {}]);
});

test("a failed export says nothing was saved and offers Try again", async ({ page }) => {
  const hub = newHub();
  hub.answers = [
    { status: 500, body: { code: "INTERNAL", message: "the hub could not complete this request" } },
  ];
  await open(page, hub);
  await exportNow(page).click();
  const notice = page.getByTestId("backup-notice");
  await expect(notice).toContainText(en["hub.backup.failed"]);
  await expect(notice.getByRole("button", { name: en["hub.backup.try_again"] })).toBeVisible();
  await expect(page.getByTestId("backup-last-value")).toHaveCount(0);
});
