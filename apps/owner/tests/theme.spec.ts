import { expect, test } from "@playwright/test";
import { stubCloud, t } from "./stub";

const html = (page: import("@playwright/test").Page) => page.locator("html");
const tab = (
  page: import("@playwright/test").Page,
  key: "ui.theme.system" | "ui.theme.light" | "ui.theme.dark",
) => page.getByRole("tab", { name: t(key) });

test("System follows prefers-color-scheme live; a choice is remembered per device", async ({
  page,
}) => {
  await stubCloud(page);
  await page.emulateMedia({ colorScheme: "light" });
  await page.goto("/account");
  await expect(tab(page, "ui.theme.system")).toHaveAttribute("aria-selected", "true");
  await expect(html(page)).toHaveAttribute("data-theme", "light");
  await page.emulateMedia({ colorScheme: "dark" });
  await expect(html(page)).toHaveAttribute("data-theme", "dark");
  await tab(page, "ui.theme.light").click();
  await expect(html(page)).toHaveAttribute("data-theme", "light");
  await page.reload();
  await expect(html(page)).toHaveAttribute("data-theme", "light");
  await expect(tab(page, "ui.theme.light")).toHaveAttribute("aria-selected", "true");
});
