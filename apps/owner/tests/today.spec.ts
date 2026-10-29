import { expect, test, type Page } from "@playwright/test";
import { PAID, liveOf, stubCloud, t } from "./stub";

const ONE = [PAID[0]];
const MIN = 60_000;

const card = (page: Page) => page.getByRole("region", { name: PAID[0].name });

test("each paid branch is a card of figures with its data age", async ({ page }) => {
  await stubCloud(page, ONE);
  await page.goto("/");
  const c = card(page);
  await expect(c.getByText(t("owner.today.open"), { exact: true })).toBeVisible();
  await expect(c.getByText("₹1,25,000.50")).toBeVisible();
  await expect(c.getByText("₹34,000.00")).toBeVisible();
  await expect(c.getByText("7", { exact: true })).toBeVisible();
  await expect(c.getByText("Meera, Dev")).toBeVisible();
  // Six figures carry "as of ~2 min ago"; the seventh is the last sync.
  await expect(c.getByText(/^as of [12] min ago$/)).toHaveCount(6);
  await expect(c.getByText(/^[01] min ago$|^just now$/)).toBeVisible();
  await expect(c.getByText(t("owner.today.offline"))).toHaveCount(0);
  await expect(page.getByText(t("owner.today.attention.none"))).toBeVisible();
});

test("the signed variance sits beside the cash, flagged only past the threshold", async ({
  page,
}) => {
  await stubCloud(page, ONE);
  await page.goto("/");
  const c = card(page);
  await expect(c.getByText(t("owner.today.variance.short", { amount: "−₹25.00" }))).toBeVisible();
  await expect(c.getByText(t("owner.today.variance.threshold"))).toHaveCount(0);

  await page.unroute("**/v1/**");
  await stubCloud(page, ONE, {
    live: {
      b1: liveOf(
        "b1",
        {},
        {
          shifts: [
            { shift_id: "old", staff_name: "A", opened_at: 1, closed_at: 2, variance_minor: 5 },
            {
              shift_id: "sh9",
              staff_name: "Ravi",
              opened_at: 3,
              closed_at: 9,
              variance_minor: 4000,
            },
          ],
          attention: [
            {
              kind: "shift_variance",
              ref: "sh9",
              text_params: { staff: "Ravi", variance_minor: "4000" },
            },
          ],
        },
      ),
    },
  });
  await page.reload();
  const over = card(page);
  await expect(over.getByText(t("owner.today.variance.over", { amount: "+₹40.00" }))).toBeVisible();
  await expect(over.getByText(t("owner.today.variance.threshold"))).toBeVisible();
});

test("a branch whose heartbeat is old is muted, badged offline and keeps its last values", async ({
  page,
}) => {
  await stubCloud(page, ONE, {
    live: { b1: liveOf("b1", { branch_online: false, last_heartbeat_at: Date.now() - 10 * MIN }) },
  });
  await page.goto("/");
  const c = card(page);
  await expect(c.getByText(t("owner.today.offline"))).toBeVisible();
  await expect(c.getByText("₹1,25,000.50")).toBeVisible();
  await expect(c.getByText(t("owner.age.minutes", { n: 10 }))).toBeVisible();
  await expect(page.getByText(t("owner.unavailable"))).toHaveCount(0);
});

test("a branch with no summary yet says it is waiting", async ({ page }) => {
  await stubCloud(page, ONE, {
    live: { b1: liveOf("b1", { as_of: null, summary: null, branch_online: false }) },
  });
  await page.goto("/");
  await expect(card(page).getByText(t("owner.today.waiting"))).toBeVisible();
});

test("a figure older than 30 minutes while the branch is open turns to warning, a closed one does not", async ({
  page,
}) => {
  const late = t("owner.today.age.late");
  await stubCloud(page, ONE, {
    live: { b1: liveOf("b1", {}, { as_of: Date.now() - 45 * MIN, open: true }) },
  });
  await page.goto("/");
  await expect(card(page).getByText(late, { exact: false }).first()).toBeVisible();

  await page.unroute("**/v1/**");
  await stubCloud(page, ONE, {
    live: { b1: liveOf("b1", {}, { as_of: Date.now() - 45 * MIN, open: false }) },
  });
  await page.reload();
  await expect(card(page).getByText(t("owner.today.closed"), { exact: true })).toBeVisible();
  await expect(card(page).getByText(late, { exact: false })).toHaveCount(0);
});

