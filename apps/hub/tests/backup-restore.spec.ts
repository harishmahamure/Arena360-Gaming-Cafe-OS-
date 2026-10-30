import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type { HubSnapshot, RestoreCheck, SessionStaff } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { ORIGIN } from "./origin";

const HUB = "http://127.0.0.1:7409";
const CORS = {
  "access-control-allow-origin": ORIGIN,
  "access-control-allow-methods": "GET, POST",
  "access-control-allow-headers": "authorization, content-type",
};
const STAFF: Record<string, SessionStaff> = {
  "owner-token": {
    staff_id: "01JOWNER00000000000000000",
    display_name: "Owner",
    role_name: "Owner",
    permissions: ["settings.edit", "backup.export", "ops.restore"],
  },
  "manager-token": {
    staff_id: "01JMANAGER0000000000000000",
    display_name: "Manager",
    role_name: "Manager",
    permissions: ["settings.edit", "backup.export"],
  },
};
const AUTH_FAILED = { code: "AUTH_FAILED", message: "sign-in failed" };
const OWNER_PIN = "1111";
const FILE = "D:/Backups/DEV01-2027-01-15-1325.a360";
// 2027-01-15 07:59 UTC is 13:29 in the branch's Asia/Kolkata.
const CHECK: RestoreCheck = {
  branch_name: "Dev Branch",
  branch_code: "DEV01",
  file_name: "DEV01-2027-01-15-1325.a360",
  bytes: 3 * 1024 * 1024,
  exported_at: 1_800_000_000_000 - 60_000,
  epoch: "3:01JHUB",
  last_seq: 120,
  schema_version: 84,
  rows_set_aside: 31,
  warning:
    "Everything after 2027-01-15 13:29 on this PC will be set aside. Kiosks reconnect by themselves.",
  erased_may_reappear: false,
};

type Answer = { status: number; body: unknown };
type Hub = {
  checkAnswer: Answer;
  restoreAnswer: Answer;
  checks: { path: string; pin?: string }[];
  restores: { path: string; pin: string; branch_code: string }[];
};

function newHub(): Hub {
  return {
    checkAnswer: { status: 200, body: CHECK },
    restoreAnswer: { status: 200, body: { restarting: true } },
    checks: [],
    restores: [],
  };
}

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
      backup_export_failed: false,
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

const rejected = { code: "PIN_REJECTED", message: "that PIN can't approve this" };

