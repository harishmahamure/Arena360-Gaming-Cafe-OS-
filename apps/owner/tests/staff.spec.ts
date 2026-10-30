import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Locator, type Page } from "@playwright/test";
import type { CatalogueRow, CatalogueStaff } from "@arena/ui/generated";
import { PAID, liveWith, stubCloud, t } from "./stub";

const ONE = [PAID[0]];
const TWO = [PAID[0], { ...PAID[0], id: "b3", name: "Pixel Arena Koramangala" }];
const B1 = PAID[0].name;
const B3 = "Pixel Arena Koramangala";
const GOOD = "123456";
const CODES: Record<string, string> = { b1: "K7Q49MZD", b3: "H3W8RT2N" };

const allow = (...permissions: string[]) => ({
  permissions: permissions.map((permission) => ({ permission, effect: "allow" })),
});

/** A role row the way the Hub publishes it. */
const role = (
  id: string,
  name: string,
  perms: string[],
  more: Partial<CatalogueRow> = {},
): CatalogueRow => ({ id, name, rev: 2, fields: allow(...perms), ...more });

const member = (
  id: string,
  name: string,
  login: string,
  role_id: string,
  roleName: string,
  more: Partial<CatalogueStaff> = {},
): CatalogueStaff => ({ id, name, login, role_id, role: roleName, active: true, rev: 3, ...more });

const roles = (prefix: string) => [
  role(`${prefix}-owner`, "Owner", ["session.start", "user.manage"], { detail: "builtin" }),
  role(`${prefix}-mgr`, "Manager", ["session.start", "report.view"], { detail: "builtin" }),
  role(`${prefix}-ctr`, "Counter", ["session.start"], { detail: "builtin" }),
  role(`${prefix}-dev`, "Kiosk-device", [], { detail: "builtin" }),
  role(`${prefix}-lead`, "Floor lead", ["session.start", "session.end"], { rev: 4 }),
];

const B1_CATALOGUE = {
  roles: roles("b1"),
  staff: [
    member("s1", "Asha Rao", "+919800000001", "b1-owner", "Owner"),
    member("s2", "Ravi", "+919800000002", "b1-ctr", "Counter", { rev: 6 }),
    member("s3", "Meera", "+919800000003", "b1-ctr", "Counter", { active: false }),
  ],
};
const B3_CATALOGUE = {
  roles: roles("b3").filter((r) => r.name !== "Floor lead"),
  staff: [
    member("t1", "Ravi K", "+919800000002", "b3-ctr", "Counter", { rev: 9, active: false }),
    member("t2", "Dev", "+919800000004", "b3-mgr", "Manager"),
  ],
};

const preview = (page: Page) => page.getByRole("region", { name: t("owner.cmd.preview.title") });
const dialog = (page: Page) => page.getByRole("dialog", { name: t("owner.stepup.title") });
const sendIn = (where: Locator) =>
  where.getByRole("button", { name: t("owner.cmd.send"), exact: true });
const formSend = (form: Locator) =>
  form.getByRole("button", { name: t("owner.pricing.form.send"), exact: true });
const row = (page: Page, kind: string, name: string) =>
  page.locator(`[data-component="pricing-row"][data-kind="${kind}"][data-name="${name}"]`);
const posts = (calls: { bodies: Record<string, unknown[]> }) =>
  (calls.bodies["POST /v1/commands"] ?? []) as {
    branch_id: string;
    idempotency_key: string;
    kind: string;
    payload: Record<string, unknown>;
    base_rev?: number;
  }[];

/**
 * The cloud with step-up: staff and role changes are refused `MFA_REQUIRED` until a code passes;
 * a new member (or a asked-for code) answers with the branch's setup code.
 */
