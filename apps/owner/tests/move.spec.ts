import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";
import { PAID, stubCloud, t, type Branch } from "./stub";

const android = (over: Partial<Branch> = {}): Branch => ({
  ...PAID[0],
  tier: "basic",
  flags: ["owner_app"],
  hub: { paired: true, platform: "android", last_seen_at: Date.now() - 60_000 },
  ...over,
});
const card = (page: import("@playwright/test").Page) =>
  page.getByRole("listitem").filter({ hasText: PAID[0].name }).first();
const move = (page: import("@playwright/test").Page) => card(page).getByTestId("move");

test("a paid branch on an Android Hub explains the move and shows the code card", async ({
  page,
}) => {
  const calls = await stubCloud(page, [android()]);
  await page.goto("/account/branches");
  await expect(move(page).getByText(t("owner.branches.move.step1"))).toBeVisible();
  await expect(move(page).getByText(t("owner.branches.move.step2"))).toBeVisible();
  await expect(move(page).getByText(t("owner.branches.move.step3"))).toBeVisible();
  await expect(move(page).getByText(t("owner.branches.move.sessions"))).toBeVisible();
  await move(page)
    .getByRole("button", { name: t("owner.branches.move.start") })
    .click();
  await expect(page.getByTestId("pairing-code-value")).toContainText("MOVE");
  expect(calls.bodies["move"]).toEqual(["POST"]);
  // The Hub's own code was not asked for.
  expect(calls.filter((c) => c === "POST /v1/wizard/install-hub/code")).toHaveLength(0);
  const results = await new AxeBuilder({ page }).analyze();
  expect(results.violations).toEqual([]);
});

test("on Free the move is a tier-lock and offers no button", async ({ page }) => {
  await stubCloud(page, [android({ tier: "free", flags: [] })]);
  await page.goto("/account/branches");
  await expect(move(page).getByText(t("staff.tier.available_on", { tier: "Basic" }))).toBeVisible();
  await expect(move(page).getByRole("button")).toHaveCount(0);
});

test("a Windows Hub branch has no move", async ({ page }) => {
  await stubCloud(page, [android({ hub: { paired: true, platform: "windows" } })]);
  await page.goto("/account/branches");
  await expect(card(page).getByText(PAID[0].name)).toBeVisible();
  await expect(move(page)).toHaveCount(0);
});

test("a waiting code offers a new one and the move can be cancelled", async ({ page }) => {
  const calls = await stubCloud(page, [
    android({
      migration: { phase: "awaiting_windows", code: { expires_at: Date.now() + 600_000 } },
    }),
  ]);
  await page.goto("/account/branches");
  await expect(move(page).getByTestId("move-waiting")).toBeVisible();
  await expect(
    move(page).getByRole("button", { name: t("owner.branches.move.start") }),
  ).toHaveCount(0);
  await move(page)
    .getByRole("button", { name: t("owner.branches.move.new") })
    .click();
  await expect(page.getByTestId("pairing-code-value")).toContainText("MOVE");
  await move(page)
    .getByRole("button", { name: t("owner.branches.move.cancel") })
    .click();
  await expect(page.getByTestId("pairing-code-value")).toHaveCount(0);
  expect(calls.bodies["move"]).toEqual(["POST", "DELETE"]);
});

test("once the PC has paired the card names it and only offers Cancel move", async ({ page }) => {
  await stubCloud(page, [
    android({ migration: { phase: "awaiting_handover", target_hub_name: "Front desk" } }),
  ]);
  await page.goto("/account/branches");
  await expect(move(page).getByTestId("move-paired")).toHaveText(
    t("owner.branches.move.paired", { pc: "Front desk" }),
  );
  await expect(move(page).getByRole("button")).toHaveCount(1);
  await expect(
    move(page).getByRole("button", { name: t("owner.branches.move.cancel") }),
  ).toBeVisible();
});

test("a refusal from the cloud is shown under the button", async ({ page }) => {
  await stubCloud(page, [android()], {
    move: () => ({ status: 409, body: { code: "HUB_MIGRATION_ACTIVE", message: "" } }),
  });
  await page.goto("/account/branches");
  await move(page)
    .getByRole("button", { name: t("owner.branches.move.start") })
    .click();
  await expect(move(page).getByRole("alert")).toHaveText(t("owner.branches.move.active"));
  await expect(page.getByTestId("pairing-code-value")).toHaveCount(0);
});
