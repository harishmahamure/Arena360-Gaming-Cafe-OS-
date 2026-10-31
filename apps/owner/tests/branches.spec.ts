import { expect, test } from "@playwright/test";
import { FREE, PAID, inSetup, stubCloud, t } from "./stub";

test("each branch shows name, code, plan, setup state and Hub status", async ({ page }) => {
  const live = [
    {
      ...PAID[0],
      live: true,
      hub: { paired: true, platform: "android" as const, last_seen_at: Date.now() - 180_000 },
    },
    inSetup(PAID[1]),
  ];
  await stubCloud(page, live);
  await page.goto("/account/branches");
  const first = page.getByRole("listitem").filter({ hasText: PAID[0].name });
  await expect(first).toContainText(t("owner.branches.code", { code: "MGR" }));
  await expect(first).toContainText(t("owner.branches.state.live"));
  await expect(first).toContainText(
    t("owner.branches.hub.seen", { age: t("owner.age.minutes", { n: 3 }) }),
  );
  await expect(first.getByTestId("hub-platform")).toHaveText(
    t("owner.branches.hub.platform", { platform: t("owner.branches.hub.platform.android") }),
  );
  const second = page.getByRole("listitem").filter({ hasText: PAID[1].name });
  await expect(second.getByTestId("hub-platform")).toHaveCount(0);
  await expect(second).toContainText(t("owner.branches.state.setup"));
  await expect(second).toContainText(t("owner.branches.hub.none"));
});

test("renaming sends the change and a waiting branch says so", async ({ page }) => {
  const calls = await stubCloud(page, [{ ...PAID[0], hub: { paired: true }, waiting: true }]);
  await page.goto("/account/branches");
  await expect(page.getByText(t("owner.branches.waiting"))).toBeVisible();
  await page.getByRole("button", { name: t("owner.branches.edit") }).click();
  await page.getByLabel(t("owner.branches.field.name"), { exact: true }).fill("Renamed");
  await page.getByLabel(t("owner.branches.field.timezone")).selectOption("Asia/Dubai");
  await expect(page.getByText(t("owner.branches.tz_note"))).toBeVisible();
  await page.getByRole("button", { name: t("owner.branches.save") }).click();
  await expect(page.getByRole("button", { name: t("owner.branches.edit") })).toBeVisible();
  expect(calls).toContain("PATCH /v1/branches/b1");
});

test("a branch with no Hub offers the pairing code card", async ({ page }) => {
  await stubCloud(page, PAID);
  await page.goto("/account/branches");
  await page
    .getByRole("listitem")
    .filter({ hasText: PAID[0].name })
    .getByRole("button", { name: t("owner.branches.pair") })
    .click();
  await expect(page.getByTestId("pairing-code-value")).toContainText("ABCD");
});

test("on Free, Add branch and Standby are locked; on paid, Add branch is a form", async ({
  page,
}) => {
  await stubCloud(page, FREE);
  await page.goto("/account/branches");
  // The branch card (Back up now) and Add branch lock on Basic; the card's Standby PC on Premium.
  await expect(page.getByText(t("staff.tier.available_on", { tier: "Basic" }))).toHaveCount(2);
  await expect(page.getByText(t("staff.tier.available_on", { tier: "Premium" }))).toHaveCount(1);
  await expect(page.getByRole("button", { name: t("owner.branches.add") })).toHaveCount(0);
  // Every lock links to the picker for the one Free branch; the Standby lock asks for Premium.
  const links = page.getByRole("link", { name: t("staff.tier.upgrade") });
  await expect(links).toHaveCount(3);
  await expect(links.first()).toHaveAttribute("href", /tier=basic&branch=b1&return=/);
  await expect(links.nth(1)).toHaveAttribute("href", /tier=premium&branch=b1&return=/);
  await page.unroute("**/v1/**");
  await stubCloud(page, PAID);
  await page.reload();
  await expect(page.getByRole("button", { name: t("owner.branches.add") })).toBeVisible();
  // Only the Free branch in the list keeps its lock (Back up now).
  await expect(page.getByText(t("staff.tier.available_on", { tier: "Basic" }))).toHaveCount(1);
  // Neither of these branches has the standby flag.
  await expect(page.getByText(t("staff.tier.available_on", { tier: "Premium" }))).toHaveCount(2);
});