async function open(page: Page, hub: Hub, token = "owner-token") {
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
        return json(route, 200, [
          { key: "idle_lock_minutes", value: "5", rev: 1 },
          { key: "system.backup_dir", value: "D:\\Backups", rev: 1 },
          { key: "system.media_cap_gb", value: "10", rev: 1 },
          { key: "backup.last_export", value: "", rev: 1 },
          { key: "backup.last_restore", value: "", rev: 1 },
        ]);
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
      case "/ui/backup/restore/check": {
        if (!STAFF[auth]) return json(route, 401, AUTH_FAILED);
        const sent = req.postDataJSON() as { path: string; pin?: string };
        hub.checks.push(sent);
        const owner = STAFF[auth].permissions.includes("ops.restore");
        if (!owner && sent.pin === undefined) {
          return json(route, 403, { code: "PERMISSION_DENIED", message: "denied" });
        }
        if (!owner && sent.pin !== OWNER_PIN) return json(route, 403, rejected);
        return json(route, hub.checkAnswer.status, hub.checkAnswer.body);
      }
      case "/ui/backup/restore": {
        if (!STAFF[auth]) return json(route, 401, AUTH_FAILED);
        const sent = req.postDataJSON() as { path: string; pin: string; branch_code: string };
        hub.restores.push(sent);
        if (sent.pin !== OWNER_PIN) return json(route, 403, rejected);
        if (sent.branch_code !== "DEV01") {
          return json(route, 422, {
            code: "VALIDATION_FAILED",
            message: "the branch code does not match",
            field: "branch_code",
          });
        }
        return json(route, hub.restoreAnswer.status, hub.restoreAnswer.body);
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/settings/system");
  await expect(page.getByTestId("settings-restore")).toBeVisible();
}

async function expectNoSeriousAxe(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  expect(bad.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual(
    [],
  );
}

const checkFile = (page: Page) =>
  page.getByTestId("settings-restore").getByRole("button", { name: en["hub.restore.check"] });

async function typePin(page: Page, scope: ReturnType<Page["getByTestId"]>, pin: string) {
  await scope.getByLabel(en["ui.pin.label"]).focus();
  await page.keyboard.type(pin);
  await page.keyboard.press("Enter");
}

test("the Owner checks a file, sees what will be set aside, types the code and confirms with the PIN", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub);
  await expectNoSeriousAxe(page);
  await expect(checkFile(page)).toBeDisabled();
  await page.getByTestId("restore-path").fill(FILE);
  await checkFile(page).click();

  // No PIN to look at the file: the Owner is signed in.
  const summary = page.getByTestId("restore-summary");
  await expect(summary).toContainText("Dev Branch (DEV01)");
  await expect(summary).toContainText("15 Jan 2027, 13:29");
  await expect(summary).toContainText(
    en["hub.restore.summary_epoch"].replace("{epoch}", "3:01JHUB").replace("{seq}", "120"),
  );
  await expect(page.getByTestId("restore-set-aside")).toHaveText(
    en["hub.restore.summary_set_aside"].replace("{n}", "31"),
  );
  await expect(page.getByTestId("restore-warning")).toContainText(CHECK.warning);
  expect(hub.checks).toEqual([{ path: FILE }]);
  await expectNoSeriousAxe(page);

  // The code must be typed before Restore does anything.
  const restore = page.getByRole("button", { name: en["hub.restore.restore"] });
  await expect(restore).toBeDisabled();
  await page.getByTestId("restore-code").fill("DEV02");
  await restore.click();
  const approval = page.getByTestId("restore-approval");
  await expect(approval).toContainText(en["hub.restore.confirm_pin_prompt"]);
  await typePin(page, approval, OWNER_PIN);
  // A wrong code is named and the summary stays so it can be typed again.
  await expect(page.getByTestId("restore-problem")).toContainText(
    en["hub.restore.problem.branch_code"],
  );
  await expect(summary).toBeVisible();
  expect(hub.restores).toEqual([{ path: FILE, pin: OWNER_PIN, branch_code: "DEV02" }]);

  await page.getByTestId("restore-code").fill("DEV01");
  await restore.click();
  await typePin(page, page.getByTestId("restore-approval"), "9999");
  await expect(page.getByTestId("restore-approval")).toContainText(en["ui.pin.rejected"]);
  await typePin(page, page.getByTestId("restore-approval"), OWNER_PIN);
  await expect(page.getByTestId("restore-restarting")).toHaveText(en["hub.restore.restarting"]);
  expect(hub.restores.at(-1)).toEqual({ path: FILE, pin: OWNER_PIN, branch_code: "DEV01" });
  await expect(page.getByTestId("restore-summary")).toHaveCount(0);
  await expectNoSeriousAxe(page);
});

test("a Manager is asked for the Owner's PIN in place before the file is checked", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub, "manager-token");
  await page.getByTestId("restore-path").fill(FILE);
  await checkFile(page).click();
  const approval = page.getByTestId("restore-approval");
  await expect(approval.locator('[data-component="inline-pin"]')).toBeVisible();
  await expect(approval).toContainText(en["hub.restore.owner_pin_prompt"]);
  expect(hub.checks).toEqual([]);
  await expectNoSeriousAxe(page);

  await typePin(page, approval, "2222");
  await expect(approval).toContainText(en["ui.pin.rejected"]);
  await typePin(page, approval, OWNER_PIN);
  await expect(page.getByTestId("restore-summary")).toBeVisible();
  await expect(approval).toHaveCount(0);
  expect(hub.checks).toEqual([
    { path: FILE, pin: "2222" },
    { path: FILE, pin: OWNER_PIN },
  ]);
});

test("a damaged file, another branch's file and a newer one are named and nothing goes further", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub);
  const cases: [string, string][] = [
    ["RESTORE_CORRUPT", en["hub.restore.problem.corrupt"]],
    ["RESTORE_WRONG_BRANCH", en["hub.restore.problem.wrong_branch"]],
    ["SCHEMA_TOO_NEW", en["hub.restore.problem.too_new"]],
  ];
  for (const [code, message] of cases) {
    hub.checkAnswer = { status: 422, body: { code, message: "refused" } };
    await page.getByTestId("restore-path").fill(FILE);
    await checkFile(page).click();
    await expect(page.getByTestId("restore-problem")).toContainText(message);
    await expect(page.getByTestId("restore-summary")).toHaveCount(0);
    await expect(page.getByTestId("restore-code")).toHaveCount(0);
  }
  hub.checkAnswer = {
    status: 422,
    body: { code: "VALIDATION_FAILED", message: "not_found", field: "path" },
  };
  await checkFile(page).click();
  await expect(page.getByTestId("restore-problem")).toContainText(
    en["hub.restore.problem.not_found"],
  );
  expect(hub.restores).toEqual([]);
});
