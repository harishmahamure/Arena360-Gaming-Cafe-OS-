import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";
import { FREE, PAID, inSetup, stubCloud, t } from "./stub";

const ROUTES = [
  "/",
  "/reports",
  "/alerts",
  "/manage",
  "/account",
  "/account/subscription",
  "/account/branches",
  "/account/support",
];
const INTERACTIVE = 'button, a, input, [role="tab"]';

for (const theme of ["light", "dark", "hc"] as const) {
  for (const [name, branches] of [
    ["paid", PAID],
    ["free", FREE],
    ["live and setup", [PAID[0], inSetup({ ...PAID[1], tier: "basic" })]],
    ["all in setup", [inSetup(PAID[0]), inSetup({ ...PAID[1], tier: "basic" })]],
  ] as const) {
    for (const route of ROUTES) {
      test(`${route} (${theme}, ${name}): landmarks, one h1, targets and axe`, async ({ page }) => {
        await stubCloud(page, branches);
        await page.addInitScript((v) => localStorage.setItem("arena.theme", v), theme);
        await page.goto(route);
        await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
        await expect(page.getByText(t("owner.loading"))).toHaveCount(0);
        await page.evaluate(() => document.fonts.ready);

        await expect(page.locator("header")).toHaveCount(1);
        await expect(page.locator("nav")).toHaveCount(1);
        await expect(page.locator("main")).toHaveCount(1);
        await expect(page.locator("h1")).toHaveCount(1);

        const small = await page.locator(INTERACTIVE).evaluateAll((els) =>
          els
            .map((el) => [el.outerHTML.slice(0, 70), el.getBoundingClientRect()] as const)
            .filter(([, r]) => r.width > 0 && (r.width < 44 || r.height < 44))
            .map(([h, r]) => `${h} ${r.width}x${r.height}`),
        );
        expect(small, small.join("\n")).toEqual([]);
        expect(
          await page.evaluate(
            () => document.documentElement.scrollWidth > document.documentElement.clientWidth,
          ),
        ).toBe(false);

        const blocking = (await new AxeBuilder({ page }).analyze()).violations.filter(
          (v) => v.impact === "serious" || v.impact === "critical",
        );
        const report = blocking.map(
          (v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`,
        );
        expect(report, report.join("\n")).toEqual([]);
      });
    }
  }
}
