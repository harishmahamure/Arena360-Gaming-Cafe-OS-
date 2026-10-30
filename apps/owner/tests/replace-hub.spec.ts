import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import { PAID, stubCloud, t, type Branch } from "./stub";

const GOOD = "123456";
const dialog = (page: Page) => page.getByRole("dialog", { name: t("owner.stepup.title") });
const card = (page: Page) => page.getByRole("listitem").filter({ hasText: PAID[0].name }).first();

type Post = { branch_id: string; idempotency_key: string; kind: string; payload: unknown };

/** The cloud with step-up: `hub.revoke` is refused `MFA_REQUIRED` until a code passes, then the
 * branch has no Hub and the cloud answers the row already applied. */
async function open(page: Page, tier: Branch["tier"] = "basic") {
  let proof = false;
  const branches: Branch[] = [
    {
      ...PAID[0],
      tier,
      hub: { paired: true, platform: "windows", last_seen_at: Date.now() - 60_000 },
    },
  ];
  const posts: Post[] = [];
  const calls = await stubCloud(page, () => branches, {
    post: (body) => {
      posts.push(body as Post);
      if (body.kind !== "hub.revoke") return undefined;
      if (!proof)
        return { status: 403, body: { code: "MFA_REQUIRED", message: "Confirm it's you." } };
      branches[0] = { ...branches[0], hub: { paired: false } };
      return {
        status: 200,
        body: {
          commands: [
            {
              id: 9,
              branch_id: body.branch_id,
              kind: "hub.revoke",
              status: "applied",
              payload: {},
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
  return { calls, posts };
}

test("Replace Hub confirms, asks for a step-up code, revokes, then shows a new pairing code", async ({
  page,
}) => {
  const { calls, posts } = await open(page);
  await expect(card(page).getByRole("button", { name: t("owner.branches.pair") })).toHaveCount(0);
  await card(page)
    .getByRole("button", { name: t("owner.branches.replace") })
    .click();
  await expect(card(page).getByText(t("owner.branches.replace.warning"))).toBeVisible();
  expect(posts).toHaveLength(0);

  await card(page)
    .getByRole("button", { name: t("owner.branches.replace.confirm") })
    .click();
  const d = dialog(page);
  await expect(d).toBeVisible();
  await d.getByLabel(t("owner.stepup.code")).fill(GOOD);
  await d.getByRole("button", { name: t("owner.stepup.confirm") }).click();
  await expect(d).toBeHidden();

  const sent = posts.filter((p) => p.kind === "hub.revoke");
  expect(sent).toHaveLength(2); // refused, then the same post again
  expect(sent[1].idempotency_key).toBe(sent[0].idempotency_key);
  expect(sent[0]).toMatchObject({ branch_id: "b1", payload: {} });

  const row = card(page).locator('[data-component="command-row"]');
  await expect(row).toHaveAttribute("data-status", "applied");
  await expect(row).toContainText(t("owner.cmd.kind.hub_revoke"));
  await expect(page.getByTestId("pairing-code-value")).toContainText("ABCD");
  await expect(card(page).getByText(t("owner.branches.replace.done"))).toBeVisible();
  await expect(card(page).getByRole("button", { name: t("owner.branches.replace") })).toHaveCount(
    0,
  );
  expect(calls.filter((c) => c === "POST /v1/wizard/install-hub/code")).toHaveLength(1);
});

test("Replace Hub is offered on Free too, and Cancel at the code prompt revokes nothing", async ({
  page,
}) => {
  const { posts } = await open(page, "free");
  await card(page)
    .getByRole("button", { name: t("owner.branches.replace") })
    .click();
  await card(page)
    .getByRole("button", { name: t("owner.branches.replace.confirm") })
    .click();
  const d = dialog(page);
  await expect(d).toBeVisible();
  await d.getByRole("button", { name: t("owner.cmd.cancel") }).click();
  await expect(d).toBeHidden();
  await expect(card(page).getByText(t("owner.branches.replace.warning"))).toBeVisible();
  await expect(page.getByTestId("pairing-code-value")).toHaveCount(0);
  expect(posts.filter((p) => p.kind === "hub.revoke")).toHaveLength(1);
});

test("backing out of Replace Hub sends nothing, and the confirm step has no accessibility violations", async ({
  page,
}) => {
  const { posts } = await open(page);
  await card(page)
    .getByRole("button", { name: t("owner.branches.replace") })
    .click();
  const results = await new AxeBuilder({ page }).analyze();
  expect(results.violations).toEqual([]);
  await card(page)
    .getByRole("button", { name: t("owner.branches.cancel") })
    .click();
  await expect(card(page).getByText(t("owner.branches.replace.warning"))).toHaveCount(0);
  await expect(card(page).getByRole("button", { name: t("owner.branches.replace") })).toBeVisible();
  expect(posts).toHaveLength(0);
});
