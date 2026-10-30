import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type { GstArchiveEntry, HubSnapshot, SessionStaff } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
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

/** Serves the Hub's loopback API with `answer` as the GST archive list. */
async function stub(page: Page, answer: () => { status: number; body: unknown } | "down") {
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
