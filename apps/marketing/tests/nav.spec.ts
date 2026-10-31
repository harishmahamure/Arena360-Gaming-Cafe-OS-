import { expect, test, type Page } from "@playwright/test";
import { PHONE, open, t } from "./helpers";

// The phone menu: a full-screen dialog that traps focus, closes on Esc or a link and returns focus.

test.beforeEach(async ({ page }) => {
  await page.setViewportSize(PHONE);
  await open(page, "/");
});

const hamburger = (page: Page) => page.getByRole("button", { name: t("site.nav.menu_open") });
const menu = (page: Page) => page.getByRole("dialog", { name: t("site.nav.menu_title") });
const inMenu = (page: Page) => menu(page).evaluate((el) => el.contains(document.activeElement));

test("the hamburger opens a full-screen menu with the five links and the contrast switch", async ({
  page,
}) => {
  await expect(hamburger(page)).toHaveAttribute("aria-expanded", "false");
  await hamburger(page).click();
  await expect(menu(page)).toBeVisible();
  expect(await menu(page).boundingBox()).toMatchObject({
    x: 0,
    y: 0,
    width: PHONE.width,
    height: PHONE.height,
  });
  const nav = menu(page).getByRole("navigation", { name: t("site.nav.main") });
  await expect(nav.getByRole("link")).toHaveText([
    t("site.nav.product"),
    t("site.nav.pricing"),
    t("site.nav.downloads"),
    t("site.nav.help"),
    t("site.nav.faq"),
  ]);
  await expect(menu(page).getByRole("switch")).toBeVisible();
});

test("focus moves into the menu, stays there on Tab and Shift+Tab, and Esc returns it", async ({
  page,
}) => {
  await hamburger(page).click();
  expect(await inMenu(page)).toBe(true);
  const count = await menu(page).locator("a[href], button").count();
  for (let i = 0; i < count * 2 + 1; i++) {
    await page.keyboard.press("Tab");
    expect(await inMenu(page)).toBe(true);
  }
  for (let i = 0; i < count + 1; i++) {
    await page.keyboard.press("Shift+Tab");
    expect(await inMenu(page)).toBe(true);
  }
  await page.keyboard.press("Escape");
  await expect(menu(page)).toBeHidden();
  await expect(hamburger(page)).toBeFocused();
});

test("the close button and a link both close the menu", async ({ page }) => {
  await hamburger(page).click();
  await menu(page)
    .getByRole("button", { name: t("site.nav.menu_close") })
    .click();
  await expect(menu(page)).toBeHidden();
  await expect(hamburger(page)).toBeFocused();
  await hamburger(page).click();
  await menu(page)
    .getByRole("link", { name: t("site.nav.faq") })
    .click();
  await expect(menu(page)).toBeHidden();
});

test("widening the window past the phone width closes the menu", async ({ page }) => {
  await hamburger(page).click();
  await page.setViewportSize({ width: 1024, height: 800 });
  await expect(menu(page)).toBeHidden();
});

test("High contrast can be switched on from the menu", async ({ page }) => {
  await hamburger(page).click();
  await menu(page).getByRole("switch").click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "hc");
});
