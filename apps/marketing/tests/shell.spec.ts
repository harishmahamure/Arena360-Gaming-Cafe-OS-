import { expect, test } from "@playwright/test";
import { DESKTOP, PHONE, html, open, t } from "./helpers";

// The shell on every page: landmarks, header sizes, skip link, footer links, sitemap and the 404.

test("a page has the four landmarks, one h1, and a title, description and canonical", async ({
  page,
}) => {
  await page.setViewportSize(DESKTOP);
  await open(page, "/");
  await expect(page.getByRole("banner")).toHaveCount(1);
  await expect(page.getByRole("main")).toHaveCount(1);
  await expect(page.getByRole("contentinfo")).toHaveCount(1);
  await expect(page.getByRole("navigation", { name: t("site.nav.main") })).toBeVisible();
  await expect(page.getByRole("heading", { level: 1 })).toHaveCount(1);
  await expect(page).toHaveTitle(t("site.home.title"));
  await expect(page.locator('meta[name="description"]')).toHaveAttribute(
    "content",
    t("site.home.description"),
  );
  await expect(page.locator('link[rel="canonical"]')).toHaveAttribute(
    "href",
    "https://arena360.cloud/",
  );
  await expect(page.locator('meta[property="og:title"]')).toHaveAttribute(
    "content",
    t("site.home.title"),
  );
  await expect(page.locator('meta[property="og:url"]')).toHaveAttribute(
    "content",
    "https://arena360.cloud/",
  );
  await expect(page.locator('meta[name="robots"]')).toHaveCount(0);
});

test("the skip link is first in the tab order and moves focus to main", async ({ page }) => {
  await open(page, "/");
  await page.keyboard.press("Tab");
  const skip = page.getByRole("link", { name: t("site.skip") });
  await expect(skip).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.locator("main")).toBeFocused();
});

test("at 1440 px the header is 80 px with the five links and one Get started", async ({ page }) => {
  await page.setViewportSize(DESKTOP);
  await open(page, "/");
  const bar = page.locator(".site-header .bar");
  expect((await bar.boundingBox())?.height).toBe(80);
  const nav = page.getByRole("banner").getByRole("navigation", { name: t("site.nav.main") });
  await expect(nav.getByRole("link")).toHaveText([
    t("site.nav.product"),
    t("site.nav.pricing"),
    t("site.nav.downloads"),
    t("site.nav.help"),
    t("site.nav.faq"),
  ]);
  await expect(page.getByRole("link", { name: t("site.header.home") })).toHaveAttribute(
    "href",
    "/",
  );
  const start = page.getByRole("banner").getByRole("link", { name: t("site.cta.get_started") });
  await expect(start).toHaveCount(1);
  await expect(start).toHaveAttribute("href", "https://start.arena360.cloud");
  await expect(page.getByRole("button", { name: t("site.nav.menu_open") })).toBeHidden();
  // Web gutter 56 px and section gap 128 px.
  await expect(bar).toHaveCSS("padding-left", "56px");
  await expect(page.locator("main")).toHaveCSS("padding-top", "128px");
  await expect(page.locator("main")).toHaveCSS("padding-left", "56px");
});

test("at 390 px the header is 64 px, sticky, with the hamburger and Get started", async ({
  page,
}) => {
  await page.setViewportSize(PHONE);
  await open(page, "/");
  const header = page.locator(".site-header");
  expect((await page.locator(".site-header .bar").boundingBox())?.height).toBe(64);
  await expect(page.getByRole("button", { name: t("site.nav.menu_open") })).toBeVisible();
  await expect(
    page.getByRole("banner").getByRole("link", { name: t("site.cta.get_started") }),
  ).toBeVisible();
  await expect(header).toHaveCSS("position", "sticky");
  await expect(page.locator("main")).toHaveCSS("padding-top", "64px");
  // Sticky: the header stays in view while the page scrolls.
  await page.addStyleTag({ content: "main { min-height: 3000px; }" });
  await page.evaluate(() => window.scrollTo(0, 1500));
  expect((await header.boundingBox())?.y).toBe(0);
});

test("the footer links Legal, Help, FAQ and Downloads and holds the theme toggle", async ({
  page,
}) => {
  await open(page, "/");
  const footer = page.getByRole("contentinfo");
  const hrefs = await footer
    .getByRole("link")
    .evaluateAll((els) => els.map((e) => e.getAttribute("href")));
  expect(hrefs).toEqual([
    "/help",
    "/faq",
    "/downloads",
    "/legal/terms",
    "/legal/privacy",
    "/legal/data-processing",
    "/legal/ad-policy",
  ]);
  await expect(footer.getByRole("tab")).toHaveText([
    t("ui.theme.system"),
    t("ui.theme.light"),
    t("ui.theme.dark"),
  ]);
  await expect(
    footer.getByRole("switch", { name: new RegExp(t("site.theme.contrast")) }),
  ).toBeVisible();
});

test("an unknown path returns the 404 page in the same shell with links to Home and Help", async ({
  page,
}) => {
  const response = await open(page, "/no-such-page");
  expect(response?.status()).toBe(404);
  await expect(page.locator('link[rel="canonical"]')).toHaveCount(0);
  await expect(page.locator('meta[name="robots"]')).toHaveAttribute("content", /noindex/);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(t("site.notfound.heading"));
  await expect(page.getByRole("banner")).toBeVisible();
  await expect(page.getByRole("contentinfo")).toBeVisible();
  const main = page.getByRole("main");
  await expect(main.getByRole("link", { name: t("site.notfound.home") })).toHaveAttribute(
    "href",
    "/",
  );
  await expect(main.getByRole("link", { name: t("site.notfound.help") })).toHaveAttribute(
    "href",
    "/help",
  );
  await expect(html(page)).toHaveAttribute("data-theme", /light|dark|hc/);
});

test("sitemap.xml lists the pages and robots.txt points to it", async ({ request }) => {
  const sitemap = await request.get("/sitemap.xml");
  expect(sitemap.ok()).toBe(true);
  const xml = await sitemap.text();
  expect(xml).toContain("<loc>https://arena360.cloud/</loc>");
  expect(xml).not.toContain("404");
  const robots = await (await request.get("/robots.txt")).text();
  expect(robots).toContain("Sitemap: https://arena360.cloud/sitemap.xml");
  expect(robots).toContain("Allow: /");
});

test("the font is served from the site and nothing loads from another origin", async ({ page }) => {
  const foreign: string[] = [];
  const fonts: string[] = [];
  page.on("request", (request) => {
    const url = new URL(request.url());
    if (url.origin !== "http://127.0.0.1:5180") foreign.push(request.url());
    if (/\.woff2?$/.test(url.pathname)) fonts.push(url.pathname);
  });
  await open(page, "/");
  await page.waitForLoadState("networkidle");
  expect(foreign).toEqual([]);
  expect(fonts.length).toBeGreaterThan(0);
});
