import { expect, test, type Page } from "@playwright/test";
import names from "../src-tauri/ipc-names.json" with { type: "json" };
import { SCREENS } from "./screens";
import { emit, serious } from "./support";

// Story 6.37: with the kiosk's high contrast on (Story 1.21) every player screen drops media,
// scrims, shadows and translucency, gives every interactive surface a 2px white outline, and
// passes the axe contrast checks with no serious or critical violation. The Tauri shell is
// stubbed in the page (see `support.ts`); the `theme` event turns HC on as the shell does.

// A Windows file URL path starts with a slash before the drive letter.
const HC_INIT = decodeURIComponent(
  new URL("./fixtures/theme-init-hc.js", import.meta.url).pathname,
).replace(/^\/(?=[A-Za-z]:)/, "");

/** Wraps a screen so HC is on before the screen's own checks and again after it has rendered. */
async function inHighContrast(page: Page, open: (page: Page) => Promise<void>) {
  // The shell sets the theme before any page script runs (Story 1.21): the fixture is its real
  // initialization script for HC.
  await page.addInitScript({ path: HC_INIT });
  await open(page);
  await emit(page, names.events.theme, { hc: true });
  await expect(page.locator("html")).toHaveAttribute("data-theme", "hc");
  await expect(page.getByTestId("game-media-scrim")).toHaveCount(0);
}

/** What on this page still has a shadow, a see-through fill, a blur or a plain outline. */
const hcProblems = (page: Page) =>
  page.evaluate(() => {
    const out: string[] = [];
    const label = (el: Element) =>
      el.getAttribute("data-testid") ?? `${el.tagName.toLowerCase()}.${el.className}`;
    const shown = (el: Element) => {
      const r = el.getBoundingClientRect();
      const s = getComputedStyle(el);
      return r.width > 0 && r.height > 0 && s.visibility !== "hidden" && s.display !== "none";
    };
    if (document.querySelector("video")) out.push("a video is on the page");
    for (const el of document.querySelectorAll<HTMLElement>("body *")) {
      if (!shown(el)) continue;
      const s = getComputedStyle(el);
      if (s.boxShadow !== "none") out.push(`${label(el)} has a shadow`);
      if ((s.backdropFilter || "none") !== "none") out.push(`${label(el)} blurs what is behind`);
      if (s.filter !== "none") out.push(`${label(el)} has a filter`);
      const bg = /^rgba\((\d+), (\d+), (\d+), ([\d.]+)\)$/.exec(s.backgroundColor);
      if (bg && Number(bg[4]) > 0 && Number(bg[4]) < 1)
        out.push(`${label(el)} has a see-through fill`);
      if (s.backgroundImage.includes("gradient")) out.push(`${label(el)} has a gradient scrim`);
    }
    const focused = document.activeElement;
    for (const el of document.querySelectorAll<HTMLElement>(
      "button, input, select, textarea, a[href], [role=radio], [role=slider]",
    )) {
      if (!shown(el) || el === focused) continue;
      const s = getComputedStyle(el);
      const width = parseFloat(s.outlineWidth);
      if (s.outlineStyle === "none" || width < 2) {
        out.push(`${label(el)} has no 2px outline`);
      } else if (s.outlineColor !== "rgb(255, 255, 255)") {
        out.push(`${label(el)} outline is ${s.outlineColor}, not white`);
      }
    }
    return out;
  });

for (const [name, open] of Object.entries(SCREENS)) {
  test(`${name}: no media, scrims, shadows or translucency, and a white 2px outline on every control`, async ({
    page,
  }) => {
    await inHighContrast(page, open);
    expect(await hcProblems(page)).toEqual([]);
  });

  test(`${name}: axe finds no serious or critical violation`, async ({ page }) => {
    await inHighContrast(page, open);
    expect(await serious(page)).toEqual([]);
  });
}
