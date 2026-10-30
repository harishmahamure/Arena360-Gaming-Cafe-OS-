import { expect, test, type Page } from "@playwright/test";
import { PAID, stubCloud, t } from "./stub";

const ONE = [PAID[0]];
const DAY = 86_400_000;
const ULID = /^[0-9A-HJKMNP-TV-Z]{26}$/;
const CAFE = "Pixel Arena Gaming";

type Item = {
  key: string;
  cafe: string;
  branch_id: string;
  kind: string;
  payload: unknown;
  base_rev?: number;
  created_at: number;
  state: string;
};

const RATE = {
  kind: "rate.upsert",
  branch_id: "b1",
  payload: { group_id: "g1", price_per_hour_minor: 6000 },
};

const propose = (page: Page, draft: Record<string, unknown>) =>
  page.evaluate(
    (d) => window.dispatchEvent(new CustomEvent("arena:propose-command", { detail: d })),
    draft,
  );

const preview = (page: Page) => page.getByRole("region", { name: t("owner.cmd.preview.title") });
const rows = (page: Page) => page.locator('[data-component="command-row"]');

/** The device queue as IndexedDB holds it. */
const stored = (page: Page) =>
  page.evaluate(
    () =>
      new Promise<Item[]>((ok) => {
        const req = indexedDB.open("arena-owner");
        req.onerror = () => ok([]);
        req.onsuccess = () => {
          const all = req.result.transaction("queue").objectStore("queue").getAll();
          all.onsuccess = () => ok(all.result);
        };
      }),
  );

/** Puts a queued change straight into IndexedDB, as an earlier visit would have left it. */
const seed = (page: Page, item: Item) =>
  page.evaluate(
    (i) =>
      new Promise<void>((ok) => {
        const req = indexedDB.open("arena-owner");
        req.onsuccess = () => {
          const tx = req.result.transaction("queue", "readwrite");
          tx.objectStore("queue").put(i);
          tx.oncomplete = () => ok();
        };
      }),
    item,
  );

const item = (over: Partial<Item> = {}): Item => ({
  key: "01HZZZZZZZZZZZZZZZZZZZZZZA",
  cafe: CAFE,
  branch_id: "b1",
  kind: "rate.upsert",
  payload: RATE.payload,
  created_at: Date.now() - 60_000,
  state: "queued",
  ...over,
});

/** Opens Today (which caches each branch's live answer) and moves to Manage without a reload. */
async function openManageViaToday(page: Page) {
  await page.goto("/");
  await expect(page.getByRole("region", { name: PAID[0].name })).toBeVisible();
  await page.locator("a:visible", { hasText: t("owner.nav.manage") }).click();
  await expect(page.getByRole("heading", { name: t("owner.cmd.list.title") })).toBeVisible();
}

/** Takes the cloud away (every call fails) until `up()`; `up()` also fires the browser's event. */
async function lineDown(page: Page) {
  let down = false;
  await page.route("**/v1/**", (route) =>
    down ? route.abort("internetdisconnected") : route.fallback(),
  );
  return {
    down: () => (down = true),
    up: async (fire = true) => {
      down = false;
      if (fire) await page.evaluate(() => window.dispatchEvent(new Event("online")));
    },
  };
}

test("with no network Send stores the command in IndexedDB and the row says Queued on this device", async ({
  page,
}) => {
  const calls = await stubCloud(page, ONE);
  const line = await lineDown(page);
  await openManageViaToday(page);
  line.down();
  await propose(page, { ...RATE, base_rev: 4 });

  const p = preview(page);
  await expect(p.getByText(t("owner.cmd.preview.last_known"))).toBeVisible();
  const target = p.getByRole("listitem").filter({ hasText: PAID[0].name });
  await expect(target.getByText(t("owner.cmd.preview.online"), { exact: true })).toBeVisible();
  await expect(target.getByText(/^Last known (just now|\d+ min ago)$/)).toBeVisible();
  await p.getByRole("button", { name: t("owner.cmd.send") }).click();

  await expect(preview(page)).toHaveCount(0);
  const row = rows(page).first();
  await expect(row.getByText(t("owner.cmd.state.queued"), { exact: true })).toBeVisible();
  await expect(row.getByText(t("owner.cmd.kind.rate"))).toBeVisible();
  const queue = await stored(page);
  expect(queue).toHaveLength(1);
  expect(queue[0]).toMatchObject({
    cafe: CAFE,
    branch_id: "b1",
    kind: "rate.upsert",
    base_rev: 4,
    payload: RATE.payload,
    state: "queued",
  });
  expect(queue[0].key).toMatch(ULID);
  expect(queue[0].created_at).toBeGreaterThan(Date.now() - 60_000);
  expect(calls.bodies["POST /v1/commands"]).toBeUndefined();
});