async function open(page: Page, branches = ONE) {
  let proof = false;
  let next = 100;
  const steps: unknown[] = [];
  const calls = await stubCloud(page, branches, {
    live: {
      b1: liveWith("b1", B1_CATALOGUE),
      b3: liveWith("b3", B3_CATALOGUE, 600_000),
    },
    post: (body) => {
      if (!proof) {
        return { status: 403, body: { code: "MFA_REQUIRED", message: "Confirm it's you." } };
      }
      const p = body.payload as Record<string, unknown>;
      if (body.kind === "staff.upsert" && (p.staff_id === undefined || p.setup_code === true)) {
        const id = String(p.staff_id ?? `new-${String(body.branch_id)}`);
        const branch = String(body.branch_id);
        return {
          status: 200,
          body: {
            commands: [
              {
                id: ++next,
                branch_id: branch,
                kind: body.kind,
                status: "pending",
                payload: { ...p, staff_id: id, setup_code: true },
                queued_at: Date.now(),
              },
            ],
            skipped: [],
            setup_codes: [{ branch_id: branch, staff_id: id, code: CODES[branch] }],
          },
        };
      }
      return undefined;
    },
  });
  await page.route("**/v1/session/stepup", (route) => {
    steps.push(route.request().postDataJSON());
    proof = true;
    return route.fulfill({ status: 204 });
  });
  await page.goto("/manage?area=staff");
  await expect(page.getByRole("heading", { name: t("owner.staff.title") })).toBeVisible();
  return Object.assign(calls, { steps });
}

/** Sends the preview; a sensitive change asks for a code first. */
async function sendWithCode(page: Page) {
  await sendIn(preview(page)).click();
  const d = dialog(page);
  await expect(d).toBeVisible();
  await d.getByLabel(t("owner.stepup.code")).fill(GOOD);
  await d.getByRole("button", { name: t("owner.stepup.confirm") }).click();
  await expect(d).toBeHidden();
}

test("Staff & roles is a Manage area in the URL", async ({ page }) => {
  await open(page);
  await expect(page).toHaveURL(/[?&]area=staff/);
  const areas = page.getByRole("tablist", { name: t("owner.manage.areas") });
  await expect(areas.getByRole("tab", { selected: true })).toHaveText(t("owner.manage.area.staff"));
  await page.reload();
  await expect(row(page, "staff", "Asha Rao")).toBeVisible();
});

test("staff are listed with number, role and active state per branch, and roles with their permissions", async ({
  page,
}) => {
  await open(page, TWO);
  await expect(page.getByText(`${B1} · Updated 2 min ago`)).toBeVisible();
  await expect(page.getByText(`${B3} · Updated 10 min ago`)).toBeVisible();

  const asha = row(page, "staff", "Asha Rao");
  await expect(asha).toContainText("+919800000001");
  await expect(asha).toContainText(t("owner.staff.at", { branch: B1, role: "Owner" }));

  // The same number at two branches is one person; an inactive row says so.
  const ravi = row(page, "staff", "Ravi");
  await expect(ravi).toContainText(t("owner.staff.at", { branch: B1, role: "Counter" }));
  await expect(ravi).toContainText(t("owner.staff.at", { branch: B3, role: "Counter" }));
  await expect(ravi.getByText(t("owner.staff.inactive"))).toHaveCount(1);
  await expect(row(page, "staff", "Meera").getByText(t("owner.staff.inactive"))).toBeVisible();
  await expect(page.locator('[data-component="pricing-row"][data-kind="staff"]')).toHaveCount(4);

  // The tenant's roles with their permissions; only the custom one is editable.
  const lead = row(page, "role", "Floor lead");
  await expect(lead).toContainText(t("owner.roles.count", { n: 2 }));
  await lead.getByText(t("owner.roles.show", { name: "Floor lead" })).click();
  await expect(lead.getByText("session.end")).toBeVisible();
  await expect(row(page, "role", "Owner")).toContainText(t("owner.roles.builtin"));
  await expect(
    row(page, "role", "Owner").getByRole("button", {
      name: t("owner.roles.edit", { name: "Owner" }),
    }),
  ).toHaveCount(0);
  await expect(
    lead.getByRole("button", { name: t("owner.roles.edit", { name: "Floor lead" }) }),
  ).toBeVisible();

  const search = page.getByLabel(t("owner.staff.search"));
  await search.fill("0004");
  await expect(page.locator('[data-component="pricing-row"][data-kind="staff"]')).toHaveCount(1);
  await expect(row(page, "staff", "Dev")).toBeVisible();
});

