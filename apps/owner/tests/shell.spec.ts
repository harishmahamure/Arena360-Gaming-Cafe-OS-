import { expect, test } from "@playwright/test";
import { FREE, PAID, cacheCount, stubCloud, t } from "./stub";

const nav = (page: import("@playwright/test").Page) =>
  page.getByRole("navigation", { name: t("owner.nav.label") });

test("five sections, Today is home and current", async ({ page }) => {
  await stubCloud(page);
  await page.goto("/");
  const links = nav(page).getByRole("link");
  await expect(links).toHaveText(
    ["today", "reports", "alerts", "manage", "account"].map((k) =>
      t(`owner.nav.${k}` as "owner.nav.today"),
    ),
  );
  await expect(nav(page).getByRole("link", { name: t("owner.nav.today") })).toHaveAttribute(
    "aria-current",
    "page",
  );
  await expect(page.getByRole("heading", { level: 1, name: t("owner.nav.today") })).toBeVisible();
  await nav(page)
    .getByRole("link", { name: t("owner.nav.reports") })
    .click();
  await expect(page).toHaveURL(/\/reports$/);
  await expect(nav(page).getByRole("link", { name: t("owner.nav.reports") })).toHaveAttribute(
    "aria-current",
    "page",
  );
});

test("phone gets a 64 px bottom bar, desktop a 256 px sidebar", async ({ page }, info) => {
  await stubCloud(page);
  await page.goto("/");
  const box = (await nav(page).boundingBox())!;
  const view = page.viewportSize()!;
  if (info.project.name === "phone") {
    expect(box.height).toBe(64);
    expect(Math.round(box.y + box.height)).toBe(view.height);
    expect((await page.locator("h1").boundingBox())!.x).toBe(16);
  } else {
    expect(box.width).toBe(256);
    expect(box.x).toBe(0);
    const main = (await page.locator("main").boundingBox())!;
    expect(main.width).toBeLessThanOrEqual(1200);
    expect(main.x).toBe(256);
  }
});

test("Account holds MFA, Subscription, Branches, Support and Theme", async ({ page }) => {
  await stubCloud(page);
  await page.goto("/account");
  await expect(page.getByRole("link", { name: t("owner.account.mfa") })).toHaveAttribute(
    "href",
    /\/account\/security$/,
  );
  for (const k of ["subscription", "branches", "support"] as const)
    await expect(page.getByRole("link", { name: t(`owner.account.${k}`) })).toBeVisible();
  await expect(page.getByRole("tab", { name: t("ui.theme.dark") })).toBeVisible();
});

test("Subscription is read-only with each branch's tier and flags", async ({ page }) => {
  await stubCloud(page);
  await page.goto("/account/subscription");
  await expect(page.getByText(t("owner.subscription.soon"))).toBeVisible();
  const first = page.getByRole("listitem").filter({ hasText: PAID[0].name });
  await expect(first).toContainText(t("owner.tier.basic"));
  await expect(first).toContainText("tunnel");
  await expect(page.getByRole("listitem").filter({ hasText: PAID[1].name })).toContainText(
    t("owner.tier.free"),
  );
  await expect(page.getByRole("button")).toHaveCount(0);
  await expect(page.getByRole("link", { name: /upgrade/i })).toHaveCount(0);
});

test("Support shows a mailto link and calls no API", async ({ page }) => {
  const calls = await stubCloud(page);
  await page.goto("/account/support");
  await expect(page.getByRole("link", { name: /@/ })).toHaveAttribute("href", /^mailto:.+@.+/);
  await expect(page.getByText(t("owner.support.help"))).toBeVisible();
  await page.waitForLoadState("networkidle");
  expect(calls.filter((c) => !c.endsWith("/v1/session") && !c.endsWith("/v1/branches"))).toEqual(
    [],
  );
});

test("on Free the four data sections show only the tier-lock, with no Upgrade link", async ({
  page,
}) => {
  const calls = await stubCloud(page, FREE);
  for (const k of ["today", "reports", "alerts", "manage"] as const) {
    await page.goto(k === "today" ? "/" : `/${k}`);
    await expect(page.getByText(t("staff.tier.available_on", { tier: "Basic" }))).toBeVisible();
    await expect(page.getByRole("link", { name: /upgrade/i })).toHaveCount(0);
    await expect(page.getByText(t("owner.section.soon"))).toHaveCount(0);
  }
  await page.goto("/account/subscription");
  await expect(page.getByText(FREE[0].name)).toBeVisible();
  expect([...new Set(calls)].sort()).toEqual(["GET /v1/branches", "GET /v1/session"]);
});

test("with only paid branches the sections are not locked", async ({ page }) => {
  await stubCloud(page, [{ ...PAID[0] }]);
  await page.goto("/alerts");
  await expect(page.getByText(t("owner.section.soon"))).toBeVisible();
  await expect(page.getByText(t("staff.tier.available_on", { tier: "Basic" }))).toHaveCount(0);
});

