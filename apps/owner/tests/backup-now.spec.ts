import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import { PAID, backupOf, commandOf, stubCloud, t, type Branch } from "./stub";

// Story 9.17: Account -> Branches shows each paid branch's backup line from the Hub's latest
// heartbeat and a "Back up now" button (`backup.now`, a command-row follows it); a Free branch
// shows `tier-lock` "Available on Basic" instead.

const card = (page: Page) => page.getByRole("listitem").filter({ hasText: PAID[0].name }).first();
const line = (page: Page) => card(page).getByTestId("backup-line");
const button = (page: Page) =>
  card(page).getByRole("button", { name: t("owner.branches.backup.now") });
const row = (page: Page) => card(page).locator('[data-component="command-row"]');

const paid = (backup?: Branch["backup"]): Branch[] => [
  {
    ...PAID[0],
    hub: { paired: true, platform: "windows", last_seen_at: Date.now() - 60_000 },
    ...(backup ? { backup } : {}),
  },
];

test("a paid branch shows last upload, last snapshot and pending batches as ages", async ({
  page,
}) => {
  await stubCloud(page, paid(backupOf()));
  await page.goto("/account/branches");
  await expect(line(page)).toHaveText(
    "Last upload 4 min ago · Last snapshot 3 h ago · 2 batches pending",
  );
  await expect(button(page)).toBeVisible();
  await expect(card(page).locator('[data-component="tier-lock"]')).toHaveCount(0);
});

test("one batch pending is singular, and a missing time says so", async ({ page }) => {
  await stubCloud(
    page,
    paid({ pending_batches: 1, last_snapshot_at: undefined, last_upload_at: undefined }),
  );
  await page.goto("/account/branches");
  await expect(line(page)).toHaveText("No upload yet · No snapshot yet · 1 batch pending");
});

test("before the Hub has reported, the line says so and the button is still offered", async ({
  page,
}) => {
  await stubCloud(page, paid());
  await page.goto("/account/branches");
  await expect(line(page)).toHaveText(t("owner.branches.backup.none"));
  await expect(button(page)).toBeEnabled();
});

test("Back up now sends one backup.now and a command-row follows it to Applied", async ({
  page,
}) => {
  await page.clock.install();
  const calls = await stubCloud(page, paid(backupOf()));
  await page.goto("/account/branches");
  await button(page).click();

  await expect(row(page)).toHaveCount(1);
  await expect(row(page)).toHaveAttribute("data-status", "pending");
  await expect(row(page)).toContainText(t("owner.cmd.kind.backup"));
  await expect(row(page).getByText(/^Waiting for branch/)).toBeVisible();
  const posts = calls.bodies["POST /v1/commands"] as {
    branch_id: string;
    kind: string;
    payload: unknown;
  }[];
  expect(posts).toHaveLength(1);
  expect(posts[0]).toMatchObject({ branch_id: "b1", kind: "backup.now", payload: {} });
  // Not offered again while it is on its way.
  await expect(button(page)).toBeDisabled();

  calls.commands[0] = { ...calls.commands[0], status: "applied", done_at: Date.now() };
  await page.clock.runFor(10_500);
  await expect(row(page)).toHaveAttribute("data-status", "applied");
  await expect(row(page).getByText(/^Applied · \d{1,2}:\d{2}/)).toBeVisible();
  await expect(button(page)).toBeEnabled();
  expect(calls.bodies["POST /v1/commands"]).toHaveLength(1);
});

test("a backup too soon after an applied one shows the cloud's message under the button and no row", async ({
  page,
}) => {
  const message = "A backup ran at 14:05. Try again after 14:20.";
  const calls = await stubCloud(page, paid(backupOf()), {
    post: () => ({ status: 429, body: { code: "RATE_LIMITED", message } }),
  });
  await page.goto("/account/branches");
  await button(page).click();
  await expect(card(page).getByTestId("backup-error")).toHaveText(message);
  await expect(row(page)).toHaveCount(0);
  await expect(button(page)).toBeEnabled();
  expect(calls.commands).toHaveLength(0);
});

test("a cycle that fails rejects the row with Retry and the line says Backup failed with the time", async ({
  page,
}) => {
  await page.clock.install();
  const failedAt = Date.now() - 30_000;
  let branches = paid(backupOf());
  const calls = await stubCloud(page, () => branches);
  await page.goto("/account/branches");
  await button(page).click();
  await expect(row(page)).toHaveAttribute("data-status", "pending");

  calls.commands[0] = {
    ...calls.commands[0],
    status: "rejected",
    done_at: Date.now(),
    error: { code: "CLOUD_UNREACHABLE", message: "The cloud couldn't be reached." },
  };
  branches = paid(backupOf({ failed_at: failedAt }));
  await page.clock.runFor(10_500);
  await expect(row(page)).toHaveAttribute("data-status", "rejected");
  await expect(row(page).getByText(t("owner.cmd.state.rejected"))).toBeVisible();
  await expect(row(page).getByRole("button", { name: t("common.retry") })).toBeVisible();
  await expect(line(page)).toHaveText(/^Backup failed · \d{1,2}:\d{2}/);
  await expect(line(page)).toHaveAttribute("data-failed", "true");

  // Retry sends a fresh backup.now and the row follows the new one.
  await row(page)
    .getByRole("button", { name: t("common.retry") })
    .click();
  await expect(row(page)).toHaveAttribute("data-status", "pending");
  expect(calls.bodies["POST /v1/commands"]).toHaveLength(2);
});

test("a Free branch shows Available on Basic in place of the button, and no backup line", async ({
  page,
}) => {
  await stubCloud(page, [{ ...PAID[0], tier: "free", flags: [] }]);
  await page.goto("/account/branches");
  const lock = card(page).locator('[data-component="tier-lock"]');
  await expect(lock).toContainText(t("staff.tier.available_on", { tier: "Basic" }));
  await expect(button(page)).toHaveCount(0);
  await expect(line(page)).toHaveCount(0);
});

test("the backup line and button have no accessibility violations", async ({ page }) => {
  await stubCloud(page, paid(backupOf({ failed_at: Date.now() - 60_000 })));
  await page.goto("/account/branches");
  await expect(button(page)).toBeVisible();
  const results = await new AxeBuilder({ page }).analyze();
  expect(results.violations).toEqual([]);
});

test("with no signal the tap waits on this device as Queued", async ({ page }) => {
  await stubCloud(page, paid(backupOf()), {
    commands: [commandOf({ id: 1, kind: "backup.now", payload: {}, status: "applied" })],
  });
  await page.goto("/account/branches");
  await page.route("**/v1/commands", (route) =>
    route.request().method() === "POST" ? route.abort("internetdisconnected") : route.fallback(),
  );
  await button(page).click();
  await expect(row(page)).toHaveAttribute("data-status", "local-queued");
  await expect(row(page).getByText(t("owner.cmd.state.queued"))).toBeVisible();
});
