import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type { OwnerRangeCredit, RollupCreditPlayer } from "@arena/ui/generated";
import { PAID, commandOf, liveOf, rangeOfBranch, stubCloud, t } from "./stub";

const ONE = [PAID[0]];
const DUBAI = { ...PAID[0], id: "b3", name: "Pixel Arena Dubai", branch_code: "DXB" };
const TWO = [PAID[0], DUBAI];

const player = (
  id: string,
  username: string,
  outstanding: number,
  oldest: string,
  consent = true,
): RollupCreditPlayer => ({
  player_id: id,
  username,
  outstanding,
  oldest_unpaid_on: oldest,
  consent,
});

const CREDIT: OwnerRangeCredit = {
  as_of: "2026-10-29",
  outstanding: 1_450_000,
  aged: { d0_7: 450_000, d8_14: 400_000, d15p: 600_000 },
  players: [
    player("P1", "asha", 600_000, "2026-10-05"),
    player("P2", "ravi", 400_000, "2026-10-19"),
    player("P3", "meena", 250_000, "2026-10-27"),
    player("P4", "kiran", 200_000, "2026-10-28", false),
  ],
};

const withCredit = (id: string, credit: OwnerRangeCredit) => (q: URLSearchParams) =>
  rangeOfBranch(id, { from: q.get("from") ?? "", to: q.get("to") ?? "", credit });

const openCredit = async (page: Page) => {
  await page.goto("/reports");
  await page.getByRole("tab", { name: t("owner.reports.tab.credit") }).click();
};

const posts = (calls: { bodies: Record<string, unknown[]> }) =>
  (calls.bodies["POST /v1/commands"] ?? []) as {
    branch_id: string;
    kind: string;
    payload: Record<string, unknown>;
    idempotency_key: string;
  }[];

const playerRow = (page: Page, id: string) => page.locator(`[data-player="${id}"]`);
const remindButton = (page: Page, id: string) =>
  playerRow(page, id).getByRole("button", { name: t("owner.reports.credit.remind") });

test("Credit shows the aged outstanding with its date and the players who owe, by id and username only", async ({
  page,
}) => {
  await stubCloud(page, ONE, { range: { b1: withCredit("b1", CREDIT) } });
  await openCredit(page);
  const panel = page.getByRole("tabpanel");
  await expect(
    panel.getByText(t("owner.reports.credit.as_of", { date: "2026-10-29" })),
  ).toBeVisible();
  for (const [bucket, amount] of [
    ["0_7", "4,500.00"],
    ["8_14", "4,000.00"],
    ["15_plus", "6,000.00"],
  ] as const) {
    await expect(
      panel
        .getByText(t(`owner.reports.credit.bucket.${bucket}`))
        .locator("xpath=..")
        .first(),
    ).toContainText(amount);
  }
  await expect(panel).toContainText("14,500.00");

  const asha = playerRow(page, "P1");
  await expect(asha).toContainText("asha");
  await expect(asha).toContainText("P1");
  await expect(asha).toContainText("6,000.00");
  await expect(asha).toContainText(t("owner.reports.credit.oldest", { date: "2026-10-05" }));
  await expect(page.locator("[data-player]")).toHaveCount(4);
  // The answer carries no phone, name or email, so none is on the screen.
  const text = (await panel.textContent()) ?? "";
  expect(text).not.toMatch(/@|\+\d{6,}/);
});

test("Send reminder is one tap: it posts credit.remind for the player and the row shows its state", async ({
  page,
}) => {
  const calls = await stubCloud(page, ONE, { range: { b1: withCredit("b1", CREDIT) } });
  await openCredit(page);
  await remindButton(page, "P1").click();

  await expect.poll(() => posts(calls).length).toBe(1);
  const [post] = posts(calls);
  expect(post.kind).toBe("credit.remind");
  expect(post.branch_id).toBe("b1");
  expect(post.payload).toEqual({ player_id: "P1" });
  // No code prompt: the kind is not sensitive.
  await expect(page.getByRole("dialog")).toHaveCount(0);

  const row = playerRow(page, "P1").locator('[data-component="command-row"]');
  await expect(row).toHaveAttribute("data-status", "pending");
  await expect(row).toContainText(t("owner.cmd.kind.credit_remind"));
  // The other players have no row, and the one just reminded is now marked.
  await expect(page.locator('[data-component="command-row"]')).toHaveCount(1);
  await expect(playerRow(page, "P1")).toContainText(t("owner.reports.credit.reminded"));
});

