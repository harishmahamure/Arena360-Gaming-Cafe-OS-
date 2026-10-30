import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type { BackupStatus, GstArchiveEntry, HubSnapshot, SessionStaff } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { agoOf, isBehind, parseBackupStatus } from "../src/lib/backup-status";
import { parseGstArchives } from "../src/lib/gst-archive";
import { ORIGIN } from "./origin";

const HUB = "http://127.0.0.1:7409";
const CORS = {
  "access-control-allow-origin": ORIGIN,
  "access-control-allow-methods": "GET, POST",
  "access-control-allow-headers": "authorization, content-type",
};
const MANAGER: SessionStaff = {
  staff_id: "01JMANAGER0000000000000000",
  display_name: "Manager",
  role_name: "Manager",
  permissions: ["settings.edit", "report.view"],
};

function snapshot(): HubSnapshot {
  return {
    sessions: [],
    branch: { name: "Dev Branch", branch_code: "DEV01" },
    groups: [],
    zones: [],
    devices: [],
    locale: null,
    ui: { idle_lock_minutes: 5, hc: false },
    connectivity: { state: "online", checked_at: null },
    setup: null,
    lan: null,
    paired: true,
  } as unknown as HubSnapshot;
}

const ARCHIVED: GstArchiveEntry = {
  fy: "FY2026-27",
  label: "26-27",
  invoices: 120,
  credit_notes: 3,
  orphaned: 0,
  bytes: 4096,
  built_at: 1_800_000_000_000,
};

/** A paid Hub that backed up 4 minutes ago, on a 5-minute interval. */
function paid(over: Partial<BackupStatus> = {}): BackupStatus {
  const now = Date.now();
  return {
    cloud: true,
    as_of: now,
    upload_interval_s: 300,
    last_upload_at: now - 4 * 60_000,
    last_summary_at: now - 3 * 60_000,
    last_snapshot_at: now - 5 * 3_600_000,
    last_snapshot_date: "2027-01-14",
    last_keys_at: now - 2 * 3_600_000,
    last_gst_archive_at: null,
    pending_rows: 0,
    pending_batches: 0,
    first_backup_pending: false,
    error: null,
    ...over,
  };
}

/**
 * Serves the Hub's loopback API with `answer` as the GST archive list and `status` as the backup
 * status (a paid Hub that backed up 4 minutes ago when not given).
 */
