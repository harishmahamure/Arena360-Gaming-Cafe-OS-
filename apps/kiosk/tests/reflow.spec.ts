import { expect, test, type Page } from "@playwright/test";
import { WIDTHS, layoutProblems, openShell, pseudoLocale } from "./support";
import { SCREENS } from "./screens";

// Story 6.37: the kiosk reflows at every supported width and is never scaled. Each player screen
// is opened at 1366, 1920, 2560 and a 21:9 ultrawide, plain and with a pseudo-locale that makes
// every text 30% longer; nothing may scroll sideways, leave the screen or sit on another control.
// The Tauri shell is stubbed in the page (see `support.ts`).

for (const size of WIDTHS) {
  test.describe(`at ${size.name}`, () => {
    test.use({ viewport: { width: size.width, height: size.height } });

    for (const [name, open] of Object.entries(SCREENS)) {
      test(`${name} fits`, async ({ page }) => {
        await open(page);
        expect(await layoutProblems(page)).toEqual([]);
      });

      test(`${name} fits with 30% longer text`, async ({ page }) => {
        await open(page);
        await pseudoLocale(page);
        expect(await layoutProblems(page)).toEqual([]);
      });
    }
  });
}

// ---- The grid rules (UX-DR86): tiles per row follow the width; the layout is not scaled. ----

const columns = (page: Page, testId: string) =>
  page.getByTestId(testId).evaluate((el) => {
    const tops = [...el.children].map((c) => Math.round(c.getBoundingClientRect().top));
    return tops.filter((t) => t === tops[0]).length;
  });

const featuredTiles = (page: Page) =>
  page
    .getByTestId("featured-row")
    .locator("li:not(.all)")
    .evaluateAll((lis) => lis.filter((li) => getComputedStyle(li).display !== "none").length);

const LAYOUT = [
  { size: WIDTHS[0], tiles: 4, columns: 5, hero: 720 },
  { size: WIDTHS[1], tiles: 6, columns: 7, hero: 864 },
  { size: WIDTHS[2], tiles: 8, columns: 9, hero: 864 },
  // On an ultrawide the copy is bounded by the left 45% instead (the next test).
  { size: WIDTHS[3], tiles: 8, columns: 9, hero: null },
];

for (const { size, tiles, columns: cols, hero } of LAYOUT) {
  test(`at ${size.name} Home shows ${tiles} tiles and All games, Games ${cols} columns`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: size.width, height: size.height });
    await openShell(page);
    await expect(page.getByTestId("featured-row")).toBeVisible();
    expect(await featuredTiles(page)).toBe(tiles);
    await expect(page.getByTestId("all-games-tile")).toBeVisible();
    const copy = (await page.getByTestId("hero-copy").boundingBox())!;
    if (hero !== null) expect(copy.width).toBeLessThanOrEqual(hero + 1);
    // The 56px margin: the shell's content starts that far in (or further, centred on 21:9).
    const bar = (await page.locator(".bar").boundingBox())!;
    const expectedLeft = Math.max(56, (size.width - 2560) / 2 + 56);
    expect(Math.round(bar.x)).toBe(Math.round(expectedLeft));
    await page.getByTestId("tab-games").click();
    await expect(page.getByTestId("games-grid")).toBeVisible();
    expect(await columns(page, "games-grid")).toBe(cols);
  });
}

test("on a 21:9 ultrawide the content is capped at 2560px and the hero copy stays in its left 45%", async ({
  page,
}) => {
  const size = WIDTHS[3];
  await page.setViewportSize({ width: size.width, height: size.height });
  await openShell(page);
  const shell = (await page.getByTestId("shell").boundingBox())!;
  expect(Math.round(shell.width)).toBe(2560);
  expect(Math.round(shell.x)).toBe((size.width - 2560) / 2);
  const copy = (await page.getByTestId("hero-copy").boundingBox())!;
  const title = (await page.getByTestId("hero-title").boundingBox())!;
  const left = (size.width - 2560) / 2;
  expect(copy.x + copy.width).toBeLessThanOrEqual(left + 2560 * 0.45 + 1);
  expect(title.x).toBeGreaterThanOrEqual(left + 56 - 1);
  expect(title.x + title.width).toBeLessThanOrEqual(left + 2560 * 0.45 + 1);
});

test("a screen is reflowed, never scaled: no transform or zoom on the page", async ({ page }) => {
  await page.setViewportSize({ width: 2560, height: 1440 });
  await openShell(page);
  const scaled = await page.evaluate(() =>
    [document.documentElement, document.body, document.querySelector(".kiosk")!].some((el) => {
      const s = getComputedStyle(el);
      return s.transform !== "none" || (s as CSSStyleDeclaration & { zoom: string }).zoom !== "1";
    }),
  );
  expect(scaled).toBe(false);
});
