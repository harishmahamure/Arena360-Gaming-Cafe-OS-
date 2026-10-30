import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";
import { PAID, stubCloud, t, type Branch } from "./stub";

const premium = (over: Partial<Branch> = {}): Branch => ({
  ...PAID[0],
  tier: "premium",
  flags: ["standby", "tunnel"],
  hub: { paired: true, platform: "windows", last_seen_at: Date.now() - 60_000 },
  ...over,
});
const card = (page: import("@playwright/test").Page) =>
  page.getByRole("listitem").filter({ hasText: PAID[0].name }).first();
const standby = (page: import("@playwright/test").Page) => card(page).getByTestId("standby");

test("a Premium branch with a Windows Hub shows 'Add standby PC' and then the code card", async ({
  page,
}) => {
  const calls = await stubCloud(page, [premium()]);
  await page.goto("/account/branches");
  await expect(standby(page).getByText(t("owner.branches.standby"), { exact: true })).toBeVisible();
  await standby(page)
    .getByRole("button", { name: t("owner.branches.standby.add") })
    .click();
  await expect(page.getByTestId("pairing-code-value")).toContainText("WXYZ");
  await expect(standby(page).getByText(t("owner.branches.standby.note"))).toBeVisible();
  expect(calls.bodies["standby"]).toEqual([{ branch_id: "b1" }]);
  // The Hub's own code was not asked for.
  expect(calls.filter((c) => c === "POST /v1/wizard/install-hub/code")).toHaveLength(0);
  const results = await new AxeBuilder({ page }).analyze();
  expect(results.violations).toEqual([]);
});

test("a waiting standby code offers a new one", async ({ page }) => {
  await stubCloud(page, [
    premium({ standby: { paired: false, code: { expires_at: Date.now() + 600_000 } } }),
  ]);
  await page.goto("/account/branches");
  await expect(standby(page).getByTestId("standby-waiting")).toBeVisible();
  await expect(
    standby(page).getByRole("button", { name: t("owner.branches.standby.add") }),
  ).toHaveCount(0);
  await standby(page)
    .getByRole("button", { name: t("owner.branches.standby.new") })
    .click();
  await expect(page.getByTestId("pairing-code-value")).toContainText("WXYZ");
});

test("a paired standby shows its state and offers no second code", async ({ page }) => {
  await stubCloud(page, [
    premium({ standby: { paired: true, last_seen_at: Date.now() - 120_000 } }),
  ]);
  await page.goto("/account/branches");
  await expect(standby(page).getByTestId("standby-paired")).toBeVisible();
  await expect(standby(page).getByRole("button")).toHaveCount(0);
});

test("without the standby flag the card shows the Premium tier-lock", async ({ page }) => {
  await stubCloud(page, [premium({ tier: "basic", flags: ["tunnel"] })]);
  await page.goto("/account/branches");
  await expect(
    standby(page).getByText(t("staff.tier.available_on", { tier: "Premium" })),
  ).toBeVisible();
  await expect(standby(page).getByRole("button")).toHaveCount(0);
});

test("an Android Hub branch says it needs the Windows Hub instead of offering a code", async ({
  page,
}) => {
  await stubCloud(page, [premium({ hub: { paired: true, platform: "android" } })]);
  await page.goto("/account/branches");
  await expect(standby(page).getByText(t("owner.branches.standby.needs_windows"))).toBeVisible();
  await expect(standby(page).getByRole("button")).toHaveCount(0);
});

test("a branch whose main Hub is not paired yet asks for it first", async ({ page }) => {
  await stubCloud(page, [premium({ hub: { paired: false } })]);
  await page.goto("/account/branches");
  await expect(standby(page).getByText(t("owner.branches.standby.needs_hub"))).toBeVisible();
  await expect(standby(page).getByRole("button")).toHaveCount(0);
});

test("a refusal from the cloud is shown under the button", async ({ page }) => {
  await stubCloud(page, [premium()], {
    standby: () => ({ status: 409, body: { code: "STANDBY_EXISTS", message: "" } }),
  });
  await page.goto("/account/branches");
  await standby(page)
    .getByRole("button", { name: t("owner.branches.standby.add") })
    .click();
  await expect(standby(page).getByRole("alert")).toHaveText(t("owner.branches.standby.exists"));
  await expect(page.getByTestId("pairing-code-value")).toHaveCount(0);
});