async function stub(
  page: Page,
  answer: () => { status: number; body: unknown } | "down",
  status: () => BackupStatus = () => paid(),
) {
  await page.addInitScript(() => sessionStorage.setItem("arena.hub.token", "manager-token"));
  await page.route(`${HUB}/**`, (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    const json = (status: number, body: unknown) =>
      route.fulfill({
        status,
        contentType: "application/json",
        headers: CORS,
        body: JSON.stringify(body),
      });
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot":
        return json(200, snapshot());
      case "/ui/me":
        return json(200, MANAGER);
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: "retry: 60000\n\n",
        });
      case "/ui/backup/status":
        return json(200, status());
      case "/ui/backup/gst-archive": {
        const a = answer();
        return a === "down" ? route.abort() : json(a.status, a.body);
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
}

test("System -> Backup lists each archived year as saved", async ({ page }) => {
  await stub(page, () => ({
    status: 200,
    body: {
      archives: [
        { ...ARCHIVED, fy: "FY2027-28", label: "27-28", invoices: 5, orphaned: 2 },
        ARCHIVED,
      ],
    },
  }));
  await page.goto("/system/backup");
  await expect(page.getByRole("heading", { level: 2 })).toHaveText(en["hub.backup.section_title"]);
  const latest = page.getByTestId("gst-archive-FY2027-28");
  await expect(latest).toContainText("GST archive 27-28 saved");
  await expect(latest).toContainText("Invoices: 5");
  await expect(latest).toContainText("Invoice numbers set aside by a restore or promote: 2");
  const first = page.getByTestId("gst-archive-FY2026-27");
  await expect(first).toContainText("GST archive 26-27 saved");
  await expect(first).toContainText("Invoices: 120");
  await expect(first).toContainText("Credit notes: 3");
  await expect(first).not.toContainText("set aside");
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
});

test("a branch with no archive yet says so, and the section is in the System navigation", async ({
  page,
}) => {
  await stub(page, () => ({ status: 200, body: { archives: [] } }));
  await page.goto("/system");
  await page.getByRole("link", { name: en["hub.system.nav.backup"] }).click();
  await expect(page).toHaveURL(/\/system\/backup$/);
  await expect(page.getByTestId("gst-none")).toHaveText(en["hub.backup.gst.none"]);
});

test("a Hub that cannot be read, or a refusal, is said plainly", async ({ page }) => {
  await stub(page, () => "down");
  await page.goto("/system/backup");
  await expect(page.getByTestId("gst-unreadable")).toHaveText(en["hub.backup.gst.unreadable"]);

  const denied = await page.context().newPage();
  await stub(denied, () => ({ status: 403, body: { code: "PERMISSION_DENIED", message: "no" } }));
  await denied.goto("/system/backup");
  await expect(denied.getByTestId("gst-denied")).toHaveText(en["hub.backup.gst.denied"]);
});

test("only a well-formed list is read", () => {
  expect(parseGstArchives({ archives: [ARCHIVED] })).toEqual([ARCHIVED]);
  expect(parseGstArchives({ archives: [] })).toEqual([]);
  expect(parseGstArchives({ archives: [{ ...ARCHIVED, invoices: "many" }] })).toBeNull();
  expect(parseGstArchives({ archives: [{ fy: 1 }] })).toBeNull();
  expect(parseGstArchives({})).toBeNull();
  expect(parseGstArchives(null)).toBeNull();
});

const FREE = () => ({ ...paid(), cloud: false, upload_interval_s: 0 });
const NO_ARCHIVES = () => ({ status: 200, body: { archives: [] } });

test("the header says when the branch last backed up, with the pending batches", async ({
  page,
}) => {
  test.skip(test.info().project.name === "phone", "the phone header drops the backup status");
  await stub(page, NO_ARCHIVES, () => paid({ pending_batches: 3, pending_rows: 40 }));
  await page.goto("/system");
  const chip = page.getByTestId("hub-backup-status");
  await expect(chip).toContainText("Backed up 4 min ago · 3 pending");
  await expect(chip).toHaveAttribute("data-behind", "false");
  await expect(chip).toHaveAttribute(
    "title",
    "Last upload 4 min ago · last summary 3 min ago · 3 batches pending",
  );
  await expect(chip).toHaveAccessibleName(/Last upload 4 min ago · last summary 3 min ago/);
  await chip.click();
  await expect(page).toHaveURL(/\/system\/backup$/);
});

test("the header warns 'Backup behind' after twice the interval and says so politely", async ({
  page,
}) => {
  test.skip(test.info().project.name === "phone", "the phone header drops the backup status");
  await stub(page, NO_ARCHIVES, () => paid({ last_upload_at: Date.now() - 11 * 60_000 }));
  await page.goto("/system");
  const chip = page.getByTestId("hub-backup-status");
  await expect(chip).toHaveAttribute("data-behind", "true");
  await expect(chip).toContainText(en["hub.header.backup.behind"]);
  await expect(page.getByTestId("hub-announce")).toContainText("Backup is behind");
});

test("a paid Hub with no upload yet says the first backup is pending, not behind", async ({
  page,
}) => {
  test.skip(test.info().project.name === "phone", "the phone header drops the backup status");
  await stub(page, NO_ARCHIVES, () =>
    paid({ last_upload_at: null, first_backup_pending: true, pending_batches: 1 }),
  );
  await page.goto("/system");
  const chip = page.getByTestId("hub-backup-status");
  await expect(chip).toContainText(en["hub.header.backup.first"]);
  await expect(chip).toHaveAttribute("data-behind", "false");
});

test("a Free Hub shows no backup status in the header and only local exports in System", async ({
  page,
}) => {
  await stub(page, NO_ARCHIVES, FREE);
  await page.goto("/system");
  await expect(page.getByTestId("hub-branch")).toBeVisible();
  await expect(page.getByTestId("hub-backup-status")).toHaveCount(0);
  await page.goto("/system/backup");
  await expect(page.getByTestId("cloud-free")).toContainText("Available on Basic");
  await expect(page.getByTestId("local-exports")).toBeVisible();
  await expect(page.getByTestId("gst-archives")).toHaveCount(0);
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
});

test("below 840px the header drops the backup status and System keeps it", async ({ page }) => {
  await page.setViewportSize({ width: 600, height: 900 });
  await stub(page, NO_ARCHIVES, () => paid());
  await page.goto("/system");
  await expect(page.getByTestId("hub-branch")).toBeVisible();
  await expect(page.getByTestId("hub-backup-status")).toHaveCount(0);
  await page.goto("/system/backup");
  await expect(page.getByTestId("cloud-last-upload")).toHaveText("4 min ago");
});

test("System -> Backup lists each last time, the pending batches and the last error in words", async ({
  page,
}) => {
  await stub(page, NO_ARCHIVES, () =>
    paid({
      pending_batches: 2,
      error: { source: "upload", code: "CLOUD_UNREACHABLE", at: Date.now() - 60_000 },
    }),
  );
  await page.goto("/system/backup");
  await expect(page.getByTestId("cloud-last-upload")).toHaveText("4 min ago");
  await expect(page.getByTestId("cloud-last-summary")).toHaveText("3 min ago");
  await expect(page.getByTestId("cloud-last-snapshot")).toHaveText("5 h ago (for 2027-01-14)");
  await expect(page.getByTestId("cloud-last-keys")).toHaveText("2 h ago");
  await expect(page.getByTestId("cloud-last-gst")).toHaveText("not yet");
  await expect(page.getByTestId("cloud-pending")).toHaveText("2");
  await expect(page.getByTestId("cloud-error")).toContainText(
    "Uploads: The cloud couldn't be reached.",
  );
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
});

test("System -> Backup says 'First backup pending' after an upgrade", async ({ page }) => {
  await stub(page, NO_ARCHIVES, () =>
    paid({ last_upload_at: null, first_backup_pending: true, last_summary_at: null }),
  );
  await page.goto("/system/backup");
  await expect(page.getByTestId("cloud-first-pending")).toContainText("First backup pending");
  await expect(page.getByTestId("cloud-last-upload")).toHaveText("not yet");
});

test("the backup status helpers read the Hub's answer", () => {
  const now = 10_000_000;
  const s = paid({ as_of: now, last_upload_at: now - 600_000, upload_interval_s: 300 });
  expect(isBehind(s, now)).toBe(false);
  expect(isBehind(s, now + 1)).toBe(true);
  expect(isBehind({ ...s, last_upload_at: null }, now)).toBe(false);
  expect(isBehind({ ...s, cloud: false }, now)).toBe(false);
  expect(agoOf(now - 30_000, now)).toEqual({ key: "hub.backup.ago.now" });
  expect(agoOf(now - 4 * 60_000, now)).toEqual({ key: "hub.backup.ago.minutes", n: 4 });
  expect(agoOf(now - 3 * 3_600_000, now)).toEqual({ key: "hub.backup.ago.hours", n: 3 });
  expect(agoOf(now - 72 * 3_600_000, now)).toEqual({ key: "hub.backup.ago.days", n: 3 });
  expect(agoOf(null, now)).toEqual({ key: "hub.backup.ago.never" });
  expect(parseBackupStatus(s)).toEqual(s);
  expect(parseBackupStatus({ ...s, cloud: "yes" })).toBeNull();
  expect(parseBackupStatus({ cloud: true })).toBeNull();
  expect(parseBackupStatus(null)).toBeNull();
});