test("a new member asks for a code, sends no credential, and the setup code shows once", async ({
  page,
}) => {
  const calls = await open(page);
  await page.getByRole("button", { name: t("owner.staff.add"), exact: true }).click();
  const form = page.getByRole("form", { name: t("owner.staff.form.add") });
  await formSend(form).click();
  await expect(form.getByText(t("owner.staff.invalid.name"))).toBeVisible();
  await expect(form.getByText(t("owner.staff.invalid.phone"))).toBeVisible();
  await expect(form.getByText(t("owner.staff.invalid.role"))).toBeVisible();
  await expect(preview(page)).toHaveCount(0);

  await form.getByLabel(t("owner.staff.name")).fill(" Neha Rao ");
  await form.getByLabel(t("owner.staff.phone")).fill("98765 43210");
  await formSend(form).click();
  await expect(form.getByText(t("owner.staff.invalid.phone"))).toBeVisible();
  await form.getByLabel(t("owner.staff.phone")).fill("+91 98765 43210");
  await form.getByLabel(t("owner.staff.role")).selectOption("Counter");
  // The device role is never offered for a person.
  await expect(form.getByRole("option", { name: "Kiosk-device" })).toHaveCount(0);
  await formSend(form).click();

  await expect(preview(page).getByText(`${t("owner.cmd.kind.staff")} · Neha Rao`)).toBeVisible();
  expect(posts(calls)).toHaveLength(0);
  await sendWithCode(page);

  const sent = posts(calls);
  expect(sent).toHaveLength(2);
  expect(sent[1].idempotency_key).toBe(sent[0].idempotency_key);
  expect(sent[0]).toMatchObject({ branch_id: "b1", kind: "staff.upsert" });
  expect(sent[0].payload).toEqual({
    login: "+919876543210",
    display_name: "Neha Rao",
    role_id: "b1-ctr",
    active: true,
  });
  expect(sent[0]).not.toHaveProperty("base_rev");
  expect(JSON.stringify(sent)).not.toMatch(/password|pin|setup_code_hash|K7Q4/i);

  const codes = page.locator('[data-component="setup-codes"]');
  await expect(codes).toContainText(
    t("owner.staff.code.shown", { name: "Neha Rao", code: "K7Q4 9MZD" }),
  );
  await expect(codes).toContainText(B1);
  await codes.getByRole("button", { name: t("owner.staff.code.done") }).click();
  await expect(codes).toHaveCount(0);
  await page.reload();
  await expect(page.getByText(/K7Q4/)).toHaveCount(0);
});