test("a mixed tenant locks the Free branch and leaves the paid branch working", async ({
  page,
}) => {
  const calls = await stubCloud(page);
  for (const k of ["today", "alerts", "manage"] as const) {
    await page.goto(k === "today" ? "/" : `/${k}`);
    const paid = page.getByRole("listitem").filter({ hasText: PAID[0].name });
    const free = page.getByRole("listitem").filter({ hasText: PAID[1].name });
    // Today shows the paid branch's figures; Manage shows its prices and one command list (the
    // paid branch has no lock or gate entry of its own); the other sections still hold their placeholder.
    if (k === "manage") {
      await expect(page.getByRole("heading", { name: t("owner.cmd.list.title") })).toBeVisible();
      await expect(page.getByRole("tablist", { name: t("owner.pricing.tabs") })).toBeVisible();
    } else {
      await expect(
        paid.getByText(k === "today" ? t("owner.today.revenue") : t("owner.section.soon")),
      ).toBeVisible();
    }
    await expect(paid.getByText(t("staff.tier.available_on", { tier: "Basic" }))).toHaveCount(0);
    await expect(free.getByText(t("staff.tier.available_on", { tier: "Basic" }))).toBeVisible();
    await expect(free.getByText(t("owner.section.soon"))).toHaveCount(0);
    await expect(free.getByText(t("owner.today.revenue"))).toHaveCount(0);
    await expect(page.getByRole("link", { name: /upgrade/i })).toHaveCount(0);
  }
  // Reports picks from the paid branches and locks the Free one beside it.
  await page.goto("/reports");
  await expect(page.getByLabel(t("owner.reports.branch"))).toContainText(PAID[0].name);
  await expect(page.getByLabel(t("owner.reports.branch"))).not.toContainText(PAID[1].name);
  await expect(
    page
      .getByRole("region", { name: PAID[1].name })
      .getByText(t("staff.tier.available_on", { tier: "Basic" })),
  ).toBeVisible();
  // Only the paid branch is ever read: the Free branch makes no owner read call.
  await expect.poll(() => calls.some((c) => c.endsWith("/b1/data-age"))).toBe(true);
  await expect.poll(() => calls.some((c) => c.endsWith("/b1/summary"))).toBe(true);
  expect([...new Set(calls)].sort()).toEqual([
    "GET /v1/branches",
    "GET /v1/branches/b1/live",
    "GET /v1/branches/b1/summary",
    "GET /v1/commands",
    "GET /v1/session",
    "POST /v1/branches/b1/data-age",
  ]);
});

test("on Free, pairing and Support stay usable", async ({ page }) => {
  await stubCloud(page, FREE);
  await page.goto("/account/branches");
  await expect(page.getByText(FREE[0].name)).toBeVisible();
  await page.goto("/account/support");
  await expect(page.getByRole("link", { name: /@/ })).toBeVisible();
});

test("a Free branch that upgrades unlocks on the next open without signing out", async ({
  page,
}) => {
  await stubCloud(page, FREE);
  await page.goto("/alerts");
  await expect(page.getByText(t("staff.tier.available_on", { tier: "Basic" }))).toBeVisible();
  await page.unroute("**/v1/**");
  await stubCloud(page, [{ ...FREE[0], tier: "basic" }]);
  await page.reload();
  await expect(page.getByText(t("owner.section.soon"))).toBeVisible();
  await expect(page.getByText(t("staff.tier.available_on", { tier: "Basic" }))).toHaveCount(0);
  await page.goto("/");
  await expect(page.getByText(t("owner.today.revenue"))).toBeVisible();
});

test("a refused session goes to sign-in and clears the cache", async ({ page }) => {
  await stubCloud(page);
  await page.goto("/account/subscription");
  await expect(page.getByText(PAID[0].name)).toBeVisible();
  expect(await cacheCount(page)).toBeGreaterThan(0);
  await page.route("**/v1/**", (route) =>
    route.fulfill({ status: 401, contentType: "application/json", body: "{}" }),
  );
  await page.route("**/sign-in", (route) => route.fulfill({ body: "sign in" }));
  await page.reload();
  await page.waitForURL(/\/sign-in$/);
  await page.goto("/manifest.webmanifest");
  expect(await cacheCount(page)).toBe(0);
});

test("a 401 refreshes the session once and carries on", async ({ page }) => {
  await stubCloud(page);
  let rotated = 0;
  await page.route("**/v1/**", (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/v1/session/refresh") {
      rotated++;
      return route.fulfill({ status: 200, contentType: "application/json", body: "{}" });
    }
    if (!rotated)
      return route.fulfill({ status: 401, contentType: "application/json", body: "{}" });
    return route.fallback();
  });
  await page.goto("/account/subscription");
  await expect(page.getByText(PAID[0].name)).toBeVisible();
  expect(rotated).toBe(1);
});

test("Sign out clears the cache and signs out even if the call fails", async ({ page }) => {
  await stubCloud(page);
  await page.route("**/v1/signout", (route) => route.abort());
  await page.route("**/sign-in", (route) => route.fulfill({ body: "sign in" }));
  await page.goto("/account");
  await expect.poll(() => cacheCount(page)).toBeGreaterThan(0);
  await page.getByRole("button", { name: t("owner.account.sign_out") }).click();
  await page.waitForURL(/\/sign-in$/);
  await page.goto("/manifest.webmanifest");
  expect(await cacheCount(page)).toBe(0);
});

test("Sign out calls the API", async ({ page }) => {
  const calls = await stubCloud(page);
  await page.route("**/sign-in", (route) => route.fulfill({ body: "sign in" }));
  await page.goto("/account");
  await page.getByRole("button", { name: t("owner.account.sign_out") }).click();
  await page.waitForURL(/\/sign-in$/);
  expect(calls).toContain("POST /v1/signout");
});
