import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };

// With no session the kiosk shows the locked placeholder (Story 1.17), from the catalogue.

test("with no session the locked placeholder is shown", async ({ page }) => {
  await page.goto("/");
  const locked = page.getByTestId("locked");
  await expect(locked.getByRole("heading", { level: 1 })).toHaveText(en["player.locked.ready"]);
  await expect(locked).toContainText(en["player.locked.hint"]);
  expect(en["player.locked.ready"]).toBe(
    "This PC is ready. Log in at the counter or with your account.",
  );
  const results = await new AxeBuilder({ page }).analyze();
  expect(
    results.violations.filter((v) => v.impact === "serious" || v.impact === "critical"),
  ).toEqual([]);
});