test("a new member for All branches gets one code per branch and skips a branch that has the number", async ({
  page,
}) => {
  const calls = await open(page, TWO);
  await page.getByRole("button", { name: t("owner.staff.add"), exact: true }).click();
  const form = page.getByRole("form", { name: t("owner.staff.form.add") });
  await form.getByLabel(t("owner.staff.name")).fill("Neha Rao");
  await form.getByLabel(t("owner.staff.phone")).fill("+919876543210");
  await form.getByLabel(t("owner.staff.role")).selectOption("Manager");
  await formSend(form).click();
  await expect(preview(page).getByText(t("owner.cmd.scope.all"))).toBeVisible();
  await sendWithCode(page);

  const sent = posts(calls).filter((s) => s.payload.login);
  expect(
    sent.map((s) => [s.branch_id, s.payload.role_id]).sort((a, b) => a[0].localeCompare(b[0])),
  ).toEqual(
    expect.arrayContaining([
      ["b1", "b1-mgr"],
      ["b3", "b3-mgr"],
    ]),
  );
  const codes = page.locator('[data-component="setup-codes"]');
  await expect(codes.getByText(/Setup code for Neha Rao/)).toHaveCount(2);
  await expect(codes).toContainText("K7Q4 9MZD");
  await expect(codes).toContainText("H3W8 RT2N");

  // Ravi's number is already on the team at both branches.
  await codes.getByRole("button", { name: t("owner.staff.code.done") }).click();
  await page.getByRole("button", { name: t("owner.staff.add"), exact: true }).click();
  const again = page.getByRole("form", { name: t("owner.staff.form.add") });
  await again.getByLabel(t("owner.staff.name")).fill("Ravi again");
  await again.getByLabel(t("owner.staff.phone")).fill("+919800000002");
  await again.getByLabel(t("owner.staff.role")).selectOption("Counter");
  await formSend(again).click();
  await expect(preview(page).locator("[data-excluded]")).toHaveText([
    t("owner.pricing.excluded.label", {
      reason: t("owner.staff.excluded.exists", { name: "Ravi" }),
    }),
    t("owner.pricing.excluded.label", {
      reason: t("owner.staff.excluded.exists", { name: "Ravi K" }),
    }),
  ]);
  await expect(sendIn(preview(page))).toBeDisabled();
});

test("editing a member changes the role at each branch with its own base_rev and no new code", async ({
  page,
}) => {
  const calls = await open(page, TWO);
  await row(page, "staff", "Ravi")
    .getByRole("button", { name: t("owner.staff.edit", { name: "Ravi" }) })
    .click();
  const form = page.getByRole("form", { name: t("owner.staff.edit", { name: "Ravi" }) });
  await expect(form.getByText(t("owner.staff.phone_is", { phone: "+919800000002" }))).toBeVisible();
  await form.getByLabel(t("owner.staff.role")).selectOption("Floor lead");
  await formSend(form).click();

  // Floor lead exists at b1 only; b3 has none and says why.
  const p = preview(page);
  await expect(p.locator("[data-excluded]")).toHaveText([
    t("owner.pricing.excluded.label", {
      reason: t("owner.staff.excluded.role", { name: "Floor lead" }),
    }),
  ]);
  await sendWithCode(page);
  const sent = posts(calls).filter((s) => s.payload.staff_id);
  expect(sent).toHaveLength(2);
  expect(sent[0]).toMatchObject({ branch_id: "b1", kind: "staff.upsert", base_rev: 6 });
  expect(sent[0].payload).toEqual({
    staff_id: "s2",
    login: "+919800000002",
    display_name: "Ravi",
    role_id: "b1-lead",
    active: true,
  });
  await expect(page.locator('[data-component="setup-codes"]')).toHaveCount(0);
});

test("a new setup code can be asked for when editing, and shows once", async ({ page }) => {
  const calls = await open(page);
  await row(page, "staff", "Ravi")
    .getByRole("button", { name: t("owner.staff.edit", { name: "Ravi" }) })
    .click();
  const form = page.getByRole("form", { name: t("owner.staff.edit", { name: "Ravi" }) });
  await form.getByRole("checkbox", { name: t("owner.staff.new_code") }).check();
  await formSend(form).click();
  await sendWithCode(page);
  expect(posts(calls)[0].payload).toMatchObject({ staff_id: "s2", setup_code: true });
  await expect(page.locator('[data-component="setup-codes"]')).toContainText(
    t("owner.staff.code.shown", { name: "Ravi", code: "K7Q4 9MZD" }),
  );
});