test("a reminder the Hub refused reads Couldn't apply at branch, with Retry", async ({ page }) => {
  const calls = await stubCloud(page, ONE, {
    range: { b1: withCredit("b1", CREDIT) },
    commands: [
      commandOf({
        id: 7,
        kind: "credit.remind",
        status: "rejected",
        payload: { player_id: "P3" },
        error: { code: "STALE", message: "" },
      }),
    ],
  });
  await openCredit(page);
  const row = playerRow(page, "P3").locator('[data-component="command-row"]');
  await expect(row).toHaveAttribute("data-status", "rejected");
  await expect(row).toContainText(t("owner.cmd.state.rejected"));
  // A refused reminder does not count as sent: one tap, no confirmation.
  await expect(playerRow(page, "P3")).not.toContainText(t("owner.reports.credit.reminded"));
  await remindButton(page, "P3").click();
  await expect.poll(() => posts(calls).length).toBe(1);
  expect(posts(calls)[0].payload).toEqual({ player_id: "P3" });
});

test("a player reminded today asks to confirm before another is sent", async ({ page }) => {
  const calls = await stubCloud(page, ONE, {
    range: { b1: withCredit("b1", CREDIT) },
    live: { b1: liveOf("b1", {}, { reminded_today: ["P2"] }) },
  });
  await openCredit(page);
  const ravi = playerRow(page, "P2");
  await expect(ravi).toContainText(t("owner.reports.credit.reminded"));

  await remindButton(page, "P2").click();
  await expect(ravi.getByRole("button", { name: t("owner.reports.credit.again") })).toBeVisible();
  expect(posts(calls)).toHaveLength(0);

  // Cancel sends nothing and goes back.
  await ravi.getByRole("button", { name: t("owner.cmd.cancel") }).click();
  await expect(remindButton(page, "P2")).toBeVisible();
  expect(posts(calls)).toHaveLength(0);

  await remindButton(page, "P2").click();
  await ravi.getByRole("button", { name: t("owner.reports.credit.again") }).click();
  await expect.poll(() => posts(calls).length).toBe(1);
  expect(posts(calls)[0].payload).toEqual({ player_id: "P2" });
  // Someone else is still one tap.
  await remindButton(page, "P3").click();
  await expect.poll(() => posts(calls).length).toBe(2);
});

test("tapping again after sending asks to confirm, and an older Hub without the list is one tap", async ({
  page,
}) => {
  const calls = await stubCloud(page, ONE, {
    range: { b1: withCredit("b1", CREDIT) },
    live: { b1: liveOf("b1") },
  });
  await openCredit(page);
  await remindButton(page, "P1").click();
  await expect.poll(() => posts(calls).length).toBe(1);
  await remindButton(page, "P1").click();
  await expect(
    playerRow(page, "P1").getByRole("button", { name: t("owner.reports.credit.again") }),
  ).toBeVisible();
  expect(posts(calls)).toHaveLength(1);
});

test("a player without consent says so and has no button", async ({ page }) => {
  await stubCloud(page, ONE, { range: { b1: withCredit("b1", CREDIT) } });
  await openCredit(page);
  const kiran = playerRow(page, "P4");
  await expect(kiran).toContainText(t("owner.reports.credit.no_consent"));
  await expect(kiran.getByRole("button")).toHaveCount(0);
  await expect(playerRow(page, "P1").getByRole("button")).toHaveCount(1);
});

test("a branch with no accepted rollup reads No credit figures yet", async ({ page }) => {
  await stubCloud(page, ONE);
  await openCredit(page);
  await expect(page.getByRole("tabpanel")).toContainText(t("owner.reports.credit.none"));
  await expect(page.locator("[data-player]")).toHaveCount(0);
});

