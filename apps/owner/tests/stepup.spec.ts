import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import { PAID, stubCloud, t } from "./stub";

const ONE = [PAID[0]];
const CAFE = "Pixel Arena Gaming";
const GOOD = "123456";
const SENSITIVE = "test.sensitive";

type Item = {
  key: string;
  cafe: string;
  branch_id: string;
  kind: string;
  payload: unknown;
  created_at: number;
  state: string;
};

const draft = (kind: string) => ({ kind, branch_id: "b1", payload: { anything: true } });

const propose = (page: Page, d: Record<string, unknown>) =>
  page.evaluate(
    (x) => window.dispatchEvent(new CustomEvent("arena:propose-command", { detail: x })),
    d,
  );

const preview = (page: Page) => page.getByRole("region", { name: t("owner.cmd.preview.title") });
const dialog = (page: Page) => page.getByRole("dialog", { name: t("owner.stepup.title") });
const rows = (page: Page) => page.locator('[data-component="command-row"]');

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

/**
 * The cloud as a session without a proof: the test kind is refused `MFA_REQUIRED` until a step-up
 * with `GOOD` passes; `wrong` codes are `TOTP_INVALID` (and past `limit` of them 429 with a wait).
 */
async function cloud(page: Page, opts: { limit?: number } = {}) {
  let proof = false;
  let wrong = 0;
  const steps: { method: string; code: string }[] = [];
  const calls = await stubCloud(page, ONE, {
    post: (body) =>
      body.kind === SENSITIVE && !proof
        ? { status: 403, body: { code: "MFA_REQUIRED", message: "Confirm it's you." } }
        : undefined,
  });
  await page.route("**/v1/session/stepup", (route) => {
    const body = route.request().postDataJSON();
    steps.push(body);
    if (opts.limit !== undefined && wrong >= opts.limit)
      return route.fulfill({
        status: 429,
        headers: { "retry-after": "900" },
        contentType: "application/json",
        body: JSON.stringify({ code: "RATE_LIMITED", message: "" }),
      });
    if (body.code === GOOD) {
      proof = true;
      return route.fulfill({ status: 204 });
    }
    wrong++;
    return route.fulfill({
      status: 400,
      contentType: "application/json",
      body: JSON.stringify({ code: "TOTP_INVALID", field: "code", message: "no" }),
    });
  });
  return Object.assign(calls, { steps });
}

const posts = (calls: Awaited<ReturnType<typeof cloud>>) =>
  (calls.bodies["POST /v1/commands"] ?? []) as { idempotency_key: string; kind: string }[];

async function openManage(page: Page) {
  await page.goto("/manage");
  await expect(page.getByRole("heading", { name: t("owner.cmd.list.title") })).toBeVisible();
}

test("a sensitive change asks for a code and goes again with the same key once it passes", async ({
  page,
}) => {
  const calls = await cloud(page);
  await openManage(page);
  await propose(page, draft(SENSITIVE));
  await preview(page)
    .getByRole("button", { name: t("owner.cmd.send") })
    .click();

  const d = dialog(page);
  await expect(d).toBeVisible();
  const code = d.getByLabel(t("owner.stepup.code"));
  await expect(code).toBeFocused();
  // Pasting a code is allowed: the field never cancels a paste.
  const prevented = await code.evaluate((el) => {
    const event = new ClipboardEvent("paste", {
      bubbles: true,
      cancelable: true,
      clipboardData: new DataTransfer(),
    });
    el.dispatchEvent(event);
    return event.defaultPrevented;
  });
  expect(prevented).toBe(false);

  await code.fill(GOOD);
  await d.getByRole("button", { name: t("owner.stepup.confirm") }).click();
  await expect(d).toBeHidden();
  await expect(preview(page)).toHaveCount(0);
  await expect(page.locator('[data-status="pending"]')).toHaveCount(1);

  expect(calls.steps).toEqual([{ method: "totp", code: GOOD }]);
  const sent = posts(calls);
  expect(sent).toHaveLength(2);
  expect(sent[1].idempotency_key).toBe(sent[0].idempotency_key);
  expect(sent[1]).toMatchObject({ kind: SENSITIVE });
});

test("a change that is not sensitive never asks", async ({ page }) => {
  const calls = await cloud(page);
  await openManage(page);
  await propose(page, draft("rate.upsert"));
  await preview(page)
    .getByRole("button", { name: t("owner.cmd.send") })
    .click();
  await expect(page.locator('[data-status="pending"]')).toHaveCount(1);
  await expect(dialog(page)).toHaveCount(0);
  expect(calls.steps).toEqual([]);
  expect(posts(calls)).toHaveLength(1);
});

test("a wrong code reads the same for every method and never names the factor", async ({
  page,
}) => {
  await cloud(page);
  await openManage(page);
  await propose(page, draft(SENSITIVE));
  await preview(page)
    .getByRole("button", { name: t("owner.cmd.send") })
    .click();

  const d = dialog(page);
  await d.getByLabel(t("owner.stepup.code")).fill("000000");
  await d.getByRole("button", { name: t("owner.stepup.confirm") }).click();
  await expect(d.getByText(t("owner.stepup.wrong"))).toBeVisible();

  await d.getByRole("button", { name: t("owner.stepup.use_backup") }).click();
  await expect(d.getByText(t("owner.stepup.hint.backup"))).toBeVisible();
  await expect(d.getByLabel(t("owner.stepup.code"))).toBeFocused();
  await d.getByLabel(t("owner.stepup.code")).fill("abcd-efgh");
  await d.getByRole("button", { name: t("owner.stepup.confirm") }).click();
  await expect(d.getByText(t("owner.stepup.wrong"))).toBeVisible();
  const text = (await d.innerText()).toLowerCase();
  expect(text).not.toContain("authenticator app is wrong");
  expect(text).not.toContain("backup code is wrong");
  await expect(d).toBeVisible();
});

