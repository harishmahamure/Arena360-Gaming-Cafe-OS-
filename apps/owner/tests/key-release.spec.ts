import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import { PAID, stubCloud, t, type Branch } from "./stub";

// Story 9.10: a replacement Hub asks for the branch key; the owner sees "Release branch key to
// <PC name>" on the branch card and approves or denies it with a step-up code (`backup.restore`).

const GOOD = "123456";
const dialog = (page: Page) => page.getByRole("dialog", { name: t("owner.stepup.title") });
const card = (page: Page) => page.getByRole("listitem").filter({ hasText: PAID[0].name }).first();
const prompt = (page: Page) => card(page).getByTestId("key-request");

type Post = {
  branch_id: string;
  idempotency_key: string;
  kind: string;
  payload: { request_id?: string; decision?: string };
};

/** The cloud with step-up: `backup.restore` is refused `MFA_REQUIRED` until a code passes, then the
 * request is decided and the branch no longer lists it. */
async function open(page: Page, pcName: string | null = "FRONT-DESK", refuse?: string) {
  let proof = false;
  const branches: Branch[] = [
    {
      ...PAID[0],
      hub: { paired: true, platform: "windows", last_seen_at: Date.now() - 60_000 },
      key_request: {
        id: "R1",
        pc_name: pcName ?? undefined,
        requested_at: Date.now() - 2 * 60_000,
      },
    },
  ];
  const posts: Post[] = [];
  await stubCloud(page, () => branches, {
    post: (body) => {
      posts.push(body as Post);
      if (body.kind !== "backup.restore") return undefined;
      if (!proof)
        return { status: 403, body: { code: "MFA_REQUIRED", message: "Confirm it's you." } };
      if (refuse) return { status: 409, body: { code: refuse, message: "No." } };
      delete branches[0].key_request;
      return {
        status: 200,
        body: {
          commands: [
            {
              id: 11,
              branch_id: body.branch_id,
              kind: "backup.restore",
              status: "applied",
              payload: body.payload,
              queued_at: Date.now(),
              done_at: Date.now(),
            },
          ],
          skipped: [],
        },
      };
    },
  });
  await page.route("**/v1/session/stepup", (route) => {
    proof = true;
    return route.fulfill({ status: 204 });
  });
  await page.goto("/account/branches");
  return { posts };
}

async function passStepUp(page: Page) {
  const d = dialog(page);
  await expect(d).toBeVisible();
  await d.getByLabel(t("owner.stepup.code")).fill(GOOD);
  await d.getByRole("button", { name: t("owner.stepup.confirm") }).click();
  await expect(d).toBeHidden();
}

test("the branch card offers to release the key to the PC that asked, with Approve and Deny", async ({
  page,
}) => {
  await open(page);
  const ask = prompt(page);
  await expect(ask).toContainText(t("owner.branches.key.title", { pc: "FRONT-DESK" }));
  await expect(ask).toContainText(t("owner.branches.key.asked", { age: "2 min ago" }));
  await expect(ask.getByRole("button", { name: t("owner.branches.key.approve") })).toBeVisible();
  await expect(ask.getByRole("button", { name: t("owner.branches.key.deny") })).toBeVisible();
  const results = await new AxeBuilder({ page }).analyze();
  expect(results.violations).toEqual([]);
});

test("a request without a PC name says the new PC", async ({ page }) => {
  await open(page, null);
  await expect(prompt(page)).toContainText(
    t("owner.branches.key.title", { pc: t("owner.branches.key.pc_fallback") }),
  );
});

test("Approve asks for a step-up code, sends backup.restore once and shows the applied row", async ({
  page,
}) => {
  const { posts } = await open(page);
  await prompt(page)
    .getByRole("button", { name: t("owner.branches.key.approve") })
    .click();
  await passStepUp(page);

  const sent = posts.filter((p) => p.kind === "backup.restore");
  expect(sent).toHaveLength(2); // refused, then the same post again
  expect(sent[1].idempotency_key).toBe(sent[0].idempotency_key);
  expect(sent[0]).toMatchObject({
    branch_id: "b1",
    payload: { request_id: "R1", decision: "approve" },
  });
  const row = card(page).locator('[data-component="command-row"]');
  await expect(row).toHaveAttribute("data-status", "applied");
  await expect(row).toContainText(t("owner.cmd.kind.key_release"));
  await expect(prompt(page)).toHaveCount(0);
});

test("Deny sends the denial and the prompt goes", async ({ page }) => {
  const { posts } = await open(page);
  await prompt(page)
    .getByRole("button", { name: t("owner.branches.key.deny") })
    .click();
  await passStepUp(page);
  const sent = posts.filter((p) => p.kind === "backup.restore");
  expect(sent[0]).toMatchObject({ payload: { request_id: "R1", decision: "deny" } });
  await expect(card(page).locator('[data-component="command-row"]')).toHaveAttribute(
    "data-status",
    "applied",
  );
  await expect(prompt(page)).toHaveCount(0);
});

test("Cancel at the code prompt sends no decision and keeps the prompt", async ({ page }) => {
  const { posts } = await open(page);
  await prompt(page)
    .getByRole("button", { name: t("owner.branches.key.approve") })
    .click();
  const d = dialog(page);
  await expect(d).toBeVisible();
  await d.getByRole("button", { name: t("owner.cmd.cancel") }).click();
  await expect(d).toBeHidden();
  await expect(prompt(page)).toBeVisible();
  expect(posts.filter((p) => p.kind === "backup.restore")).toHaveLength(1);
});

test("a request the cloud will not approve any more says so", async ({ page }) => {
  await open(page, "FRONT-DESK", "KEY_ALREADY_RELEASED");
  await prompt(page)
    .getByRole("button", { name: t("owner.branches.key.approve") })
    .click();
  await passStepUp(page);
  await expect(prompt(page).getByRole("alert")).toHaveText(t("owner.branches.key.refused"));
});
