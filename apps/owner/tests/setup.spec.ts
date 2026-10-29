import { expect, test, type Page } from "@playwright/test";
import { FREE, PAID, inSetup, stubCloud, t } from "./stub";

const MIN = 60_000;
const LIVE_CALLS = /\/(live|data-age|summary)$/;
const NEXT = t("ui.wizard.step.devices");

const setupCard = (page: Page, name: string) =>
  page.getByRole("region", { name: new RegExp(name) });

test("a tenant whose branches are all in setup sees one setup card per branch and no figures", async ({
  page,
}) => {
  const calls = await stubCloud(page, [
    inSetup(PAID[0]),
    inSetup({ ...PAID[1], tier: "basic" }, 7, "products"),
  ]);
  await page.goto("/");
  const first = setupCard(page, PAID[0].name);
  await expect(
    first.getByRole("heading", {
      name: t("owner.setup.title", { branch: PAID[0].name, done: 4, total: 10 }),
    }),
  ).toBeVisible();
  await expect(first.getByText(t("owner.setup.next", { step: NEXT }))).toBeVisible();
  await expect(first.getByRole("button", { name: t("owner.branches.continue") })).toBeVisible();
  await expect(
    setupCard(page, PAID[1].name).getByText(
      t("owner.setup.next", { step: t("ui.wizard.step.products") }),
    ),
  ).toBeVisible();
  await expect(page.getByText(t("owner.today.attention"))).toHaveCount(0);
  await expect(page.getByRole("button", { name: t("owner.today.refresh") })).toHaveCount(0);
  await page.waitForTimeout(300);
  expect(calls.filter((c) => LIVE_CALLS.test(c))).toEqual([]);
});

test("Continue setup opens the wizard at the branch", async ({ page }) => {
  await stubCloud(page, [inSetup(PAID[0])]);
  let opened = "";
  await page.route("https://start.arena360.cloud/**", (route) => {
    opened = route.request().url();
    return route.fulfill({ status: 200, contentType: "text/html", body: "<title>wizard</title>" });
  });
  await page.goto("/");
  await page.getByRole("button", { name: t("owner.branches.continue") }).click();
  await expect.poll(() => opened).toBe("https://start.arena360.cloud/setup?branch=b1");
});

test("a live branch and a branch in setup share the list", async ({ page }) => {
  const calls = await stubCloud(page, [PAID[0], inSetup({ ...PAID[1], tier: "basic" })]);
  await page.goto("/");
  await expect(page.getByText("₹1,25,000.50")).toBeVisible();
  await expect(setupCard(page, PAID[1].name)).toBeVisible();
  await expect(page.getByRole("listitem").filter({ hasText: PAID[0].name }).first()).toBeVisible();
  expect(calls.filter((c) => LIVE_CALLS.test(c)).every((c) => c.includes("/b1/"))).toBe(true);
});

test("a branch that goes live switches to its dashboard card at the next poll", async ({
  page,
}) => {
  let live = false;
  const calls = await stubCloud(page, () => [live ? PAID[0] : inSetup(PAID[0])], {
    live: { b1: () => ({ ...liveWaiting() }) },
  });
  await page.clock.install();
  await page.goto("/");
  await expect(setupCard(page, PAID[0].name)).toContainText(NEXT);
  const branchReads = () => calls.filter((c) => c === "GET /v1/branches").length;
  await expect.poll(branchReads).toBe(1);

  live = true;
  await page.clock.fastForward(MIN + 1000);
  await expect(page.getByText(t("owner.today.waiting"))).toBeVisible();
  await expect(page.getByRole("button", { name: t("owner.branches.continue") })).toHaveCount(0);
  expect(branchReads()).toBe(2);
  expect(calls).toContain("GET /v1/branches/b1/live");
});

function liveWaiting() {
  return {
    branch_id: "b1",
    currency: "INR",
    as_of: null,
    branch_online: false,
    last_heartbeat_at: null,
    summary: null,
  };
}

test("the Refresh button picks up a go-live", async ({ page }) => {
  let live = false;
  await stubCloud(page, () => [live ? PAID[0] : inSetup(PAID[0]), { ...PAID[1], tier: "basic" }], {
    live: { b1: () => ({ ...liveWaiting() }) },
  });
  await page.goto("/");
  await expect(setupCard(page, PAID[0].name)).toBeVisible();
  live = true;
  await page.getByRole("button", { name: t("owner.today.refresh") }).click();
  await expect(page.getByText(t("owner.today.waiting"))).toBeVisible();
});

test("a Free branch shows setup first, then the Free limits once live", async ({ page }) => {
  let live = false;
  const calls = await stubCloud(page, () => [live ? FREE[0] : inSetup(FREE[0])]);
  await page.clock.install();
  await page.goto("/");
  await expect(setupCard(page, FREE[0].name)).toBeVisible();
  await expect(page.getByText(t("staff.tier.available_on", { tier: "Basic" }))).toHaveCount(0);
  live = true;
  await page.clock.fastForward(MIN + 1000);
  await expect(page.getByText(t("staff.tier.available_on", { tier: "Basic" }))).toBeVisible();
  await expect(page.getByRole("button", { name: t("owner.branches.continue") })).toHaveCount(0);
  expect(calls.filter((c) => LIVE_CALLS.test(c))).toEqual([]);
});

test("branch-scoped screens say a branch in setup is available after it goes live", async ({
  page,
}) => {
  await stubCloud(page, [PAID[0], inSetup({ ...PAID[1], tier: "basic" })]);
  for (const path of ["/reports", "/manage", "/alerts"]) {
    await page.goto(path);
    const gated = page.getByRole("listitem").filter({ hasText: PAID[1].name });
    await expect(
      gated.getByText(t("owner.setup.after_live", { branch: PAID[1].name })),
    ).toBeVisible();
    await expect(gated.getByRole("link", { name: t("owner.branches.continue") })).toHaveAttribute(
      "href",
      "https://start.arena360.cloud/setup?branch=b2",
    );
    const open = page.getByRole("listitem").filter({ hasText: PAID[0].name });
    await expect(open.getByText(t("owner.section.soon"))).toBeVisible();
  }
});