test("a branch never seen is Unknown in the offline preview", async ({ page }) => {
  await stubCloud(page, ONE);
  const line = await lineDown(page);
  await page.goto("/manage");
  await expect(page.getByRole("heading", { name: t("owner.cmd.list.title") })).toBeVisible();
  line.down();
  await propose(page, RATE);
  const target = preview(page).getByRole("listitem").filter({ hasText: PAID[0].name });
  await expect(target.getByText(t("owner.cmd.preview.unknown"), { exact: true })).toBeVisible();
  await expect(target.getByText(t("owner.cmd.preview.never"))).toBeVisible();
});

test("back online the queue posts in creation order and the rows move to Waiting for branch", async ({
  page,
}) => {
  const calls = await stubCloud(page, ONE);
  const line = await lineDown(page);
  await openManageViaToday(page);
  line.down();
  for (const price of [6000, 7000]) {
    await propose(page, { ...RATE, payload: { ...RATE.payload, price_per_hour_minor: price } });
    await preview(page)
      .getByRole("button", { name: t("owner.cmd.send") })
      .click();
    await expect(preview(page)).toHaveCount(0);
  }
  await expect(page.getByText(t("owner.cmd.state.queued"), { exact: true })).toHaveCount(2);
  const [first, second] = (await stored(page)).sort((a, b) => a.created_at - b.created_at);

  await line.up();
  await expect(page.getByText(t("owner.cmd.state.queued"), { exact: true })).toHaveCount(0);
  await expect(page.locator('[data-status="pending"]')).toHaveCount(2);
  const sent = calls.bodies["POST /v1/commands"] as { idempotency_key: string }[];
  expect(sent.map((b) => b.idempotency_key)).toEqual([first.key, second.key]);
  expect(await stored(page)).toEqual([]);
});

test("a retry after a lost response returns the same server row", async ({ page }) => {
  const calls = await stubCloud(page, ONE, { lose: 1 });
  await page.goto("/manage");
  await expect(page.getByRole("heading", { name: t("owner.cmd.list.title") })).toBeVisible();
  await propose(page, RATE);
  await preview(page)
    .getByRole("button", { name: t("owner.cmd.send") })
    .click();

  // The cloud made the row but the answer never arrived: the change waits on the device, then goes again.
  await expect(page.locator('[data-status="pending"]')).toHaveCount(1);
  const sent = calls.bodies["POST /v1/commands"] as { idempotency_key: string }[];
  expect(sent).toHaveLength(2);
  expect(sent[1].idempotency_key).toBe(sent[0].idempotency_key);
  expect(calls.commands).toHaveLength(1);
  await expect(rows(page)).toHaveCount(1);
  expect(await stored(page)).toEqual([]);
});

test("a queued command older than 7 days is not sent and shows Expired with Resend", async ({
  page,
}) => {
  const calls = await stubCloud(page, ONE);
  await page.goto("/manage");
  await expect(page.getByRole("heading", { name: t("owner.cmd.list.title") })).toBeVisible();
  await seed(page, item({ created_at: Date.now() - 8 * DAY }));
  await page.reload();

  const row = rows(page).first();
  await expect(row.getByText(t("owner.cmd.state.expired"), { exact: true })).toBeVisible();
  expect(calls.bodies["POST /v1/commands"]).toBeUndefined();
  expect((await stored(page))[0].state).toBe("expired");

  await row.getByRole("button", { name: t("owner.cmd.resend") }).click();
  await expect(page.locator('[data-status="pending"]')).toHaveCount(1);
  const sent = calls.bodies["POST /v1/commands"] as { idempotency_key: string }[];
  expect(sent).toHaveLength(1);
  expect(sent[0].idempotency_key).toMatch(ULID);
  expect(sent[0].idempotency_key).not.toBe("01HZZZZZZZZZZZZZZZZZZZZZZA");
  await expect(page.locator('[data-status="local-expired"]')).toHaveCount(0);
  expect(await stored(page)).toEqual([]);
});

