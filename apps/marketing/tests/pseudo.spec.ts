import { expect, test } from "@playwright/test";
import { PHONE, THEMES, open } from "./helpers";

// The pseudo-locale build (every string accented and 30% longer): nothing scrolls sideways at 390 px.

for (const path of ["/", "/pricing", "/product", "/downloads", "/help", "/no-such-page"]) {
  for (const theme of THEMES) {
    test(`${path} in ${theme} has no horizontal scroll with strings 30% longer`, async ({
      page,
    }) => {
      await page.setViewportSize(PHONE);
      await open(page, path, theme);
      await expect(page.getByRole("heading", { level: 1 })).toContainText("[");
      const overflow = await page.evaluate(
        () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
      );
      expect(overflow).toBeLessThanOrEqual(0);
    });
  }
}
