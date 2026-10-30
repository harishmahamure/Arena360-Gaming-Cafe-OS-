import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Locator, type Page } from "@playwright/test";
import type { CatalogueDevice, CatalogueRow } from "@arena/ui/generated";
import { PAID, liveWith, stubCloud, t } from "./stub";

const ONE = [PAID[0]];
const TWO = [PAID[0], { ...PAID[0], id: "b3", name: "Pixel Arena Koramangala" }];
const B1 = PAID[0].name;
const GOOD = "123456";

const group = (
  id: string,
  name: string,
  kind: string,
  colour: string,
  sort_order: number,
  rev = 2,
): CatalogueRow => ({
  id,
  name,
  rev,
  detail: kind,
  fields: { name, group_kind: kind, colour, sort_order },
});

const device = (
  id: string,
  name: string,
  kind: string,
  group_id: string | undefined,
  more: Partial<CatalogueDevice> = {},
): CatalogueDevice => ({ id, name, kind, group_id, revoked: false, rev: 3, ...more });

const B1_CATALOGUE = {
  groups: [
    group("g-pc", "Row A", "pc", "teal", 1),
    group("g-pc2", "Row B", "pc", "coral", 2),
    group("g-con", "Consoles", "console", "orange", 3),
  ],
  devices: [
    device("d1", "PC-01", "pc", "g-pc"),
    device("d2", "PC-02", "pc", undefined),
    device("d3", "PS5-1", "console", "g-con"),
    device("d4", "PC-03", "pc", "g-pc", { revoked: true }),
  ],
};
const B3_CATALOGUE = {
  groups: [group("h-pc", "Row A", "pc", "teal", 1, 5)],
  devices: [device("e1", "Pi-TV-1", "console", undefined)],
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

/** The cloud with step-up: a revoke is refused `MFA_REQUIRED` until a code passes. */
async function open(page: Page, branches = ONE) {
  let proof = false;
  const calls = await stubCloud(page, branches, {
    live: {
      b1: liveWith("b1", B1_CATALOGUE),
      b3: liveWith("b3", B3_CATALOGUE, 600_000),
    },
    post: (body) =>
      body.kind === "device.revoke" && !proof
        ? { status: 403, body: { code: "MFA_REQUIRED", message: "Confirm it's you." } }
        : undefined,
  });
  await page.route("**/v1/session/stepup", (route) => {
    proof = true;
    return route.fulfill({ status: 204 });
  });
  await page.goto("/manage?area=devices");
  await expect(page.getByRole("heading", { name: t("owner.devices.title") })).toBeVisible();
  return calls;
}

test("Devices & groups is a Manage area in the URL", async ({ page }) => {
  await open(page);
  await expect(page).toHaveURL(/[?&]area=devices/);
  const areas = page.getByRole("tablist", { name: t("owner.manage.areas") });
  await expect(areas.getByRole("tab", { selected: true })).toHaveText(
    t("owner.manage.area.devices"),
  );
  await page.reload();
  await expect(row(page, "device", "PC-01")).toBeVisible();
});

test("groups show a swatch dot and devices their kind, group and revoked state", async ({
  page,
}) => {
  await open(page, TWO);
  await expect(page.getByText(`${B1} · Updated 2 min ago`)).toBeVisible();

  // "Row A" exists at both branches and is one item.
  await expect(page.locator('[data-component="pricing-row"][data-kind="group"]')).toHaveCount(3);
  const a = row(page, "group", "Row A");
  await expect(a.locator('[data-component="group-chip"] .dot.teal')).toHaveCount(1);
  await expect(row(page, "group", "Row B").locator(".dot.coral")).toHaveCount(1);

  const pc1 = row(page, "device", "PC-01");
  await expect(pc1).toContainText(`${B1} · ${t("owner.devices.kind.pc")}`);
  await expect(pc1.locator('[data-component="group-chip"]')).toContainText("Row A");
  await expect(row(page, "device", "PC-02")).toContainText(t("owner.devices.unassigned"));
  await expect(row(page, "device", "PS5-1")).toContainText(t("owner.devices.kind.console"));
  const gone = row(page, "device", "PC-03");
  await expect(gone.getByText(t("owner.devices.revoked"))).toBeVisible();
  // A revoked device can be neither moved nor revoked again.
  await expect(gone.getByRole("button")).toHaveCount(0);
  await expect(row(page, "device", "Pi-TV-1")).toContainText("Pixel Arena Koramangala");
});

test("a group edit sends group.upsert per branch with its own id and no step-up", async ({
  page,
}) => {
  const calls = await open(page, TWO);
  await row(page, "group", "Row A")
    .getByRole("button", { name: t("owner.devices.group.edit", { name: "Row A" }) })
    .click();
  const form = page.getByRole("form", { name: t("owner.devices.group.edit", { name: "Row A" }) });
  await form.getByLabel(t("owner.devices.group.name")).fill(" ");
  await formSend(form).click();
  await expect(form.getByText(t("owner.devices.invalid.name"))).toBeVisible();
  await expect(preview(page)).toHaveCount(0);

  await form.getByLabel(t("owner.devices.group.name")).fill("Row A");
  await form.getByRole("radio", { name: t("owner.devices.colour.lime") }).check();
  await formSend(form).click();
  await expect(preview(page).getByText(`${t("owner.cmd.kind.group")} · Row A`)).toBeVisible();
  await sendIn(preview(page)).click();
  await expect(dialog(page)).toHaveCount(0);
  await expect.poll(() => posts(calls).length).toBe(2);

  const sent = posts(calls).filter((s) => s.kind === "group.upsert");
  expect(sent.map((s) => [s.branch_id, s.payload.id, s.base_rev])).toEqual([
    ["b1", "g-pc", 2],
    ["b3", "h-pc", 5],
  ]);
  expect(sent[0].payload).toEqual({
    id: "g-pc",
    name: "Row A",
    group_kind: "pc",
    colour: "lime",
    sort_order: 1,
  });
});

test("a new group takes the next sort order and checks the name", async ({ page }) => {
  const calls = await open(page);
  await page.getByRole("button", { name: t("owner.devices.group.add"), exact: true }).click();
  const form = page.getByRole("form", { name: t("owner.devices.group.form.add") });
  await formSend(form).click();
  await expect(form.getByText(t("owner.devices.invalid.name"))).toBeVisible();
  await form.getByLabel(t("owner.devices.group.name")).fill("VR bay");
  await form.getByLabel(t("owner.devices.group.kind")).selectOption("vr");
  await form.getByRole("radio", { name: t("owner.devices.colour.silver") }).check();
  await formSend(form).click();
  await sendIn(preview(page)).click();
  await expect.poll(() => posts(calls).length).toBe(1);
  expect(posts(calls)[0]).toMatchObject({
    branch_id: "b1",
    kind: "group.upsert",
    payload: { name: "VR bay", group_kind: "vr", colour: "silver", sort_order: 4 },
  });
  expect(posts(calls)[0].payload).not.toHaveProperty("id");
});

test("moving a device offers the groups of its kind and sends device.assign without step-up", async ({
  page,
}) => {
  const calls = await open(page);
  await row(page, "device", "PC-02")
    .getByRole("button", { name: t("owner.devices.move", { name: "PC-02" }) })
    .click();
  const form = page.getByRole("form", { name: t("owner.devices.move", { name: "PC-02" }) });
  const select = form.getByLabel(t("owner.devices.move.group"));
  // A PC joins PC groups only.
  await expect(select.getByRole("option")).toHaveText([
    t("owner.devices.unassigned"),
    "Row A",
    "Row B",
  ]);
  await select.selectOption("g-pc2");
  await formSend(form).click();
  await expect(preview(page).getByText(`${t("owner.cmd.kind.assign")} · PC-02`)).toBeVisible();
  await sendIn(preview(page)).click();
  await expect.poll(() => posts(calls).length).toBe(1);
  expect(posts(calls)[0]).toMatchObject({
    branch_id: "b1",
    kind: "device.assign",
    payload: { device_id: "d2", group_id: "g-pc2" },
  });
  await expect(dialog(page)).toHaveCount(0);

  // Back to Unassigned sends a null group.
  await row(page, "device", "PC-01")
    .getByRole("button", { name: t("owner.devices.move", { name: "PC-01" }) })
    .click();
  const again = page.getByRole("form", { name: t("owner.devices.move", { name: "PC-01" }) });
  await again.getByLabel(t("owner.devices.move.group")).selectOption("");
  await formSend(again).click();
  await sendIn(preview(page)).click();
  await expect.poll(() => posts(calls).length).toBe(2);
  expect(posts(calls)[1].payload).toEqual({ device_id: "d1", group_id: null });
});

test("revoking needs a reason and a step-up code, then sends device.revoke to that branch only", async ({
  page,
}) => {
  const calls = await open(page, TWO);
  await row(page, "device", "Pi-TV-1")
    .getByRole("button", { name: t("owner.devices.revoke", { name: "Pi-TV-1" }) })
    .click();
  const form = page.getByRole("form", { name: t("owner.devices.revoke", { name: "Pi-TV-1" }) });
  await expect(
    form.getByText(t("owner.devices.revoke.warning", { name: "Pi-TV-1" })),
  ).toBeVisible();
  await formSend(form).click();
  await expect(form.getByText(t("owner.devices.invalid.reason"))).toBeVisible();
  await expect(preview(page)).toHaveCount(0);

  await form.getByLabel(t("owner.devices.reason")).selectOption("stolen");
  await formSend(form).click();
  const p = preview(page);
  await expect(p.getByText(`${t("owner.cmd.kind.revoke")} · Pi-TV-1`)).toBeVisible();
  await sendIn(p).click();
  const d = dialog(page);
  await expect(d).toBeVisible();
  expect(posts(calls)).toHaveLength(1);
  await d.getByLabel(t("owner.stepup.code")).fill(GOOD);
  await d.getByRole("button", { name: t("owner.stepup.confirm") }).click();
  await expect(d).toBeHidden();

  const sent = posts(calls).filter((s) => s.kind === "device.revoke");
  expect(sent).toHaveLength(2); // refused, then the same post again
  expect(sent[1].idempotency_key).toBe(sent[0].idempotency_key);
  expect(sent[0]).toMatchObject({
    branch_id: "b3",
    payload: { device_id: "e1", reason: "stolen" },
  });
  expect(sent[0]).not.toHaveProperty("base_rev");
});

test("the devices screen has no accessibility violations", async ({ page }) => {
  await open(page);
  await row(page, "device", "PC-01")
    .getByRole("button", { name: t("owner.devices.revoke", { name: "PC-01" }) })
    .click();
  const results = await new AxeBuilder({ page }).analyze();
  expect(results.violations).toEqual([]);
});
