import { expect, test } from "@playwright/test";
import { stubCloud, t } from "./stub";

// The footer theme toggle: System by default and following the OS live, Light and Dark
// remembered on this device across reloads (set before first paint).

const html = (page: import("@playwright/test").Page) => page.locator("html");
const tab = (page: import("@playwright/test").Page, key: Parameters<typeof t>[0]) =>
  page.getByRole("contentinfo").getByRole("tab", { name: t(key) });

test("System is the default and follows prefers-color-scheme live", async ({ page }) => {
  await stubCloud(page);
  await page.emulateMedia({ colorScheme: "light" });
  await page.goto("/");
  await expect(tab(page, "ui.theme.system")).toHaveAttribute("aria-selected", "true");
  await expect(html(page)).toHaveAttribute("data-theme", "light");
  await page.emulateMedia({ colorScheme: "dark" });
  await expect(html(page)).toHaveAttribute("data-theme", "dark");
});

test("a chosen theme is remembered across reloads", async ({ page }) => {
  await stubCloud(page);
  await page.emulateMedia({ colorScheme: "light" });
  await page.goto("/");
  await tab(page, "ui.theme.dark").click();
  await expect(html(page)).toHaveAttribute("data-theme", "dark");
  await page.reload();
  await expect(html(page)).toHaveAttribute("data-theme", "dark");
  await expect(tab(page, "ui.theme.dark")).toHaveAttribute("aria-selected", "true");
  // Light overrides a dark OS, too; arrow keys move between the choices.
  await page.emulateMedia({ colorScheme: "dark" });
  await tab(page, "ui.theme.dark").focus();
  await page.keyboard.press("ArrowLeft");
  await expect(tab(page, "ui.theme.light")).toHaveAttribute("aria-selected", "true");
  await page.reload();
  await expect(html(page)).toHaveAttribute("data-theme", "light");
});

test("the stored theme is set before the app starts", async ({ page }) => {
  await stubCloud(page);
  await page.addInitScript(() => localStorage.setItem("arena.theme", "dark"));
  await page.emulateMedia({ colorScheme: "light" });
  // Without JavaScript bundles running, only the inline script in app.html can set it.
  await page.route("**/_app/**", (route) => route.abort());
  await page.goto("/");
  await expect(html(page)).toHaveAttribute("data-theme", "dark");
});

test("High contrast is set before the app starts and wins over the OS", async ({ page }) => {
  await stubCloud(page);
  await page.emulateMedia({ colorScheme: "light" });
  await page.addInitScript(() => localStorage.setItem("arena.theme", "hc"));
  await page.goto("/sign-in");
  await expect(html(page)).toHaveAttribute("data-theme", "hc");
  await expect(tab(page, "ui.theme.hc")).toHaveAttribute("aria-selected", "true");
});