test("a command the cloud refuses on send shows Couldn't apply with Retry and the rest of the queue goes on", async ({
  page,
}) => {
  const refused = "01HZZZZZZZZZZZZZZZZZZZZZZA";
  const calls = await stubCloud(page, ONE, {
    post: (body) =>
      body.idempotency_key === refused
        ? { status: 403, body: { code: "FEATURE_NOT_ENTITLED", message: "" } }
        : undefined,
  });
  await page.goto("/manage");
  await expect(page.getByRole("heading", { name: t("owner.cmd.list.title") })).toBeVisible();
  await seed(page, item({ key: refused, created_at: Date.now() - 120_000 }));
  await seed(page, item({ key: "01HZZZZZZZZZZZZZZZZZZZZZZB", created_at: Date.now() - 60_000 }));
  await page.reload();

  await expect(page.locator('[data-status="pending"]')).toHaveCount(1);
  const bad = page.locator('[data-status="local-refused"]');
  await expect(bad).toHaveCount(1);
  await expect(bad.getByText(t("owner.cmd.state.rejected"), { exact: true })).toBeVisible();
  await expect(bad.getByRole("button", { name: t("common.retry") })).toBeVisible();
  const sent = calls.bodies["POST /v1/commands"] as { idempotency_key: string }[];
  expect(sent.map((b) => b.idempotency_key)).toEqual([refused, "01HZZZZZZZZZZZZZZZZZZZZZZB"]);

  // Retry goes again under a new key (the cloud now takes it).
  await bad.getByRole("button", { name: t("common.retry") }).click();
  await expect(page.locator('[data-status="pending"]')).toHaveCount(2);
  await expect(page.locator('[data-status="local-refused"]')).toHaveCount(0);
});

test("a queued all-branches change shows a row per branch that can take it and posts as all", async ({
  page,
}) => {
  const calls = await stubCloud(page, PAID);
  const line = await lineDown(page);
  await page.goto("/manage");
  await expect(page.getByRole("heading", { name: t("owner.cmd.list.title") })).toBeVisible();
  line.down();
  await seed(page, item({ branch_id: "all" }));
  await page.reload();
  // The second branch is on Free: only the first can take it.
  await expect(rows(page)).toHaveCount(1);
  const row = rows(page).first();
  await expect(row.getByText(t("owner.cmd.state.queued"), { exact: true })).toBeVisible();
  await expect(row.getByText(/All branches · Pixel Arena MG Road/)).toBeVisible();

  await line.up();
  await expect(page.locator('[data-status="pending"]')).toHaveCount(1);
  expect(calls.bodies["POST /v1/commands"]).toMatchObject([{ branch_id: "all" }]);
});

test.describe("sign out with queued changes", () => {
  async function queued(page: Page) {
    await stubCloud(page, ONE);
    await page.route("**/v1/signout", (route) => route.fulfill({ status: 200, body: "null" }));
    const line = await lineDown(page);
    await page.goto("/");
    await expect(page.getByRole("region", { name: PAID[0].name })).toBeVisible();
    await page.locator("a:visible", { hasText: t("owner.nav.manage") }).click();
    await expect(page.getByRole("heading", { name: t("owner.cmd.list.title") })).toBeVisible();
    line.down();
    await propose(page, RATE);
    await preview(page)
      .getByRole("button", { name: t("owner.cmd.send") })
      .click();
    await expect(rows(page)).toHaveCount(1);
    // Back on the line without the browser event, so nothing sends before the sign-out.
    await line.up(false);
    await page.route("**/sign-in", (route) => route.fulfill({ status: 200, body: "sign in" }));
    await page.locator("a:visible", { hasText: t("owner.nav.account") }).click();
    await page.getByRole("button", { name: t("owner.account.sign_out") }).click();
    return line;
  }

  /** The device queue after the sign-out, read from the app again with the line down. */
  async function after(page: Page, line: Awaited<ReturnType<typeof lineDown>>) {
    await page.waitForURL(/\/sign-in$/);
    line.down();
    await page.goto("/account");
    return stored(page);
  }

  test("Keep queued changes leaves them on the device", async ({ page }) => {
    const line = await queued(page);
    await expect(page.getByText(/Changes still waiting on this device: 1/)).toBeVisible();
    await page.getByRole("button", { name: t("owner.account.queue.keep") }).click();
    expect(await after(page, line)).toHaveLength(1);
  });

  test("otherwise they are deleted", async ({ page }) => {
    const line = await queued(page);
    await page.getByRole("button", { name: t("owner.account.queue.delete") }).click();
    expect(await after(page, line)).toEqual([]);
  });
});
