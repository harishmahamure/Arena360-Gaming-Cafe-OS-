import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import { stubCloud, t, toCodeStep } from "./stub";

// Accessibility floor for every onboarding view in light and dark: landmarks, one h1, labelled
// fields, targets of at least 44 × 44 px and no serious or critical axe violations.

const THEMES = ["light", "dark"] as const;
const INTERACTIVE = 'button, a, input, select, [role="tab"], [tabindex]:not([tabindex="-1"])';

const VIEWS: { name: string; open: (page: Page) => Promise<void> }[] = [
  {
    name: "get started",
    open: async (page) => {
      await page.goto("/");
      await page.getByRole("button", { name: t("onboarding.start.send") }).click();
    },
  },
  { name: "code step", open: (page) => toCodeStep(page) },
  {
    name: "account setup",
    open: async (page) => {
      await toCodeStep(page);
      await page.keyboard.insertText("123456");
      await expect(page.getByText(t("onboarding.account_setup.body"))).toBeVisible();
    },
  },
];

for (const theme of THEMES) {
  for (const view of VIEWS) {
    test(`${view.name} (${theme}): landmarks, one h1, labels, targets and axe`, async ({
      page,
    }) => {
      await stubCloud(page);
      await page.addInitScript((v) => localStorage.setItem("arena.theme", v), theme);
      await view.open(page);
      await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
      await page.evaluate(() => document.fonts.ready);

      await expect(page.locator("header")).toHaveCount(1);
      await expect(page.locator("main")).toHaveCount(1);
      await expect(page.locator("footer")).toHaveCount(1);
      await expect(page.locator("h1")).toHaveCount(1);

      const unlabelled = await page.evaluate(() =>
        [...document.querySelectorAll("input, select, textarea")]
          .filter((el) => {
            const id = el.id;
            const label = id ? document.querySelector(`label[for="${id}"]`) : null;
            return !label?.textContent?.trim() && !el.getAttribute("aria-label");
          })
          .map((el) => el.outerHTML.slice(0, 80)),
      );
      expect(unlabelled, unlabelled.join("\n")).toEqual([]);

      const controls = page.locator(INTERACTIVE);
      const small: string[] = [];
      for (let i = 0; i < (await controls.count()); i++) {
        const box = await controls.nth(i).boundingBox();
        if (!box) continue;
        if (box.width < 44 || box.height < 44) {
          const what = await controls.nth(i).evaluate((el) => el.outerHTML.slice(0, 80));
          small.push(`${what} is ${box.width.toFixed(1)}×${box.height.toFixed(1)}`);
        }
      }
      expect(small, small.join("\n")).toEqual([]);

      const overflow = await page.evaluate(
        () => document.documentElement.scrollWidth > document.documentElement.clientWidth,
      );
      expect(overflow, "no horizontal scroll").toBe(false);

      const results = await new AxeBuilder({ page }).analyze();
      const blocking = results.violations.filter(
        (v) => v.impact === "serious" || v.impact === "critical",
      );
      const report = blocking.map(
        (v) => `${v.id} (${v.impact}): ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`,
      );
      expect(report, report.join("\n")).toEqual([]);
    });
  }
}
