import { expect, test, type Page } from "@playwright/test";
import { PAID, commandOf, liveOf, stubCloud, summaryOf, t } from "./stub";

const ONE = [PAID[0]];
const DAY = 86_400_000;
const ULID = /^[0-9A-HJKMNP-TV-Z]{26}$/;

/** The Manage forms do not exist yet: a fixture change goes to the preview the way they will. */
const propose = (page: Page, draft: Record<string, unknown>) =>
  page.evaluate(
    (d) => window.dispatchEvent(new CustomEvent("arena:propose-command", { detail: d })),
    draft,
  );

const RATE = {
  kind: "rate.upsert",
  branch_id: "b1",
  payload: { group_id: "g1", price_per_hour_minor: 6000 },
};

async function openManage(page: Page) {
  await page.goto("/manage");
  await expect(page.getByRole("heading", { name: t("owner.cmd.list.title") })).toBeVisible();
}

const preview = (page: Page) => page.getByRole("region", { name: t("owner.cmd.preview.title") });
const rows = (page: Page) => page.locator('[data-component="command-row"]');

test("the preview names each target with its online state and says why one is not offered", async ({
  page,
}) => {
  await stubCloud(page, PAID, { offline: ["b1"] });
  await openManage(page);
  await propose(page, { ...RATE, branch_id: "all" });

  const p = preview(page);
  await expect(p).toBeVisible();
  await expect(p.getByText(t("owner.cmd.scope.all"))).toBeVisible();
  const first = p.getByRole("listitem").filter({ hasText: PAID[0].name });
  await expect(first.getByText(t("owner.cmd.preview.offline"), { exact: true })).toBeVisible();
  await expect(first.getByText(t("owner.cmd.preview.offline_note"))).toBeVisible();
  const second = p.getByRole("listitem").filter({ hasText: PAID[1].name });
  await expect(second.getByText(t("owner.cmd.preview.online"), { exact: true })).toBeVisible();
  await expect(
    second.getByText(`${t("owner.cmd.preview.not_offered")} · ${t("owner.cmd.reason.entitled")}`),
  ).toBeVisible();
  await expect(p.getByRole("button", { name: t("owner.cmd.send") })).toBeEnabled();
  await expect(p.getByRole("button", { name: t("owner.cmd.cancel") })).toBeVisible();
  await expect(p.getByRole("heading", { name: t("owner.cmd.preview.title") })).toBeFocused();
});

test("a branch that needs a Hub update is not offered and Send stays off when none can take it", async ({
  page,
}) => {
  const calls = await stubCloud(page, ONE, {
    post: () => ({ status: 409, body: { code: "BRANCH_UPDATE_REQUIRED", message: "" } }),
  });
  await page.route("**/v1/commands/preview", (route) =>
    route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        targets: [
          {
            branch_id: "b1",
            name: PAID[0].name,
            online: true,
            offered: false,
            error: { code: "BRANCH_UPDATE_REQUIRED", message: "" },
          },
        ],
      }),
    }),
  );
  await openManage(page);
  await propose(page, RATE);
  const p = preview(page);
  await expect(p.getByText(t("owner.cmd.reason.update"))).toBeVisible();
  await expect(p.getByText(t("owner.cmd.preview.none"), { exact: true }).first()).toBeVisible();
  await expect(p.getByRole("button", { name: t("owner.cmd.send") })).toBeDisabled();
  expect(calls.bodies["POST /v1/commands"]).toBeUndefined();
});

test("Cancel closes the preview and sends nothing", async ({ page }) => {
  const calls = await stubCloud(page, ONE);
  await openManage(page);
  await propose(page, RATE);
  await preview(page)
    .getByRole("button", { name: t("owner.cmd.cancel") })
    .click();
  await expect(preview(page)).toHaveCount(0);
  expect(calls.bodies["POST /v1/commands"]).toBeUndefined();
});

test("Send queues one command with a ULID key and the row shows Waiting for branch", async ({
  page,
}) => {
  const calls = await stubCloud(page, ONE);
  await openManage(page);
  await expect(page.getByText(t("owner.cmd.list.empty"))).toBeVisible();
  await propose(page, { ...RATE, base_rev: 4 });
  const send = preview(page).getByRole("button", { name: t("owner.cmd.send") });
  await expect(send).toBeEnabled();
  await send.click();

  await expect(preview(page)).toHaveCount(0);
  const sent = calls.bodies["POST /v1/commands"];
  expect(sent).toHaveLength(1);
  expect(sent[0]).toMatchObject({
    branch_id: "b1",
    kind: "rate.upsert",
    base_rev: 4,
    payload: RATE.payload,
  });
  expect((sent[0] as { idempotency_key: string }).idempotency_key).toMatch(ULID);

  await expect(rows(page)).toHaveCount(1);
  const row = rows(page).first();
  await expect(row.getByText(t("owner.cmd.kind.rate"))).toBeVisible();
  await expect(row.getByText(PAID[0].name)).toBeVisible();
  await expect(row.getByText(/^Waiting for branch · (just now|\d+ min ago)$/)).toBeVisible();
  await expect(page.getByRole("heading", { name: t("owner.cmd.group.today") })).toBeVisible();
});

