import { readdirSync } from "node:fs";
import { expect, test, type Page } from "@playwright/test";
import { DESKTOP, PHONE, open, t } from "./helpers";

// Help centre: three categories and a search box; the launch article set, each with title, summary,
// last-reviewed date and a stable slug; the hardware table (from the versioned data file, tested
// badge, filters); the static search (no server call); and the support channel on every article.

const DIR = new URL("../src/content/help/", import.meta.url);
const SLUGS = readdirSync(DIR)
  .filter((file) => file.endsWith(".md"))
  .map((file) => file.replace(/\.md$/, ""));
const SETUP = [
  "install-the-hub-on-windows",
  "install-the-hub-on-android",
  "pair-a-kiosk",
  "set-up-a-printer-and-cash-drawer",
  "add-a-standby-pc",
  "invite-staff",
];
const TROUBLESHOOTING = [
  "kiosk-cannot-find-the-hub",
  "printer-offline",
  "hub-is-slow",
  "branch-shows-offline-in-the-owner-app",
  "restore-from-an-export",
];

const section = (page: Page, id: string) => page.locator(`section#${id}`);
const slugsIn = (page: Page, id: string) =>
  section(page, id)
    .locator("li[data-slug]")
    .evaluateAll((els) => els.map((el) => el.getAttribute("data-slug")));

test("Help shows Setup, Troubleshooting, Hardware compatibility and a search box", async ({
  page,
}) => {
  await page.setViewportSize(DESKTOP);
  await open(page, "/help");
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(t("site.help.heading"));
  await expect(page.getByRole("heading", { level: 2 })).toHaveText([
    t("site.help.category.setup"),
    t("site.help.category.troubleshooting"),
    t("site.help.hardware.heading"),
  ]);
  await expect(page.getByRole("searchbox", { name: t("site.help.search.label") })).toBeVisible();
  await expect(section(page, "hardware-compatibility")).toBeVisible();
});

test("the launch content set is there, each article with title, summary, reviewed date and slug", async ({
  page,
}) => {
  await open(page, "/help");
  expect((await slugsIn(page, "setup")).sort()).toEqual([...SETUP].sort());
  expect((await slugsIn(page, "troubleshooting")).sort()).toEqual([...TROUBLESHOOTING].sort());
  expect([...SLUGS].sort()).toEqual([...SETUP, ...TROUBLESHOOTING].sort());
  const item = section(page, "setup").locator('li[data-slug="pair-a-kiosk"]');
  await expect(item.getByRole("link", { name: "Pair a kiosk with your Hub" })).toHaveAttribute(
    "href",
    "/help/pair-a-kiosk",
  );
  await expect(item).toContainText("Install the Kiosk on a gaming PC");
  await expect(item).toContainText(t("site.help.reviewed", { date: "31 Oct 2026" }));
});

test("every article opens, has one h1, shows its reviewed date and the support channels", async ({
  page,
}) => {
  for (const slug of SLUGS) {
    const response = await page.goto(`/help/${slug}`);
    expect(response?.status(), slug).toBe(200);
    await expect(page.getByRole("heading", { level: 1 })).toHaveCount(1);
    await expect(page.locator(".help-reviewed")).toContainText("31 Oct 2026");
    const support = page.locator(".help-support");
    await expect(support).toContainText(t("site.help.support.free.channel"));
    await expect(support).toContainText(t("site.help.support.basic.channel"));
    await expect(support).toContainText(t("site.help.support.premium.channel"));
    // No personal data is asked for on the page.
    await expect(page.locator("main form, main input, main textarea")).toHaveCount(0);
  }
});

test("links between articles and to the hardware list resolve", async ({ page }) => {
  const targets = new Set<string>();
  for (const slug of SLUGS) {
    await page.goto(`/help/${slug}`);
    const hrefs = await page
      .locator(".help-body a[href^='/']")
      .evaluateAll((els) => els.map((el) => (el.getAttribute("href") ?? "").split("#")[0] ?? ""));
    hrefs.forEach((href) => targets.add(href));
  }
  expect(targets.size).toBeGreaterThan(0);
  for (const href of targets) {
    const response = await page.goto(href);
    expect(response?.status(), href).toBe(200);
  }
  await page.goto("/help#hardware-compatibility");
  await expect(page.locator("#hardware-compatibility")).toBeVisible();
});

