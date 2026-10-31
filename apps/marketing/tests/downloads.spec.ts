import { readFileSync } from "node:fs";
import { expect, test, type Page } from "@playwright/test";
import { DESKTOP, PHONE, open, t } from "./helpers";

// Downloads: the installers from the release manifest (read at build time, then again in the
// browser), each with version, size, date, a copyable SHA-256 and a "How to verify" disclosure;
// "Coming soon" rows with no link; the dated build-time list when the browser cannot read the
// manifest; and the system requirements.

const MANIFEST = "https://downloads.arena360.cloud/releases/manifest.json";
const BUILD_TIME = JSON.parse(
  readFileSync(new URL("./fixtures/releases/manifest.json", import.meta.url), "utf8"),
) as { releases: Record<string, unknown>[] };
const HUB_SHA = String(BUILD_TIME.releases[0]?.sha256);
const PI = {
  product: "pi-image",
  channel: "stable",
  version: "0.9.0",
  file: "arena360-pi-0.9.0.img.xz",
  size: 220200960,
  sha256: "a".repeat(64),
  signature_url:
    "https://downloads.arena360.cloud/releases/pi-image/0.9.0/arena360-pi-0.9.0.img.xz.sig",
  published_at: "2026-11-02T10:00:00Z",
};

const row = (page: Page, id: string) => page.locator(`.dl-row[data-product="${id}"]`);
const downloadName = (name: string) => t("site.downloads.download", { name });

/** The browser's second read of the manifest answers with `releases`, or fails. */
async function manifest(page: Page, releases: Record<string, unknown>[] | "fail") {
  await page.route(MANIFEST, (route) =>
    releases === "fail"
      ? route.abort()
      : route.fulfill({
          contentType: "application/json",
          headers: { "access-control-allow-origin": "*" },
          body: JSON.stringify({ releases }),
        }),
  );
}

test("the page lists Windows Hub, Windows Kiosk, Android Hub and the Raspberry Pi image", async ({
  page,
}) => {
  await manifest(page, BUILD_TIME.releases);
  await page.setViewportSize(DESKTOP);
  await open(page, "/downloads");
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(t("site.downloads.heading"));
  await expect(page.locator(".dl-row h3")).toHaveText([
    t("site.downloads.hub.name"),
    t("site.downloads.kiosk.name"),
    t("site.downloads.android.name"),
    t("site.downloads.pi.name"),
  ]);
});

test("a released installer shows version, size, date and a copyable SHA-256", async ({ page }) => {
  await manifest(page, BUILD_TIME.releases);
  await page.setViewportSize(DESKTOP);
  await open(page, "/downloads");
  const hub = row(page, "hub-windows");
  await expect(hub.locator("dl")).toContainText("1.4.0");
  await expect(hub.locator("dl")).toContainText(t("site.downloads.mb", { mb: "84.3" }));
  await expect(hub.locator("dl")).toContainText("20 Oct 2026");
  const sha = hub.getByRole("textbox", { name: t("site.downloads.sha") });
  await expect(sha).toHaveValue(HUB_SHA);
  await expect(sha).toHaveJSProperty("readOnly", true);
  expect(await sha.evaluate((el) => getComputedStyle(el).fontFamily)).toMatch(/mono/i);
  await expect(hub.getByRole("link", { name: downloadName("Windows Hub") })).toHaveAttribute(
    "href",
    "https://downloads.arena360.cloud/releases/hub-windows/1.4.0/Arena360-Hub-1.4.0-x64.exe",
  );
});

test("the newest release of a product is the one listed", async ({ page }) => {
  await manifest(page, BUILD_TIME.releases);
  await open(page, "/downloads");
  const kiosk = row(page, "kiosk-windows");
  await expect(kiosk.locator("dl")).toContainText("1.4.0");
  await expect(kiosk.locator("dl")).not.toContainText("1.3.0");
});

test("the copy button copies the SHA-256", async ({ page, context }) => {
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await manifest(page, BUILD_TIME.releases);
  await open(page, "/downloads");
  const hub = row(page, "hub-windows");
  await hub.getByRole("button", { name: t("site.downloads.copy") }).click();
  await expect(hub.getByRole("button", { name: t("site.downloads.copied") })).toBeVisible();
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(HUB_SHA);
});

test("How to verify opens on the PowerShell Get-FileHash command for that file", async ({
  page,
}) => {
  await manifest(page, BUILD_TIME.releases);
  await open(page, "/downloads");
  const details = row(page, "hub-windows").locator("details");
  await expect(details).not.toHaveAttribute("open", "");
  await details.getByText(t("site.downloads.verify"), { exact: true }).click();
  await expect(details.locator("pre")).toHaveText(
    "Get-FileHash .\\Arena360-Hub-1.4.0-x64.exe -Algorithm SHA256",
  );
});