test("a send that fails keeps the preview open and says nothing changed; a retry reuses the key", async ({
  page,
}) => {
  let fail = true;
  const calls = await stubCloud(page, ONE, {
    post: () => (fail ? { status: 500, body: { code: "INTERNAL", message: "" } } : undefined),
  });
  await openManage(page);
  await propose(page, RATE);
  const send = preview(page).getByRole("button", { name: t("owner.cmd.send") });
  await send.click();
  await expect(preview(page).getByText(t("owner.cmd.send.failed"))).toBeVisible();
  fail = false;
  await send.click();
  await expect(preview(page)).toHaveCount(0);
  const keys = calls.bodies["POST /v1/commands"].map(
    (b) => (b as { idempotency_key: string }).idempotency_key,
  );
  expect(keys).toHaveLength(2);
  expect(keys[0]).toBe(keys[1]);
  await expect(rows(page)).toHaveCount(1);
});

test("All branches sends one command and shows a row per branch it reached", async ({ page }) => {
  const three = [PAID[0], { ...PAID[0], id: "b3", name: "Pixel Arena Koramangala" }, PAID[1]];
  const calls = await stubCloud(page, three);
  await openManage(page);
  await propose(page, { ...RATE, branch_id: "all" });
  await preview(page)
    .getByRole("button", { name: t("owner.cmd.send") })
    .click();
  await expect(rows(page)).toHaveCount(2);
  expect(calls.bodies["POST /v1/commands"]).toHaveLength(1);
  await expect(
    rows(page)
      .first()
      .getByText(/^All branches · /),
  ).toBeVisible();
});

test("every server state reads in the owner's words, grouped Today and Earlier", async ({
  page,
}) => {
  const now = Date.now();
  await stubCloud(page, ONE, {
    commands: [
      commandOf({ id: 6, status: "pending" }, 120_000),
      commandOf({ id: 5, status: "fetched" }, 5 * 60_000),
      commandOf({ id: 4, status: "applied", done_at: now - 60_000 }, 180_000),
      commandOf(
        { id: 3, status: "rejected", error: { code: "VALIDATION_FAILED", message: "x" } },
        4 * 60_000,
      ),
      commandOf(
        { id: 2, status: "rejected", error: { code: "TOKEN_REVOKED", message: "x" } },
        6 * 60_000,
      ),
      commandOf({ id: 1, status: "expired" }, 8 * DAY),
    ],
  });
  await openManage(page);
  await expect(rows(page)).toHaveCount(6);
  const by = (n: string) => page.locator(`[data-component="command-row"][data-status="${n}"]`);

  await expect(by("pending").getByText("Waiting for branch · 2 min ago")).toBeVisible();
  await expect(by("fetched").getByText("Waiting for branch · 5 min ago")).toBeVisible();
  await expect(by("applied").getByText(/^Applied · \d{1,2}:\d{2}/)).toBeVisible();
  await expect(by("applied").getByRole("button")).toHaveCount(0);

  const rejected = by("rejected");
  const revoked = rejected.filter({ hasText: t("owner.cmd.state.revoked") });
  await expect(revoked).toHaveCount(1);
  await expect(revoked.getByRole("button")).toHaveCount(0);
  const plain = rejected.filter({ hasText: t("owner.cmd.state.rejected") });
  await expect(plain.getByRole("button", { name: t("common.retry") })).toBeVisible();

  const expired = by("expired");
  await expect(expired.getByText(t("owner.cmd.state.expired"), { exact: true })).toBeVisible();
  await expect(expired.getByRole("button", { name: t("owner.cmd.resend") })).toBeVisible();

  await expect(page.getByRole("heading", { name: t("owner.cmd.group.today") })).toBeVisible();
  const earlier = page.getByRole("heading", { name: t("owner.cmd.group.earlier") });
  await expect(earlier).toBeVisible();
  const list = earlier.locator("xpath=following-sibling::ul[1]");
  await expect(list.locator('[data-component="command-row"]')).toHaveCount(1);
  await expect(list.locator('[data-status="expired"]')).toHaveCount(1);
});