test("search returns matching articles within 200 ms and makes no request", async ({ page }) => {
  await page.setViewportSize(DESKTOP);
  await open(page, "/help");
  const box = page.getByRole("searchbox", { name: t("site.help.search.label") });
  await expect(box).toBeEditable();
  const requests: string[] = [];
  page.on("request", (request) => requests.push(request.url()));

  const took = await box.evaluate(async (input: HTMLInputElement) => {
    const start = performance.now();
    input.value = "printer";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done)));
    return performance.now() - start;
  });
  expect(took).toBeLessThan(200);
  const results = page.getByRole("list", { name: t("site.help.search.results") });
  await expect(results.getByRole("link")).toHaveText([
    "Set up a printer and a cash drawer",
    "The printer is offline",
  ]);
  await expect(results).toContainText("Fixes for a receipt or kitchen printer");
  expect(requests).toEqual([]);
});

test("a search with no match says so", async ({ page }) => {
  await open(page, "/help");
  const box = page.getByRole("searchbox", { name: t("site.help.search.label") });
  await expect(box).toBeEditable();
  await box.fill("zxqv");
  await expect(page.getByText(t("site.help.search.none"))).toBeVisible();
  await expect(page.getByRole("list", { name: t("site.help.search.results") })).toHaveCount(0);
  await box.fill("");
  await expect(page.getByText(t("site.help.search.none"))).toHaveCount(0);
});

test("hardware rows come from the data file; Tested device marks both-Hub rows only", async ({
  page,
}) => {
  await page.setViewportSize(DESKTOP);
  await open(page, "/help");
  const rows = page.locator("section#hardware-compatibility tbody tr");
  await expect(rows).toHaveCount(4);
  await expect(rows.filter({ hasText: t("site.help.hardware.tested") })).toHaveCount(2);
  const northwind80 = rows.filter({ hasText: "NP-80" });
  await expect(northwind80).toContainText(t("site.help.hardware.tested"));
  await expect(northwind80).toContainText("80 mm");
  await expect(northwind80).toContainText("20 Oct 2026");
  await expect(rows.filter({ hasText: "NP-58" })).not.toContainText(t("site.help.hardware.tested"));
  await expect(rows.filter({ hasText: "CD-1" })).toContainText(t("site.help.hardware.type.drawer"));
});

test("the hardware table filters by brand and by connection", async ({ page }) => {
  await open(page, "/help");
  const rows = page.locator("section#hardware-compatibility tbody tr");
  const brand = page.getByLabel(t("site.help.hardware.brand"));
  const connection = page.getByLabel(t("site.help.hardware.connection"));
  await expect(rows).toHaveCount(4);
  await brand.selectOption("Contoso");
  await expect(rows).toHaveCount(2);
  await connection.selectOption("usb");
  await expect(rows).toHaveCount(1);
  await expect(rows.first()).toContainText("CD-1");
  await brand.selectOption("Northwind");
  await connection.selectOption("lan");
  await expect(page.getByText(t("site.help.hardware.no_match"))).toBeVisible();
  await brand.selectOption("");
  await connection.selectOption("");
  await expect(rows).toHaveCount(4);
});

test("no TV, CEC or smart-plug device is published", async ({ page }) => {
  await open(page, "/help");
  const text = (await page.locator("main").innerText()).toLowerCase();
  for (const word of ["hdmi-cec", " cec", "smart plug", "smart-plug", " tv "]) {
    expect(text, word).not.toContain(word);
  }
});

test("the sitemap lists every article", async ({ request }) => {
  const body = await (await request.get("/sitemap.xml")).text();
  expect(body).toContain("https://arena360.cloud/help</loc>");
  for (const slug of SLUGS) expect(body).toContain(`https://arena360.cloud/help/${slug}</loc>`);
});

test("the Help centre and an article fit a phone without sideways scroll", async ({ page }) => {
  await page.setViewportSize(PHONE);
  for (const path of ["/help", "/help/pair-a-kiosk"]) {
    await open(page, path);
    const overflow = await page.evaluate(
      () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
    );
    expect(overflow, path).toBeLessThanOrEqual(0);
  }
});