test("a product missing from the manifest reads Coming soon and has no link", async ({ page }) => {
  await manifest(page, BUILD_TIME.releases);
  await open(page, "/downloads");
  const pi = row(page, "pi-image");
  await expect(pi.getByText(t("site.downloads.soon"))).toBeVisible();
  await expect(pi.getByRole("link")).toHaveCount(0);
  await expect(pi.getByRole("textbox")).toHaveCount(0);
});

test("Android Hub is a Google Play link, not a file", async ({ page }) => {
  await manifest(page, BUILD_TIME.releases);
  await open(page, "/downloads");
  await expect(row(page, "hub-android").getByRole("link")).toHaveAttribute(
    "href",
    "https://play.google.com/store/apps/details?id=in.arena360.hub",
  );
  const hrefs = await page
    .locator("main a")
    .evaluateAll((els) => els.map((e) => e.getAttribute("href")));
  expect(hrefs.filter((href) => /\.apk$/i.test(href ?? ""))).toEqual([]);
});

test("a release published after the build appears without a redeploy", async ({ page }) => {
  await manifest(page, [...BUILD_TIME.releases, PI]);
  await open(page, "/downloads");
  const pi = row(page, "pi-image");
  await expect(pi.getByRole("link", { name: downloadName("Raspberry Pi image") })).toHaveAttribute(
    "href",
    "https://downloads.arena360.cloud/releases/pi-image/0.9.0/arena360-pi-0.9.0.img.xz",
  );
  await expect(pi.getByRole("textbox", { name: t("site.downloads.sha") })).toHaveValue(
    "a".repeat(64),
  );
  await expect(page.locator(".dl-stale")).toHaveCount(0);
});

test("a manifest line without a checksum gets no link", async ({ page }) => {
  await manifest(page, [...BUILD_TIME.releases, { ...PI, sha256: "" }]);
  await open(page, "/downloads");
  await expect(row(page, "hub-windows").getByRole("link")).toHaveCount(1);
  const pi = row(page, "pi-image");
  await expect(pi.getByText(t("site.downloads.soon"))).toBeVisible();
  await expect(pi.getByRole("link")).toHaveCount(0);
});

test("if the browser cannot read the manifest the build-time list stays, dated", async ({
  page,
}) => {
  await manifest(page, "fail");
  await open(page, "/downloads");
  await expect(page.locator(".dl-stale")).toHaveText(
    /^Showing the list from \d{1,2} [A-Z][a-z]{2} \d{4}$/,
  );
  for (const id of ["hub-windows", "kiosk-windows"]) {
    await expect(row(page, id).getByRole("link", { name: /^Download / })).toHaveCount(1);
  }
  // Every file link sits in a row that shows its checksum.
  for (const link of await page.getByRole("link", { name: /^Download / }).all()) {
    await expect(link.locator("xpath=ancestor::li[1]").getByRole("textbox")).toHaveValue(
      /^[0-9a-f]{64}$/,
    );
  }
});

test("the system requirements state each device's needs and link to the Help centre", async ({
  page,
}) => {
  await manifest(page, BUILD_TIME.releases);
  await open(page, "/downloads");
  const req = page.locator("section#requirements");
  await expect(req.getByRole("heading", { level: 2 })).toHaveText(t("site.downloads.req.heading"));
  await expect(req).toContainText(
    "Windows 10 version 1803 to Windows 11, 64-bit, 4 GB RAM, WebView2 Evergreen",
  );
  await expect(req).toContainText("Windows Hub: Windows 10 or later");
  await expect(req).toContainText("Android 9 or later, 3 GB RAM, 20 GB free");
  await expect(req).toContainText("Windows 7 is not supported.");
  await expect(req).toContainText("any ESC/POS");
  await expect(req.getByRole("link")).toHaveAttribute("href", "/help#hardware-compatibility");
});

test("at 390 px the rows stack and nothing scrolls sideways", async ({ page }) => {
  await manifest(page, BUILD_TIME.releases);
  await page.setViewportSize(PHONE);
  await open(page, "/downloads");
  const hub = row(page, "hub-windows");
  await hub.locator("summary").click();
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  );
  expect(overflow).toBeLessThanOrEqual(0);
  const heading = await hub.locator("h3").boundingBox();
  const facts = await hub.locator("dl").boundingBox();
  expect(facts!.y).toBeGreaterThan(heading!.y);
});