test("Retry sends a new command with a new key and the current base_rev, and keeps the old row", async ({
  page,
}) => {
  const plan = { id: "p1", name: "Evening pack" };
  const base = summaryOf("b1");
  const calls = await stubCloud(page, ONE, {
    commands: [
      commandOf({
        id: 7,
        kind: "plan.upsert",
        status: "rejected",
        error: { code: "VALIDATION_FAILED", message: "x" },
        payload: { ...plan, base_rev: 3 },
      }),
    ],
    live: {
      b1: liveOf(
        "b1",
        {},
        {
          catalogue: { ...base.catalogue, plans: [{ id: "p1", name: "Evening pack", rev: 9 }] },
        },
      ),
    },
  });
  await openManage(page);
  const row = rows(page).first();
  await expect(row.getByText(`${t("owner.cmd.kind.plan")} · Evening pack`)).toBeVisible();
  await row.getByRole("button", { name: t("common.retry") }).click();

  await expect(rows(page)).toHaveCount(2);
  const sent = calls.bodies["POST /v1/commands"];
  expect(sent).toHaveLength(1);
  expect(sent[0]).toMatchObject({
    branch_id: "b1",
    kind: "plan.upsert",
    base_rev: 9,
    payload: plan,
  });
  expect((sent[0] as { idempotency_key: string }).idempotency_key).toMatch(ULID);
  await expect(page.locator('[data-status="rejected"]')).toHaveCount(1);
  await expect(page.locator('[data-status="pending"]')).toHaveCount(1);
});

test("Resend of an expired command goes to that branch only", async ({ page }) => {
  const calls = await stubCloud(page, PAID, {
    commands: [commandOf({ id: 2, status: "expired", group_id: "G1" }, 8 * DAY)],
  });
  await openManage(page);
  await rows(page)
    .first()
    .getByRole("button", { name: t("owner.cmd.resend") })
    .click();
  await expect(rows(page)).toHaveCount(2);
  const sent = calls.bodies["POST /v1/commands"];
  expect(sent[0]).toMatchObject({ branch_id: "b1", kind: "rate.upsert" });
  expect(sent[0]).not.toHaveProperty("group_id");
});

test("a failed Retry says so beside the row and can be pressed again", async ({ page }) => {
  let fail = true;
  const calls = await stubCloud(page, ONE, {
    commands: [
      commandOf({ id: 3, status: "rejected", error: { code: "VALIDATION_FAILED", message: "x" } }),
    ],
    post: () => (fail ? { status: 500, body: { code: "INTERNAL", message: "" } } : undefined),
  });
  await openManage(page);
  const retry = rows(page)
    .first()
    .getByRole("button", { name: t("common.retry") });
  await retry.click();
  await expect(page.getByText(t("owner.cmd.again.failed"))).toBeVisible();
  fail = false;
  await retry.click();
  await expect(rows(page)).toHaveCount(2);
  expect(calls.bodies["POST /v1/commands"]).toHaveLength(2);
});

test("a row updates within 10 s while the page is visible and the change is announced once", async ({
  page,
}) => {
  await page.clock.install();
  const calls = await stubCloud(page, ONE, { commands: [commandOf({ id: 1 })] });
  await openManage(page);
  const row = rows(page).first();
  await expect(row.getByText(/^Waiting for branch/)).toBeVisible();
  const live = page.getByRole("status").filter({ hasText: /^$/ }).last();
  await expect(live).toBeAttached();

  calls.commands[0] = { ...calls.commands[0], status: "applied", done_at: Date.now() };
  await page.clock.runFor(10_500);
  await expect(row.getByText(/^Applied · \d{1,2}:\d{2}/)).toBeVisible();
  const said = page.locator('[role="status"][aria-live="polite"]').filter({ hasText: /Applied/ });
  await expect(said).toHaveCount(1);
  await expect(said).toHaveText(/^Price change at Pixel Arena MG Road: Applied · \d{1,2}:\d{2}.*$/);
  const text = await said.textContent();
  await page.clock.runFor(10_500);
  await expect(said).toHaveText(text ?? "");
});

test("an offline branch's card on Today lists its waiting commands", async ({ page }) => {
  await stubCloud(page, ONE, {
    live: { b1: liveOf("b1", { branch_online: false, last_heartbeat_at: Date.now() - 600_000 }) },
    commands: [
      commandOf({ id: 2, status: "pending" }, 120_000),
      commandOf({ id: 1, status: "applied", done_at: Date.now() }, 300_000),
    ],
  });
  await page.goto("/");
  const card = page.getByRole("region", { name: PAID[0].name });
  await expect(card.getByText(t("owner.cmd.today.waiting"), { exact: true })).toBeVisible();
  await expect(card.getByText(/Price change · Queued 2 min ago/)).toBeVisible();
  await expect(card.getByRole("listitem").filter({ hasText: /Price change/ })).toHaveCount(1);
});

test("an online branch's card shows no waiting list", async ({ page }) => {
  await stubCloud(page, ONE, { commands: [commandOf({ id: 2, status: "pending" })] });
  await page.goto("/");
  const card = page.getByRole("region", { name: PAID[0].name });
  await expect(card.getByText("₹1,25,000.50")).toBeVisible();
  await expect(card.getByText(t("owner.cmd.today.waiting"), { exact: true })).toHaveCount(0);
});