test("deactivating sends staff.deactivate where the person is active and leaves the others out", async ({
  page,
}) => {
  const calls = await open(page, TWO);
  await row(page, "staff", "Ravi")
    .getByRole("button", { name: t("owner.staff.deactivate", { name: "Ravi" }) })
    .click();
  const p = preview(page);
  await expect(p.getByText(`${t("owner.cmd.kind.staff_off")} · Ravi`)).toBeVisible();
  await expect(p.locator("[data-excluded]")).toHaveText([
    t("owner.pricing.excluded.label", {
      reason: t("owner.staff.excluded.inactive", { name: "Ravi" }),
    }),
  ]);
  await sendWithCode(page);
  const sent = posts(calls).filter((s) => s.kind === "staff.deactivate");
  expect(sent).toHaveLength(2); // the refused post and the same one again
  expect(sent[0]).toMatchObject({ branch_id: "b1", payload: { staff_id: "s2" } });
  expect(sent[0]).not.toHaveProperty("base_rev");

  // A person who is inactive everywhere has nothing to deactivate.
  await expect(
    row(page, "staff", "Meera").getByRole("button", {
      name: t("owner.staff.deactivate", { name: "Meera" }),
    }),
  ).toHaveCount(0);
});

test("a custom role sends only catalogue permissions with its base_rev; built-in names are refused", async ({
  page,
}) => {
  const calls = await open(page);
  await row(page, "role", "Floor lead")
    .getByRole("button", { name: t("owner.roles.edit", { name: "Floor lead" }) })
    .click();
  const form = page.getByRole("form", { name: t("owner.roles.edit", { name: "Floor lead" }) });
  await expect(form.getByRole("checkbox", { name: "session.start" })).toBeChecked();
  await expect(form.getByRole("checkbox", { name: "session.end" })).toBeChecked();
  await expect(form.getByRole("checkbox", { name: "user.manage" })).not.toBeChecked();
  await expect(form.getByRole("checkbox")).toHaveCount(37);
  await form.getByRole("checkbox", { name: "session.end" }).uncheck();
  await form.getByRole("checkbox", { name: "report.view" }).check();

  await form.getByLabel(t("owner.roles.name")).fill("Manager");
  await formSend(form).click();
  await expect(form.getByText(t("owner.roles.invalid.builtin"))).toBeVisible();
  await expect(preview(page)).toHaveCount(0);

  await form.getByLabel(t("owner.roles.name")).fill("Floor lead");
  await formSend(form).click();
  await expect(preview(page).getByText(`${t("owner.cmd.kind.role")} · Floor lead`)).toBeVisible();
  await sendWithCode(page);
  const sent = posts(calls)[0];
  expect(sent).toMatchObject({ branch_id: "b1", kind: "role.upsert", base_rev: 4 });
  expect(sent.payload).toEqual({
    role_id: "b1-lead",
    name: "Floor lead",
    perms: [
      { permission: "session.start", effect: "allow" },
      { permission: "report.view", effect: "allow" },
    ],
  });
});

test("a new role is added without an id and the permission list is the whole catalogue", async ({
  page,
}) => {
  const calls = await open(page);
  await page.getByRole("button", { name: t("owner.roles.add"), exact: true }).click();
  const form = page.getByRole("form", { name: t("owner.roles.form.add") });
  await formSend(form).click();
  await expect(form.getByText(t("owner.roles.invalid.name"))).toBeVisible();
  await form.getByLabel(t("owner.roles.name")).fill("Night shift");
  await form.getByRole("checkbox", { name: "pos.sell" }).check();
  await formSend(form).click();
  await sendWithCode(page);
  const sent = posts(calls)[0];
  expect(sent).toMatchObject({ branch_id: "b1", kind: "role.upsert" });
  expect(sent).not.toHaveProperty("base_rev");
  expect(sent.payload).toEqual({
    name: "Night shift",
    perms: [{ permission: "pos.sell", effect: "allow" }],
  });
});

test("the staff area has no accessibility violations", async ({ page }) => {
  await open(page, TWO);
  const results = await new AxeBuilder({ page }).analyze();
  expect(results.violations).toEqual([]);
});
