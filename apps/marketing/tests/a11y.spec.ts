import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";
import { DESKTOP, PHONE, THEMES, open } from "./helpers";

// Accessibility floor in every theme at 390 and 1440 px: zero serious or critical axe violations,
// targets of at least 44 px and no horizontal scroll. High contrast is also held to 7:1.

const PAGES = ["/", "/pricing", "/no-such-page"];
const VIEWPORTS = [
  { name: "390", size: PHONE },
  { name: "1440", size: DESKTOP },
];
const INTERACTIVE = 'button, a, input, select, [role="tab"], [role="switch"]';
const TAGS = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"];

for (const path of PAGES) {
  for (const theme of THEMES) {
    for (const viewport of VIEWPORTS) {
      test(`${path} in ${theme} at ${viewport.name} px`, async ({ page }) => {
        await page.setViewportSize(viewport.size);
        await open(page, path, theme);
        await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
        await expect(page.getByRole("tab").first()).toBeVisible();

        const results = await new AxeBuilder({ page }).withTags(TAGS).analyze();
        const blocking = results.violations.filter(
          (v) => v.impact === "serious" || v.impact === "critical",
        );
        const report = blocking.map(
          (v) => `${v.id} (${v.impact}): ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`,
        );
        expect(report, report.join("\n")).toEqual([]);

        if (theme === "hc") {
          const enhanced = await new AxeBuilder({ page })
            .options({ runOnly: ["color-contrast-enhanced"] })
            .analyze();
          const failing = enhanced.violations.map(
            (v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`,
          );
          expect(failing, failing.join("\n")).toEqual([]);
        }

        const small = await page.locator(INTERACTIVE).evaluateAll((els) =>
          els
            .filter((el) => !el.classList.contains("skip-link"))
            .map((el) => ({ el, box: el.getBoundingClientRect() }))
            .filter(({ box }) => box.width > 0 && (box.width < 44 || box.height < 44))
            .map(({ el }) => el.outerHTML.slice(0, 100)),
        );
        expect(small).toEqual([]);

        const overflow = await page.evaluate(
          () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
        );
        expect(overflow).toBeLessThanOrEqual(0);
      });
    }
  }
}