test("Needs attention lists every branch's items, naming the branch", async ({ page }) => {
  const two = [
    PAID[0],
    { ...PAID[0], id: "b3", name: "Pixel Arena Koramangala", tier: "premium" as const },
  ];
  await stubCloud(page, two, {
    live: {
      b1: liveOf(
        "b1",
        {},
        {
          attention: [{ kind: "low_stock", ref: "p1", text_params: { product: "Cola", qty: "2" } }],
        },
      ),
      b3: liveOf(
        "b3",
        {},
        {
          as_of: Date.now() - 20_000,
          attention: [
            {
              kind: "credit_overdue",
              ref: "pl1",
              text_params: { outstanding_minor: "150000", oldest_age_days: "17" },
            },
            { kind: "alert", ref: "a1", text_params: { alert_kind: "printer_offline" } },
          ],
        },
      ),
    },
  });
  await page.goto("/");
  const items = page
    .getByRole("region", { name: t("owner.today.attention") })
    .getByRole("listitem");
  await expect(items).toHaveCount(3);
  // The branch heard from most recently comes first.
  await expect(items.nth(0)).toContainText("Pixel Arena Koramangala");
  await expect(items.nth(0)).toContainText("₹1,500.00");
  await expect(items.nth(1)).toContainText("printer_offline");
  await expect(items.nth(2)).toHaveText(
    t("owner.today.attention.low_stock", { branch: PAID[0].name, product: "Cola", qty: "2" }),
  );
  await expect(page.getByText(t("owner.today.attention.none"))).toHaveCount(0);
});

test("Refresh fetches at once and leaves focus where it was", async ({ page }) => {
  const calls = await stubCloud(page, ONE);
  await page.goto("/");
  await expect(card(page).getByText(t("owner.today.open"), { exact: true })).toBeVisible();
  const lives = () => calls.filter((c) => c === "GET /v1/branches/b1/live").length;
  expect(lives()).toBe(1);
  const refresh = page.getByRole("button", { name: t("owner.today.refresh") });
  await refresh.focus();
  await refresh.click();
  await expect.poll(lives).toBe(2);
  await expect(refresh).toBeFocused();
});

test("Today re-fetches at the tier cadence while visible, without a reload", async ({ page }) => {
  const calls = await stubCloud(page, [{ ...PAID[0], tier: "premium" }]);
  await page.clock.install();
  await page.goto("/");
  const lives = () => calls.filter((c) => c === "GET /v1/branches/b1/live").length;
  await expect.poll(lives).toBe(1);
  await page.clock.fastForward(4 * MIN);
  await page.waitForTimeout(200);
  expect(lives()).toBe(1);
  await page.clock.fastForward(2 * MIN);
  await expect.poll(lives).toBe(2);
  expect(calls.filter((c) => c === "GET /v1/session")).toHaveLength(1);
});

test("an open reports the age of the figures to the cloud and nothing else", async ({ page }) => {
  const calls = await stubCloud(page, ONE);
  await page.goto("/");
  await expect.poll(() => calls.bodies.b1?.length ?? 0).toBe(1);
  const [body] = calls.bodies.b1 as { age_s: number }[];
  expect(Object.keys(body)).toEqual(["age_s"]);
  expect(body.age_s).toBeGreaterThanOrEqual(119);
  expect(body.age_s).toBeLessThan(180);
  // A refresh within the same open does not report again.
  await page.getByRole("button", { name: t("owner.today.refresh") }).click();
  await page.waitForTimeout(300);
  expect(calls.bodies.b1).toHaveLength(1);
});

test("a failed refresh keeps the figures on screen", async ({ page }) => {
  await stubCloud(page, ONE);
  await page.goto("/");
  await expect(card(page).getByText("₹1,25,000.50")).toBeVisible();
  await page.route("**/v1/branches/b1/live", (route) => route.abort());
  await page.getByRole("button", { name: t("owner.today.refresh") }).click();
  await expect(card(page).getByText("₹1,25,000.50")).toBeVisible();
  await expect(page.getByText(t("owner.unavailable"))).toHaveCount(0);
});

test("the figures sit in two columns on a desktop and one on a phone", async ({ page }, info) => {
  await stubCloud(page, ONE);
  await page.goto("/");
  const cols = await card(page)
    .locator("dl")
    .evaluate((el) => getComputedStyle(el).gridTemplateColumns.split(" ").length);
  expect(cols).toBe(info.project.name === "desktop" ? 2 : 1);
});
