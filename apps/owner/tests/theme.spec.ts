import { expect, test } from "@playwright/test";
import { FREE, stubCloud, t } from "./stub";

const html = (page: import("@playwright/test").Page) => page.locator("html");
const tab = (
  page: import("@playwright/test").Page,
  key: "ui.theme.system" | "ui.theme.light" | "ui.theme.dark" | "ui.theme.hc",
) => page.getByRole("tab", { name: t(key) });

test("System follows prefers-color-scheme live; a choice is remembered per device", async ({
  page,
}) => {
  await stubCloud(page);
  await page.emulateMedia({ colorScheme: "light" });
  await page.goto("/account");
  await expect(tab(page, "ui.theme.system")).toHaveAttribute("aria-selected", "true");
  await expect(html(page)).toHaveAttribute("data-theme", "light");
  await page.emulateMedia({ colorScheme: "dark" });
  await expect(html(page)).toHaveAttribute("data-theme", "dark");
  await tab(page, "ui.theme.light").click();
  await expect(html(page)).toHaveAttribute("data-theme", "light");
  await page.reload();
  await expect(html(page)).toHaveAttribute("data-theme", "light");
  await expect(tab(page, "ui.theme.light")).toHaveAttribute("aria-selected", "true");
});

test("High contrast beats the OS, is remembered per device and survives sign-out", async ({
  page,
}) => {
  await stubCloud(page);
  await page.route("**/sign-in", (route) => route.fulfill({ body: "sign in" }));
  await page.emulateMedia({ colorScheme: "light" });
  await page.goto("/account");
  await tab(page, "ui.theme.hc").click();
  await expect(html(page)).toHaveAttribute("data-theme", "hc");
  await expect(tab(page, "ui.theme.hc")).toHaveAttribute("aria-selected", "true");
  await page.emulateMedia({ colorScheme: "dark" });
  await expect(html(page)).toHaveAttribute("data-theme", "hc");
  await page.reload();
  await expect(html(page)).toHaveAttribute("data-theme", "hc");
  await page.getByRole("button", { name: t("owner.account.sign_out") }).click();
  await page.waitForURL(/\/sign-in$/);
  await page.goto("/account");
  await expect(html(page)).toHaveAttribute("data-theme", "hc");
  expect(await page.evaluate(() => localStorage.getItem("arena.theme"))).toBe("hc");
});

test("High contrast: 2px outline, yellow focus ring, no shadows, labelled states", async ({
  page,
}) => {
  await stubCloud(page, FREE);
  await page.addInitScript(() => localStorage.setItem("arena.theme", "hc"));
  await page.goto("/reports");
  await expect(page.getByText(t("owner.loading"))).toHaveCount(0);
  const link = page.getByRole("link", { name: t("owner.nav.alerts") }).first();
  const css = await link.evaluate((el) => {
    const s = getComputedStyle(el);
    return [s.outlineStyle, s.outlineWidth, s.boxShadow, s.transitionDuration];
  });
  expect(css).toEqual(["solid", "2px", "none", "0s"]);
  const solid = await page
    .locator(".nav")
    .evaluateAll((els) => els.map((el) => getComputedStyle(el).backgroundColor));
  expect(solid.filter((c) => /rgba\(.*, 0\)|, 0\.\d+\)/.test(c))).toEqual([]);
  const button = page.getByRole("button").first();
  if (await button.count())
    expect(await button.evaluate((el) => getComputedStyle(el).outlineWidth)).toBe("2px");
  await link.focus();
  await page.keyboard.press("Tab");
  await page.keyboard.press("Shift+Tab");
  const ring = await link.evaluate((el) => {
    const s = getComputedStyle(el);
    return [s.outlineWidth, s.outlineColor];
  });
  expect(ring).toEqual(["4px", "rgb(255, 225, 77)"]);
  await expect(
    page.getByText(t("staff.tier.available_on", { tier: t("owner.tier.basic") })).first(),
  ).toBeVisible();
  await page.context().setOffline(true);
  await page.evaluate(() => window.dispatchEvent(new Event("offline")));
  await expect(page.getByText(t("owner.offline"))).toBeVisible();
});

test("System with forced colors keeps every control visible and focusable", async ({ page }) => {
  await stubCloud(page);
  await page.emulateMedia({ forcedColors: "active" });
  await page.goto("/account");
  expect(await page.evaluate(() => matchMedia("(forced-colors: active)").matches)).toBe(true);
  await expect(html(page)).not.toHaveAttribute("data-theme", "hc");
  await expect(page.getByRole("button", { name: t("owner.account.sign_out") })).toBeVisible();
  // Live view is a sidebar entry only: on a phone it is not on the screen.
  const controls = page.locator("a:visible, button:visible");
  const n = await controls.count();
  expect(n).toBeGreaterThan(5);
  for (let i = 0; i < n; i++) {
    const c = controls.nth(i);
    await expect(c).toBeVisible();
    await c.focus();
    await expect(c).toBeFocused();
  }
});
