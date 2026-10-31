import { expect, test, type Page } from "@playwright/test";
import { html, open, t } from "./helpers";

// System · Light · Dark follow the OS and are remembered; High contrast is its own switch and
// defaults from prefers-contrast; all of it is painted before first paint.

const footer = (page: Page) => page.getByRole("contentinfo");
const tab = (page: Page, key: Parameters<typeof t>[0]) =>
  footer(page).getByRole("tab", { name: t(key) });
const contrast = (page: Page) =>
  footer(page).getByRole("switch", { name: new RegExp(t("site.theme.contrast")) });

test("System is the default and follows prefers-color-scheme live", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "light" });
  await open(page, "/");
  await expect(tab(page, "ui.theme.system")).toHaveAttribute("aria-selected", "true");
  await expect(html(page)).toHaveAttribute("data-theme", "light");
  await page.emulateMedia({ colorScheme: "dark" });
  await expect(html(page)).toHaveAttribute("data-theme", "dark");
});

test("a chosen mode is remembered per device and overrides the OS", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "light" });
  await open(page, "/");
  await tab(page, "ui.theme.dark").click();
  await expect(html(page)).toHaveAttribute("data-theme", "dark");
  await page.reload();
  await expect(html(page)).toHaveAttribute("data-theme", "dark");
  await expect(tab(page, "ui.theme.dark")).toHaveAttribute("aria-selected", "true");
  await page.emulateMedia({ colorScheme: "dark" });
  await tab(page, "ui.theme.dark").focus();
  await page.keyboard.press("ArrowLeft");
  await expect(tab(page, "ui.theme.light")).toHaveAttribute("aria-selected", "true");
  await page.reload();
  await expect(html(page)).toHaveAttribute("data-theme", "light");
});

test("the stored mode is painted before any script bundle runs", async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem("arena.theme", "dark"));
  await page.emulateMedia({ colorScheme: "light" });
  await page.route("**/_astro/**", (route) => route.abort());
  await page.goto("/");
  await expect(html(page)).toHaveAttribute("data-theme", "dark");
});

test("data-theme is already right when the body starts to parse", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "light" });
  await page.addInitScript(() => localStorage.setItem("arena.theme", "dark"));
  const seen: (string | null)[] = [];
  await page.exposeFunction("seenTheme", (value: string | null) => seen.push(value));
  await page.addInitScript(() => {
    const observer = new MutationObserver(() => {
      if (!document.body) return;
      (window as unknown as { seenTheme(v: string | null): void }).seenTheme(
        document.documentElement.getAttribute("data-theme"),
      );
      observer.disconnect();
    });
    observer.observe(document, { childList: true, subtree: true });
  });
  await page.goto("/");
  await expect.poll(() => seen.length).toBeGreaterThan(0);
  expect(seen).toEqual(["dark"]);
});

test("High contrast is a separate switch: on paints hc, off returns to the mode", async ({
  page,
}) => {
  await page.emulateMedia({ colorScheme: "light" });
  await open(page, "/");
  await expect(contrast(page)).toHaveAttribute("aria-checked", "false");
  await contrast(page).click();
  await expect(html(page)).toHaveAttribute("data-theme", "hc");
  await expect(tab(page, "ui.theme.system")).toHaveAttribute("aria-selected", "true");
  await page.reload();
  await expect(html(page)).toHaveAttribute("data-theme", "hc");
  await expect(contrast(page)).toHaveAttribute("aria-checked", "true");
  await contrast(page).click();
  await expect(html(page)).toHaveAttribute("data-theme", "light");
});

test("prefers-contrast: more turns High contrast on until the visitor chooses", async ({
  page,
}) => {
  await page.emulateMedia({ colorScheme: "light", contrast: "more" });
  await open(page, "/");
  await expect(html(page)).toHaveAttribute("data-theme", "hc");
  await expect(contrast(page)).toHaveAttribute("aria-checked", "true");
  await contrast(page).click();
  await expect(html(page)).toHaveAttribute("data-theme", "light");
  await page.reload();
  await expect(html(page)).toHaveAttribute("data-theme", "light");
  await expect(contrast(page)).toHaveAttribute("aria-checked", "false");
});

test("High contrast uses the hc tokens, 2 px white outlines and no shadows", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "light" });
  await open(page, "/no-such-page", "hc");
  const styles = await page.evaluate(() => {
    const body = getComputedStyle(document.body);
    const button = getComputedStyle(document.querySelector<HTMLElement>(".link-button")!);
    return {
      bg: body.backgroundColor,
      color: body.color,
      outline: `${button.outlineStyle} ${button.outlineWidth} ${button.outlineColor}`,
      shadow: button.boxShadow,
    };
  });
  expect(styles.bg).toBe("rgb(0, 0, 0)");
  expect(styles.color).toBe("rgb(255, 255, 255)");
  expect(styles.outline).toBe("solid 2px rgb(255, 255, 255)");
  expect(styles.shadow).toBe("none");
});

test("in light mode, links in body text are underlined in the accent text colour", async ({
  page,
}) => {
  await open(page, "/no-such-page", "light");
  const style = await page.evaluate(() => {
    const a = document.createElement("a");
    a.href = "/x";
    a.textContent = "x";
    document.querySelector("main p")!.append(a);
    const s = getComputedStyle(a);
    return [s.color, s.textDecorationLine];
  });
  expect(style).toEqual(["rgb(173, 14, 99)", "underline"]);
});