test("a branch whose newest rollup lists nobody says nobody owes", async ({ page }) => {
  const clear: OwnerRangeCredit = {
    as_of: "2026-10-29",
    outstanding: 0,
    aged: { d0_7: 0, d8_14: 0, d15p: 0 },
    players: [],
  };
  await stubCloud(page, ONE, { range: { b1: withCredit("b1", clear) } });
  await openCredit(page);
  await expect(page.getByRole("tabpanel")).toContainText(t("owner.reports.credit.players.none"));
});

test("several branches each get their own credit and a reminder goes to its own branch", async ({
  page,
}) => {
  const other: OwnerRangeCredit = {
    ...CREDIT,
    outstanding: 90_000,
    players: [player("D1", "omar", 90_000, "2026-10-26")],
  };
  const calls = await stubCloud(page, TWO, {
    range: { b1: withCredit("b1", CREDIT), b3: withCredit("b3", other) },
  });
  await openCredit(page);
  const panel = page.getByRole("tabpanel");
  await expect(panel.getByRole("heading", { name: ONE[0].name })).toBeVisible();
  await expect(panel.getByRole("heading", { name: DUBAI.name })).toBeVisible();
  await remindButton(page, "D1").click();
  await expect.poll(() => posts(calls).length).toBe(1);
  expect(posts(calls)[0]).toMatchObject({ branch_id: "b3", kind: "credit.remind" });
});

test("Export → CSV and PDF work on the Credit tab", async ({ page }) => {
  await page.addInitScript(() => {
    const w = window as unknown as { printed: number };
    w.printed = 0;
    window.print = () => {
      w.printed++;
    };
  });
  const calls = await stubCloud(page, ONE, { range: { b1: withCredit("b1", CREDIT) } });
  await openCredit(page);
  await expect(page.locator("[data-player]")).toHaveCount(4);

  const download = page.waitForEvent("download");
  await page.getByRole("button", { name: t("owner.reports.export.csv") }).click();
  const file = await download;
  expect(file.suggestedFilename()).toMatch(/^arena360-credit-\d{4}-\d{2}-\d{2}-to-/);
  expect(calls.exports.at(-1)).toMatch(/^\/v1\/branches\/b1\/export\?from=.*&report=credit$/);

  await page.getByRole("button", { name: t("owner.reports.export.pdf") }).click();
  await expect
    .poll(() => page.evaluate(() => (window as unknown as { printed: number }).printed))
    .toBe(1);
  // The print view carries the report name and drops the buttons.
  await page.emulateMedia({ media: "print" });
  await expect(page.locator(".print-header")).toContainText(t("owner.reports.tab.credit"));
  await expect(playerRow(page, "P1")).toBeVisible();
  await expect(remindButton(page, "P1")).toBeHidden();
});

for (const theme of ["light", "dark", "hc"] as const) {
  test(`Credit (${theme}): axe finds nothing serious and nothing overflows`, async ({ page }) => {
    await stubCloud(page, TWO, {
      range: { b1: withCredit("b1", CREDIT), b3: withCredit("b3", CREDIT) },
      live: { b1: liveOf("b1", {}, { reminded_today: ["P2"] }) },
      commands: [
        commandOf({
          id: 3,
          kind: "credit.remind",
          status: "applied",
          payload: { player_id: "P1" },
          done_at: Date.now(),
        }),
      ],
    });
    await page.addInitScript((v) => localStorage.setItem("arena.theme", v), theme);
    await openCredit(page);
    await expect(page.locator("[data-player]")).toHaveCount(8);
    await page.evaluate(() => document.fonts.ready);
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth > document.documentElement.clientWidth,
      ),
    ).toBe(false);
    const blocking = (await new AxeBuilder({ page }).analyze()).violations.filter(
      (v) => v.impact === "serious" || v.impact === "critical",
    );
    const report = blocking.map(
      (v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`,
    );
    expect(report, report.join("\n")).toEqual([]);
  });
}
