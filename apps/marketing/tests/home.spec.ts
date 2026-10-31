import { expect, test } from "@playwright/test";
import { DESKTOP, PHONE, THEMES, open, t } from "./helpers";

// Home: hero, the three steps, "What you get", the closing band, and the media rules.

const TABLET = { width: 900, height: 1000 };

test("at 1440 px the hero shows the promise in web-hero on the left and the Hub shot on the right", async ({
  page,
}) => {
  await page.setViewportSize(DESKTOP);
  await open(page, "/");
  const h1 = page.getByRole("heading", { level: 1 });
  await expect(h1).toHaveText(t("site.home.heading"));
  const style = await h1.evaluate((el) => {
    const s = getComputedStyle(el);
    return { size: s.fontSize, weight: s.fontWeight, line: s.lineHeight };
  });
  expect(style).toEqual({ size: "64px", weight: "200", line: "64px" });

  const copy = (await page.locator(".hero-copy").boundingBox())!;
  const shot = (await page.locator(".hub-shot").boundingBox())!;
  expect(copy.x + copy.width).toBeLessThanOrEqual(DESKTOP.width * 0.45 + 56);
  expect(shot.x).toBeGreaterThanOrEqual(copy.x + copy.width);

  const hero = page.locator(".hero");
  await expect(hero.getByText(t("site.home.lede"))).toBeVisible();
  const start = hero.getByRole("link", { name: t("site.cta.get_started") });
  await expect(start).toHaveAttribute("href", "https://start.arena360.cloud");
  await expect(page.getByRole("figure", { name: t("site.home.shot.label") })).toBeVisible();
});

test("below 1024 px the title is web-display and the shot stacks under the copy", async ({
  page,
}) => {
  for (const size of [TABLET, PHONE]) {
    await page.setViewportSize(size);
    await open(page, "/");
    const h1 = page.getByRole("heading", { level: 1 });
    expect(await h1.evaluate((el) => getComputedStyle(el).fontSize)).toBe("40px");
    const copy = (await page.locator(".hero-copy").boundingBox())!;
    const shot = (await page.locator(".hub-shot").boundingBox())!;
    expect(shot.y).toBeGreaterThanOrEqual(copy.y + copy.height);
  }
});

test("sections follow in order, 128 px apart (64 px on phone)", async ({ page }) => {
  await page.setViewportSize(DESKTOP);
  await open(page, "/");
  await expect(page.getByRole("heading", { level: 2 })).toHaveText([
    t("site.home.steps.title"),
    t("site.home.gets.title"),
    t("site.home.band.title"),
  ]);

  const steps = page.locator("ol.home-grid > li");
  await expect(steps).toHaveCount(3);
  for (const [i, key] of (["install", "pair", "play"] as const).entries()) {
    await expect(steps.nth(i).getByRole("heading", { level: 3 })).toHaveText(
      t(`site.home.steps.${key}.title`),
    );
    await expect(steps.nth(i)).toContainText(t("site.home.steps.step", { n: i + 1 }));
  }
  await expect(page.locator(".home-section").nth(1).locator(".home-card")).toHaveCount(3);

  const gap = () =>
    page
      .locator(".home-section")
      .first()
      .evaluate((el) => getComputedStyle(el).paddingTop);
  expect(await gap()).toBe("128px");
  await page.setViewportSize(PHONE);
  expect(await gap()).toBe("64px");
  const scroll = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  );
  expect(scroll).toBeLessThanOrEqual(0);
});

test("the price card quotes the live price book, per branch, with GST", async ({ page }) => {
  await open(page, "/");
  const card = page.locator(".home-card", { hasText: t("site.home.gets.price.title") });
  await expect(card).toContainText("Basic ₹200");
  await expect(card).toContainText("Premium ₹1,999");
  await expect(card).toContainText("18% GST");
  await expect(card.getByRole("link", { name: t("site.home.gets.price.link") })).toHaveAttribute(
    "href",
    "/pricing",
  );
});

test("the hero stays dark in light mode and is solid black in high contrast", async ({ page }) => {
  await page.setViewportSize(DESKTOP);
  const hero = page.locator(".hero");
  const bg = () => hero.evaluate((el) => getComputedStyle(el).backgroundColor);
  const body = () => page.locator("body").evaluate((el) => getComputedStyle(el).backgroundColor);

  await open(page, "/", "light");
  await expect(hero).toHaveAttribute("data-theme", "dark");
  const light = await bg();
  expect(light).not.toBe(await body());
  await expect(page.locator(".hero .scrim")).toBeVisible();
  // The page below the hero is "Cool paper"; the hero's own links keep the hero ink.
  const link = page.locator(".hero .hub-shot a, .hero a:not(.link-button)");
  expect(await link.count()).toBe(0);

  await open(page, "/", "dark");
  await expect(hero).toHaveAttribute("data-theme", "dark");
  expect(await bg()).toBe(light);

  await open(page, "/", "hc");
  await expect(hero).toHaveAttribute("data-theme", "hc");
  expect(await bg()).toBe("rgb(0, 0, 0)");
  await expect(page.locator(".hero .scrim")).toBeHidden();
  await expect(page.locator(".hero .cover")).toBeHidden();
});

test("switching to high contrast turns the hero solid without a reload", async ({ page }) => {
  await page.setViewportSize(DESKTOP);
  await open(page, "/", "dark");
  await expect(page.locator(".hero")).toHaveAttribute("data-theme", "dark");
  await page.evaluate(() => document.documentElement.setAttribute("data-theme", "hc"));
  await expect(page.locator(".hero")).toHaveAttribute("data-theme", "hc");
  await page.evaluate(() => document.documentElement.setAttribute("data-theme", "light"));
  await expect(page.locator(".hero")).toHaveAttribute("data-theme", "dark");
});

test("no element uses backdrop-filter or a live blur", async ({ page }) => {
  for (const theme of THEMES) {
    await open(page, "/", theme);
    const offenders = await page.evaluate(() =>
      [...document.querySelectorAll("*")]
        .filter((el) => {
          const s = getComputedStyle(el);
          return (s.backdropFilter && s.backdropFilter !== "none") || /blur/.test(s.filter ?? "");
        })
        .map((el) => el.className || el.tagName),
    );
    expect(offenders).toEqual([]);
  }
});

test("with no video cleared, the still cover shows and nothing plays or asks to be paused", async ({
  page,
}) => {
  await page.setViewportSize(DESKTOP);
  await open(page, "/", "dark");
  await expect(page.locator(".hero .cover")).toBeVisible();
  await expect(page.locator("video")).toHaveCount(0);
  await expect(page.locator(".video-control")).toHaveCount(0);
});

test("Home is prerendered: the content is in the static HTML without running scripts", async ({
  browser,
  baseURL,
}) => {
  const context = await browser.newContext({ javaScriptEnabled: false, baseURL });
  const page = await context.newPage();
  await page.goto("/");
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(t("site.home.heading"));
  await expect(page.locator(".home-card")).toHaveCount(6);
  await expect(page.locator(".hero .cover")).toBeVisible();
  await context.close();
});
