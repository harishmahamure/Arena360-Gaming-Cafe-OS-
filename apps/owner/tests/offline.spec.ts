import { expect, test } from "@playwright/test";
import { PAID, stubCloud, t } from "./stub";

test("the manifest and service worker make it installable", async ({ page, request }) => {
  await stubCloud(page);
  await page.goto("/");
  await expect(page.locator('link[rel="manifest"]')).toHaveAttribute(
    "href",
    "/manifest.webmanifest",
  );
  const manifest = await (await request.get("/manifest.webmanifest")).json();
  expect(manifest.display).toBe("standalone");
  expect(manifest.start_url).toBe("/");
  expect(manifest.icons.length).toBeGreaterThan(0);
  await page.evaluate(() => navigator.serviceWorker.ready);
});

test("offline: the shell opens from the cache with cached data, its age and an offline badge", async ({
  page,
  context,
}) => {
  await stubCloud(page);
  await page.goto("/account/subscription");
  await expect(page.getByRole("heading", { level: 2, name: PAID[0].name })).toBeVisible();
  await expect(page.getByText(t("owner.offline"))).toHaveCount(0);
  await page.evaluate(() => navigator.serviceWorker.ready);
  await page.reload();
  await expect.poll(() => page.evaluate(() => !!navigator.serviceWorker.controller)).toBe(true);

  await context.setOffline(true);
  await page.route("**/v1/**", (route) => route.abort());
  await page.reload();
  await expect(page.getByRole("heading", { level: 2, name: PAID[0].name })).toBeVisible();
  await expect(page.getByText(t("owner.offline"), { exact: true })).toBeVisible();
  await expect(page.getByText(/Updated /)).toBeVisible();
  await expect(page.getByRole("navigation")).toBeVisible();
  await expect(page.locator("h1")).toHaveCount(1);
});

test("offline with nothing cached is a message, not a blank page", async ({ page }) => {
  await page.route("**/v1/**", (route) => route.abort());
  await page.goto("/account/subscription");
  await expect(page.getByText(t("owner.unavailable"))).toBeVisible();
  await expect(page.getByText(t("owner.offline"), { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: t("common.retry") })).toBeVisible();
});

test("Today with nothing cached offers Retry, and Retry recovers", async ({ page }) => {
  let down = true;
  await stubCloud(page);
  await page.route("**/v1/**", (route) => (down ? route.abort() : route.fallback()));
  await page.goto("/");
  await expect(page.getByText(t("owner.unavailable"))).toBeVisible();
  down = false;
  await page.getByRole("button", { name: t("common.retry") }).click();
  await expect(page.getByText(t("owner.today.revenue"))).toBeVisible();
  await expect(page.getByText(t("owner.offline"), { exact: true })).toHaveCount(0);
});
