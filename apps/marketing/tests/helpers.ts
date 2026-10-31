import type { Page } from "@playwright/test";
import { t } from "../../../packages/ui/src/messages/index.js";

export { t };

export const PHONE = { width: 390, height: 844 };
export const DESKTOP = { width: 1440, height: 900 };
export const THEMES = ["light", "dark", "hc"] as const;
export type Theme = (typeof THEMES)[number];

export const html = (page: Page) => page.locator("html");

/** Opens `path` painted in `theme` (stored the way the site stores it). */
export async function open(page: Page, path: string, theme?: Theme) {
  if (theme) {
    await page.addInitScript((value) => {
      if (value === "hc") localStorage.setItem("arena.contrast", "on");
      else {
        localStorage.setItem("arena.theme", value);
        localStorage.setItem("arena.contrast", "off");
      }
    }, theme);
  }
  return page.goto(path);
}
