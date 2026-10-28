import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };

// With no session the kiosk shows the attract screen (Story 6.7), not the Epic 1 locked
// placeholder. Its content and behaviour are in `attract.spec.ts`; this is the plain-browser
// case, where the shell has told the page nothing yet.

test("with no session the attract screen replaces the locked placeholder", async ({ page }) => {
  await page.goto("/");
  const attract = page.getByTestId("attract");
  await expect(attract).toBeVisible();
  await expect(page.getByTestId("locked")).toHaveCount(0);
  await expect(attract.getByRole("heading", { level: 1 })).toHaveText(en["app.name"]);
  await expect(attract).toContainText(en["player.attract.ask_counter"]);
  await expect(attract).not.toContainText(en["player.locked.ready"]);
  await expect(page.getByTestId("logo-slot")).toBeVisible();
  const results = await new AxeBuilder({ page }).analyze();
  expect(
    results.violations.filter((v) => v.impact === "serious" || v.impact === "critical"),
  ).toEqual([]);
});