test("Esc cancels the prompt without sending the change", async ({ page }) => {
  const calls = await cloud(page);
  await openManage(page);
  await propose(page, draft(SENSITIVE));
  await preview(page)
    .getByRole("button", { name: t("owner.cmd.send") })
    .click();
  await expect(dialog(page)).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(dialog(page)).toBeHidden();

  // Only the refused post happened: nothing was queued or sent, and the preview is still there.
  expect(calls.steps).toEqual([]);
  expect(posts(calls)).toHaveLength(1);
  expect(calls.commands).toHaveLength(0);
  await expect(preview(page)).toBeVisible();
  await expect(
    preview(page).getByRole("button", { name: t("owner.cmd.send") }),
  ).not.toHaveAttribute("aria-disabled", "true");
});

test("five wrong codes pause the step-up: the dialog shows the time and the page stays signed in", async ({
  page,
}) => {
  const calls = await cloud(page, { limit: 5 });
  await openManage(page);
  await propose(page, draft(SENSITIVE));
  await preview(page)
    .getByRole("button", { name: t("owner.cmd.send") })
    .click();
  const d = dialog(page);
  for (let i = 0; i < 5; i++) {
    await d.getByLabel(t("owner.stepup.code")).fill("000000");
    await d.getByRole("button", { name: t("owner.stepup.confirm") }).click();
    await expect(d.getByText(t("owner.stepup.wrong"))).toBeVisible();
  }
  await d.getByLabel(t("owner.stepup.code")).fill(GOOD);
  await d.getByRole("button", { name: t("owner.stepup.confirm") }).click();
  await expect(d.getByText(t("owner.stepup.paused", { minutes: 15 }))).toBeVisible();
  expect(calls.steps).toHaveLength(6);
  // Still signed in: nothing sent the owner to sign in again.
  expect(new URL(page.url()).pathname).toBe("/manage");
  await page.keyboard.press("Escape");
  await expect(dialog(page)).toBeHidden();
  await expect(rows(page)).toHaveCount(0);
});

test.describe("a sensitive change queued while offline", () => {
  const held = {
    key: "01HZZZZZZZZZZZZZZZZZZZZZZA",
    cafe: CAFE,
    branch_id: "b1",
    kind: SENSITIVE,
    payload: { anything: true },
    created_at: Date.now() - 60_000,
    state: "queued",
  };

  test("is held until the owner passes the code prompt from its Send button", async ({ page }) => {
    const calls = await cloud(page);
    await openManage(page);
    await seed(page, held);
    await page.reload();

    // The drain was refused once; the row waits with a Send button, and a reload does not post it.
    const row = rows(page).first();
    await expect(row.getByText(t("owner.cmd.state.queued"), { exact: true })).toBeVisible();
    await expect(row.getByRole("button", { name: t("owner.cmd.send") })).toBeVisible();
    await expect(page.locator('[data-status="local-held"]')).toHaveCount(1);
    expect((await stored(page))[0].state).toBe("held");
    expect(posts(calls)).toHaveLength(1);
    await page.reload();
    await expect(page.locator('[data-status="local-held"]')).toHaveCount(1);
    expect(posts(calls)).toHaveLength(1);
    await expect(dialog(page)).toHaveCount(0);

    // Cancel keeps it held and posts nothing.
    await rows(page)
      .first()
      .getByRole("button", { name: t("owner.cmd.send") })
      .click();
    await expect(dialog(page)).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(dialog(page)).toBeHidden();
    await expect(page.locator('[data-status="local-held"]')).toHaveCount(1);
    expect(posts(calls)).toHaveLength(1);
    expect((await stored(page))[0].state).toBe("held");

    // A correct code releases it under its own key.
    await rows(page)
      .first()
      .getByRole("button", { name: t("owner.cmd.send") })
      .click();
    await dialog(page).getByLabel(t("owner.stepup.code")).fill(GOOD);
    await dialog(page)
      .getByRole("button", { name: t("owner.stepup.confirm") })
      .click();
    await expect(dialog(page)).toBeHidden();
    await expect(page.locator('[data-status="pending"]')).toHaveCount(1);
    await expect(page.locator('[data-status="local-held"]')).toHaveCount(0);
    expect(await stored(page)).toEqual([]);
    const sent = posts(calls);
    expect(sent).toHaveLength(2);
    expect(sent[1].idempotency_key).toBe(held.key);
  });
});

test("the open prompt has no serious accessibility violations and reachable targets", async ({
  page,
}) => {
  await cloud(page);
  await openManage(page);
  await propose(page, draft(SENSITIVE));
  await preview(page)
    .getByRole("button", { name: t("owner.cmd.send") })
    .click();
  const d = dialog(page);
  await expect(d).toBeVisible();
  const small = await d.locator("button, input").evaluateAll((els) =>
    els
      .map((el) => [el.outerHTML.slice(0, 70), el.getBoundingClientRect()] as const)
      .filter(([, r]) => r.width > 0 && (r.width < 44 || r.height < 44))
      .map(([h, r]) => `${h} ${r.width}x${r.height}`),
  );
  expect(small, small.join("\n")).toEqual([]);
  const blocking = (await new AxeBuilder({ page }).analyze()).violations.filter(
    (v) => v.impact === "serious" || v.impact === "critical",
  );
  const report = blocking.map(
    (v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`,
  );
  expect(report, report.join("\n")).toEqual([]);
});
