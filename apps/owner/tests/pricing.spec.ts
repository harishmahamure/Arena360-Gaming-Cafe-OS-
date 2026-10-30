import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Locator, type Page } from "@playwright/test";
import type { CatalogueRow } from "@arena/ui/generated";
import { PAID, liveWith, stubCloud, t } from "./stub";

const ULID = /^[0-9A-HJKMNP-TV-Z]{26}$/;
const ONE = [PAID[0]];
const TWO = [PAID[0], { ...PAID[0], id: "b3", name: "Pixel Arena Koramangala" }];
const B1 = PAID[0].name;
const B3 = "Pixel Arena Koramangala";

/** A catalogue row the way the Hub publishes it. */
const row = (
  id: string,
  name: string,
  fields: Record<string, unknown> | undefined,
  more: Partial<CatalogueRow> = {},
): CatalogueRow => ({ id, name, rev: 1, active: true, ...(fields ? { fields } : {}), ...more });

const GROUPS_B1 = [
  row("g1", "Row A", undefined, { detail: "pc", active: undefined }),
  row("g2", "VR", undefined, { detail: "vr", active: undefined }),
];
const GROUPS_B3 = [row("h1", "row a", undefined, { detail: "pc", active: undefined })];

const FRIDAY_EVENING = row(
  "ru1",
  "Friday evening",
  {
    group_id: "g1",
    rule_kind: "weekly",
    days_mask: 16,
    start_min: 18 * 60,
    end_min: 22 * 60,
    adjust_kind: "percent_bp",
    adjust_value: -2000,
    priority: 0,
  },
  { rev: 3, detail: "weekly" },
);
const NIGHT_PASS = row(
  "p1",
  "Night pass",
  {
    plan_kind: "pass",
    price_minor: 90_000,
    validity_days: 30,
    window_start_min: 22 * 60,
    window_end_min: 6 * 60,
    group_ids: ["g1"],
  },
  { rev: 4, detail: "pass" },
);
const VR_WEEKEND = row(
  "ru2",
  "VR weekend",
  {
    group_id: "g2",
    rule_kind: "weekly",
    days_mask: 96,
    start_min: 600,
    end_min: 1200,
    adjust_kind: "override_minor",
    adjust_value: 15_000,
    priority: 2,
  },
  { rev: 2, detail: "weekly" },
);
const WELCOME = row(
  "c1",
  "WELCOME10",
  {
    percent_bp: 1000,
    min_bill_minor: 0,
    targets: [{ kind: "group", id: "g1" }],
  },
  { rev: 1 },
);
const RATE_A = row(
  "r1",
  "Row A",
  { group_id: "g1", price_per_hour_minor: 6000 },
  { rev: 2, detail: "6000" },
);
const RATE_OLD = row(
  "r0",
  "Row A",
  { group_id: "g1", price_per_hour_minor: 5000 },
  { rev: 1, detail: "5000", active: false },
);

const B1_CATALOGUE = {
  groups: GROUPS_B1,
  rates: [RATE_OLD, RATE_A],
  plans: [NIGHT_PASS],
  pricing_rules: [FRIDAY_EVENING, VR_WEEKEND],
  coupons: [WELCOME],
};
const B3_CATALOGUE = {
  groups: GROUPS_B3,
  rates: [row("r3", "Row A", { group_id: "h1", price_per_hour_minor: 7000 }, { detail: "7000" })],
  plans: [],
  pricing_rules: [],
  coupons: [],
};

const preview = (page: Page) => page.getByRole("region", { name: t("owner.cmd.preview.title") });
const sendIn = (where: Locator) =>
  where.getByRole("button", { name: t("owner.cmd.send"), exact: true });
const formSend = (form: Locator) =>
  form.getByRole("button", { name: t("owner.pricing.form.send"), exact: true });
const items = (page: Page, kind: string) =>
  page.locator(`[data-component="pricing-row"][data-kind="${kind}"]`);
const item = (page: Page, kind: string, name: string) =>
  items(page, kind).and(page.locator(`[data-name="${name}"]`));
const posts = (calls: { bodies: Record<string, unknown[]> }) =>
  (calls.bodies["POST /v1/commands"] ?? []) as {
    branch_id: string;
    idempotency_key: string;
    kind: string;
    payload: Record<string, unknown>;
    base_rev?: number;
  }[];

async function open(page: Page, branches = ONE, path = "/manage") {
  const calls = await stubCloud(page, branches, {
    live: {
      b1: liveWith("b1", B1_CATALOGUE),
      b3: liveWith("b3", B3_CATALOGUE, 600_000),
    },
  });
  await page.goto(path);
  await expect(page.getByRole("tablist", { name: t("owner.pricing.tabs") })).toBeVisible();
  return calls;
}

test("Pricing offers three tabs, keeps the selected one in the URL and shows each branch's data age", async ({
  page,
}) => {
  await open(page, TWO);
  const tabs = page.getByRole("tablist", { name: t("owner.pricing.tabs") });
  await expect(tabs.getByRole("tab")).toHaveText([
    t("owner.pricing.tab.plans"),
    t("owner.pricing.tab.rules"),
    t("owner.pricing.tab.coupons"),
  ]);
  await expect(page).toHaveURL(/[?&]tab=plans/);
  await expect(page.locator("[data-component='tab-control']")).toBeVisible();

  await expect(page.getByText(`${B1} · Updated 2 min ago`)).toBeVisible();
  await expect(page.getByText(`${B3} · Updated 10 min ago`)).toBeVisible();
  await expect(item(page, "rate", "Row A")).toContainText(`${B1}: ₹60.00 per hour`);
  await expect(item(page, "rate", "Row A")).toContainText(`${B3}: ₹70.00 per hour`);
  await expect(item(page, "plan", "Night pass")).toContainText(B1);

  await tabs.getByRole("tab", { name: t("owner.pricing.tab.rules") }).click();
  await expect(page).toHaveURL(/[?&]tab=rules/);
  await expect(items(page, "rule")).toHaveCount(2);
  await tabs.getByRole("tab", { name: t("owner.pricing.tab.coupons") }).click();
  await expect(page).toHaveURL(/[?&]tab=coupons/);
  await expect(item(page, "coupon", "WELCOME10")).toBeVisible();

  await page.reload();
  await expect(tabs.getByRole("tab", { selected: true })).toHaveText(
    t("owner.pricing.tab.coupons"),
  );
});

test("a link with ?tab= opens that tab; an unknown tab opens Plans & rates", async ({ page }) => {
  await open(page, ONE, "/manage?tab=rules");
  await expect(
    page
      .getByRole("tablist", { name: t("owner.pricing.tabs") })
      .getByRole("tab", { selected: true }),
  ).toHaveText(t("owner.pricing.tab.rules"));
  await page.goto("/manage?tab=nope");
  await expect(
    page
      .getByRole("tablist", { name: t("owner.pricing.tabs") })
      .getByRole("tab", { selected: true }),
  ).toHaveText(t("owner.pricing.tab.plans"));
  await expect(page).toHaveURL(/[?&]tab=plans/);
});

test("editing a rate previews rate.upsert with the row's base_rev, with no MFA prompt", async ({
  page,
}) => {
  const calls = await open(page);
  await item(page, "rate", "Row A")
    .getByRole("button", { name: t("hub.groups.rate_edit", { name: "Row A" }) })
    .click();
  const form = page.getByRole("form", { name: t("hub.groups.rate_edit", { name: "Row A" }) });
  const price = form.getByLabel(t("hub.groups.rate"));
  await expect(price).toHaveValue("60.00");
  await price.fill("80");
  await formSend(form).click();

  const p = preview(page);
  await expect(p).toBeVisible();
  await expect(p.getByText(`${t("owner.cmd.kind.rate")} · ${B1}`)).toBeVisible();
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(page.getByText(/authenticator|verification code/i)).toHaveCount(0);
  expect(posts(calls)).toHaveLength(0);
  await sendIn(p).click();

  await expect(p).toHaveCount(0);
  await expect(form).toHaveCount(0);
  const sent = posts(calls);
  expect(sent).toHaveLength(1);
  expect(sent[0]).toMatchObject({
    branch_id: "b1",
    kind: "rate.upsert",
    base_rev: 2,
    payload: { group_id: "g1", price_per_hour_minor: 8000 },
  });
  expect(sent[0].idempotency_key).toMatch(ULID);
  expect(Object.keys(sent[0].payload).sort()).toEqual(["group_id", "price_per_hour_minor"]);
  await expect(page.locator('[data-component="command-row"]')).toHaveCount(1);
});

test("a rate below 1 or not a number shows the Hub's copy and opens no preview", async ({
  page,
}) => {
  const calls = await open(page);
  await item(page, "rate", "Row A")
    .getByRole("button", { name: t("hub.groups.rate_edit", { name: "Row A" }) })
    .click();
  const form = page.getByRole("form", { name: t("hub.groups.rate_edit", { name: "Row A" }) });
  for (const bad of ["0", "abc", "", "10.555"]) {
    await form.getByLabel(t("hub.groups.rate")).fill(bad);
    await formSend(form).click();
    await expect(form.getByText(t("hub.groups.rate_invalid"))).toBeVisible();
    await expect(form.getByLabel(t("hub.groups.rate"))).toHaveAttribute("aria-invalid", "true");
    await expect(preview(page)).toHaveCount(0);
  }
  expect(calls.filter((c) => c === "POST /v1/commands/preview")).toHaveLength(0);
  expect(posts(calls)).toHaveLength(0);
});

test("a plan edit for All branches sends each branch its own ids; the branch without it chooses Create here", async ({
  page,
}) => {
  const calls = await open(page, TWO);
  await item(page, "plan", "Night pass")
    .getByRole("button", { name: t("owner.pricing.edit", { name: "Night pass" }) })
    .click();
  const form = page.getByRole("form", { name: t("hub.plans.edit", { name: "Night pass" }) });
  await expect(form.getByLabel(t("hub.plans.price"))).toHaveValue("900.00");
  await expect(form.getByRole("checkbox", { name: "Row A" })).toBeChecked();
  await form.getByLabel(t("hub.plans.price")).fill("950");
  await formSend(form).click();

  const p = preview(page);
  await expect(p.getByText(t("owner.cmd.scope.all"))).toBeVisible();
  await expect(p.getByText(t("owner.pricing.ask"))).toBeVisible();
  const send = sendIn(p);
  await expect(send).toHaveAttribute("aria-disabled", "true");
  await expect(p.getByText(t("owner.pricing.choose_first"))).toBeVisible();
  await send.click({ force: true });
  expect(posts(calls)).toHaveLength(0);

  await p
    .getByRole("group", { name: t("owner.pricing.ask.group", { branch: B3 }) })
    .getByRole("radio", { name: t("owner.pricing.ask.create") })
    .check();
  await expect(send).not.toHaveAttribute("aria-disabled", "true");
  await send.click();

  await expect(p).toHaveCount(0);
  const sent = posts(calls).sort((a, b) => a.branch_id.localeCompare(b.branch_id));
  expect(sent).toHaveLength(2);
  const terms = {
    plan_kind: "pass",
    name: "Night pass",
    price_minor: 95_000,
    validity_days: 30,
    window_start_min: 1320,
    window_end_min: 360,
    active: true,
  };
  expect(sent[0]).toMatchObject({ branch_id: "b1", kind: "plan.upsert", base_rev: 4 });
  expect(sent[0].payload).toEqual({ id: "p1", ...terms, group_ids: ["g1"] });
  expect(sent[1].branch_id).toBe("b3");
  expect(sent[1]).not.toHaveProperty("base_rev");
  expect(sent[1].payload).toEqual({ ...terms, group_ids: ["h1"] });
  expect(sent[0].idempotency_key).toMatch(ULID);
  expect(sent[1].idempotency_key).toMatch(ULID);
  expect(sent[0].idempotency_key).not.toBe(sent[1].idempotency_key);
  await expect(page.locator('[data-component="command-row"]')).toHaveCount(2);
});

test("Exclude leaves the branch out and sends the rest", async ({ page }) => {
  const calls = await open(page, TWO);
  await item(page, "plan", "Night pass")
    .getByRole("button", { name: t("owner.pricing.edit", { name: "Night pass" }) })
    .click();
  await formSend(
    page.getByRole("form", { name: t("hub.plans.edit", { name: "Night pass" }) }),
  ).click();
  const p = preview(page);
  await p
    .getByRole("group", { name: t("owner.pricing.ask.group", { branch: B3 }) })
    .getByRole("radio", { name: t("owner.pricing.ask.exclude") })
    .check();
  await sendIn(p).click();
  await expect(p).toHaveCount(0);
  const sent = posts(calls);
  expect(sent.map((s) => s.branch_id)).toEqual(["b1"]);
});

test("a branch without a group the rule names is left out with the reason shown", async ({
  page,
}) => {
  const calls = await open(page, TWO);
  await page.getByRole("tab", { name: t("owner.pricing.tab.rules") }).click();
  await item(page, "rule", "VR weekend")
    .getByRole("button", { name: t("owner.pricing.edit", { name: "VR weekend" }) })
    .click();
  const form = page.getByRole("form", { name: t("hub.rules.edit", { name: "VR weekend" }) });
  await expect(form.getByRole("radio", { name: "VR" })).toBeChecked();
  await formSend(form).click();

  const p = preview(page);
  const left = p.getByRole("listitem").filter({ hasText: B3 });
  await expect(
    left.getByText(
      t("owner.pricing.excluded.label", {
        reason: t("owner.pricing.excluded.missing", { name: "VR" }),
      }),
    ),
  ).toBeVisible();
  await expect(left.getByRole("radio")).toHaveCount(0);
  await expect(sendIn(p)).not.toHaveAttribute("aria-disabled", "true");
  await sendIn(p).click();
  await expect(p).toHaveCount(0);
  const sent = posts(calls);
  expect(sent).toHaveLength(1);
  expect(sent[0]).toMatchObject({ branch_id: "b1", kind: "rule.upsert", base_rev: 2 });
  expect(sent[0].payload).toEqual({
    id: "ru2",
    name: "VR weekend",
    group_id: "g2",
    rule_kind: "weekly",
    days_mask: 96,
    start_min: 600,
    end_min: 1200,
    adjust_kind: "override_minor",
    adjust_value: 15_000,
    priority: 2,
    active: true,
  });
});

test("a branch picked alone is edited with its own ids and no choice to make", async ({ page }) => {
  const calls = await open(page, TWO);
  await page.getByLabel(t("owner.pricing.branch")).selectOption("b3");
  await expect(items(page, "plan")).toHaveCount(0);
  await expect(item(page, "rate", "row a")).toContainText(`${B3}: ₹70.00 per hour`);
  await expect(item(page, "rate", "row a")).not.toContainText(B1);
  await item(page, "rate", "row a")
    .getByRole("button", { name: t("hub.groups.rate_edit", { name: "row a" }) })
    .click();
  const form = page.getByRole("form", { name: t("hub.groups.rate_edit", { name: "row a" }) });
  await form.getByLabel(t("hub.groups.rate")).fill("75.50");
  await formSend(form).click();
  const p = preview(page);
  await expect(p.getByText(`${t("owner.cmd.kind.rate")} · ${B3}`)).toBeVisible();
  await sendIn(p).click();
  await expect(p).toHaveCount(0);
  const sent = posts(calls);
  expect(sent).toHaveLength(1);
  expect(sent[0]).toMatchObject({
    branch_id: "b3",
    base_rev: 1,
    payload: { group_id: "h1", price_per_hour_minor: 7550 },
  });
});

test("an All-branches rate is matched by group name and sent to each branch", async ({ page }) => {
  const calls = await open(page, TWO);
  await item(page, "rate", "Row A")
    .getByRole("button", { name: t("hub.groups.rate_edit", { name: "Row A" }) })
    .click();
  const form = page.getByRole("form", { name: t("hub.groups.rate_edit", { name: "Row A" }) });
  await form.getByLabel(t("hub.groups.rate")).fill("100");
  await formSend(form).click();
  await sendIn(preview(page)).click();
  await expect(preview(page)).toHaveCount(0);
  const sent = posts(calls).sort((a, b) => a.branch_id.localeCompare(b.branch_id));
  expect(sent.map((s) => [s.branch_id, s.base_rev, s.payload])).toEqual([
    ["b1", 2, { group_id: "g1", price_per_hour_minor: 10_000 }],
    ["b3", 1, { group_id: "h1", price_per_hour_minor: 10_000 }],
  ]);
});

const friday = (hhmm: string) => `2026-10-30T${hhmm}:00+05:30`;

test("a rule row shows its schedule, branches and Active now", async ({ page }) => {
  await page.clock.setFixedTime(friday("19:00"));
  await open(page, ONE, "/manage?tab=rules");
  const r = item(page, "rule", "Friday evening");
  await expect(r.getByText("Fri, 18:00–22:00")).toBeVisible();
  await expect(r).toContainText(t("owner.pricing.branches", { names: B1 }));
  await expect(r.getByText(t("owner.pricing.rule.status.active"), { exact: true })).toBeVisible();
  await expect(r).toContainText("Row A");
  await expect(r).toContainText("−20%");
  const v = item(page, "rule", "VR weekend");
  await expect(v.getByText("Sat, Sun, 10:00–20:00")).toBeVisible();
  await expect(v).toContainText("₹150.00 per hour");
  await expect(
    v.getByText(t("owner.pricing.rule.status.scheduled"), { exact: true }),
  ).toBeVisible();
});

test("rule status: a window past midnight, a past date, a future date and an inactive rule", async ({
  page,
}) => {
  const night = row(
    "ru3",
    "Thursday night",
    {
      group_id: "g1",
      rule_kind: "weekly",
      days_mask: 8,
      start_min: 22 * 60,
      end_min: 2 * 60,
      adjust_kind: "percent_bp",
      adjust_value: -1000,
      priority: 0,
    },
    { detail: "weekly" },
  );
  const date = (id: string, name: string, from: string, to: string, active = true) =>
    row(
      id,
      name,
      {
        group_id: "g1",
        rule_kind: "date",
        date_from: from,
        date_to: to,
        adjust_kind: "percent_bp",
        adjust_value: 500,
        priority: 0,
      },
      { detail: "date", active },
    );
  await page.clock.setFixedTime(friday("01:00"));
  await stubCloud(page, ONE, {
    live: {
      b1: liveWith("b1", {
        ...B1_CATALOGUE,
        pricing_rules: [
          night,
          FRIDAY_EVENING,
          date("d1", "Diwali", "2026-10-20", "2026-10-21"),
          date("d2", "New year", "2026-12-31", "2026-12-31"),
          date("d3", "Today only", "2026-10-30", "2026-10-30"),
          date("d4", "Switched off", "2026-10-30", "2026-10-30", false),
        ],
      }),
    },
  });
  await page.goto("/manage?tab=rules");
  const status = (name: string, key: string) =>
    expect(
      item(page, "rule", name).getByText(t(`owner.pricing.rule.status.${key}` as never), {
        exact: true,
      }),
    ).toBeVisible();
  await status("Thursday night", "active");
  await status("Friday evening", "scheduled");
  await status("Diwali", "ended");
  await status("New year", "scheduled");
  await status("Today only", "active");
  await status("Switched off", "off");
  await expect(item(page, "rule", "New year").getByText("On 2026-12-31")).toBeVisible();
  await expect(
    item(page, "rule", "Diwali").getByText("From 2026-10-20 to 2026-10-21"),
  ).toBeVisible();
});

test("an older summary lists the row by name and hides Edit", async ({ page }) => {
  await stubCloud(page, ONE, {
    live: {
      b1: liveWith("b1", {
        groups: GROUPS_B1,
        rates: [row("r1", "Row A", undefined, { detail: "6000" })],
        plans: [row("p9", "Old pack", undefined, { detail: "pack" })],
        pricing_rules: [row("ru9", "Old rule", undefined, { detail: "weekly" })],
        coupons: [row("c9", "OLDCODE", undefined)],
      }),
    },
  });
  await page.goto("/manage");
  await expect(item(page, "plan", "Old pack")).toContainText(B1);
  await expect(page.getByRole("button", { name: /Old pack/ })).toHaveCount(0);
  await expect(item(page, "rate", "Row A")).toContainText(`${B1}: ₹60.00 per hour`);
  await expect(item(page, "rate", "Row A").getByRole("button")).toHaveCount(0);
  // The group with no rate yet can still be given one.
  await expect(item(page, "rate", "VR").getByRole("button")).toHaveCount(1);
  await expect(item(page, "rate", "VR")).toContainText(t("owner.pricing.rate.none"));
  await page.getByRole("tab", { name: t("owner.pricing.tab.rules") }).click();
  await expect(item(page, "rule", "Old rule")).toBeVisible();
  await expect(items(page, "rule").getByRole("button")).toHaveCount(0);
  await page.getByRole("tab", { name: t("owner.pricing.tab.coupons") }).click();
  await expect(item(page, "coupon", "OLDCODE")).toBeVisible();
  await expect(items(page, "coupon").getByRole("button")).toHaveCount(0);
});

test("a new weekly rule sends rule.upsert with the Hub's fields", async ({ page }) => {
  const calls = await open(page, ONE, "/manage?tab=rules");
  await page.getByRole("button", { name: t("hub.rules.add"), exact: true }).click();
  const form = page.getByRole("form", { name: t("hub.rules.add") });
  await formSend(form).click();
  for (const key of [
    "hub.rules.invalid.name",
    "hub.rules.invalid.group",
    "hub.rules.invalid.days",
    "hub.rules.invalid.window",
    "hub.rules.invalid.adjust",
  ] as const) {
    await expect(form.getByText(t(key), { exact: true })).toBeVisible();
  }
  await expect(preview(page)).toHaveCount(0);

  await form.getByLabel(t("hub.rules.name")).fill("Happy hour");
  await form.getByRole("radio", { name: "Row A" }).check();
  await form.getByRole("checkbox", { name: t("hub.rules.day.mon") }).check();
  await form.getByRole("checkbox", { name: t("hub.rules.day.tue") }).check();
  await form.getByLabel(t("hub.rules.start")).fill("16:00");
  await form.getByLabel(t("hub.rules.end")).fill("18:00");
  await form.getByLabel(t("hub.rules.percent")).fill("-20");
  await formSend(form).click();
  const p = preview(page);
  await expect(p.getByText(`${t("owner.cmd.kind.rule")} · Happy hour`)).toBeVisible();
  await sendIn(p).click();
  await expect(p).toHaveCount(0);
  const sent = posts(calls);
  expect(sent).toHaveLength(1);
  expect(sent[0].kind).toBe("rule.upsert");
  expect(sent[0]).not.toHaveProperty("base_rev");
  expect(sent[0].payload).toEqual({
    name: "Happy hour",
    group_id: "g1",
    rule_kind: "weekly",
    days_mask: 3,
    start_min: 960,
    end_min: 1080,
    adjust_kind: "percent_bp",
    adjust_value: -2000,
    priority: 0,
    active: true,
  });
});

test("a date rule checks its dates; a rule that already exists is not added twice", async ({
  page,
}) => {
  const calls = await open(page, ONE, "/manage?tab=rules");
  await page.getByRole("button", { name: t("hub.rules.add"), exact: true }).click();
  const form = page.getByRole("form", { name: t("hub.rules.add") });
  await form.getByRole("radio", { name: t("hub.rules.type.date") }).check();
  await form.getByLabel(t("hub.rules.name")).fill("friday evening");
  await form.getByRole("radio", { name: "Row A" }).check();
  await form.getByLabel(t("hub.rules.from")).fill("2026-11-10");
  await form.getByLabel(t("hub.rules.to")).fill("2026-11-09");
  await form.getByRole("radio", { name: t("hub.rules.adjust.override") }).check();
  await form.getByLabel(t("hub.rules.rate"), { exact: true }).fill("150");
  await formSend(form).click();
  await expect(form.getByText(t("hub.rules.invalid.dates"))).toBeVisible();
  await form.getByLabel(t("hub.rules.to")).fill("");
  await formSend(form).click();

  const p = preview(page);
  await expect(
    p.getByText(
      t("owner.pricing.excluded.label", {
        reason: t("owner.pricing.excluded.exists", { name: "Friday evening" }),
      }),
    ),
  ).toBeVisible();
  await expect(sendIn(p)).toHaveAttribute("aria-disabled", "true");
  await expect(p.getByText(t("owner.cmd.preview.none"), { exact: true }).first()).toBeVisible();
  expect(posts(calls)).toHaveLength(0);

  await form.getByLabel(t("hub.rules.name")).fill("Derby day");
  await formSend(form).click();
  await sendIn(preview(page)).click();
  await expect(preview(page)).toHaveCount(0);
  expect(posts(calls)[0].payload).toEqual({
    name: "Derby day",
    group_id: "g1",
    rule_kind: "date",
    date_from: "2026-11-10",
    date_to: "2026-11-10",
    adjust_kind: "override_minor",
    adjust_value: 15_000,
    priority: 0,
    active: true,
  });
});

test("a new coupon sends coupon.upsert with the code upper-cased", async ({ page }) => {
  const calls = await open(page, ONE, "/manage?tab=coupons");
  await page.getByRole("button", { name: t("hub.coupons.add"), exact: true }).click();
  const form = page.getByRole("form", { name: t("hub.coupons.add") });
  await form.getByLabel(t("hub.coupons.code")).fill("a");
  await form.getByLabel(t("hub.coupons.percent")).fill("120");
  await formSend(form).click();
  await expect(form.getByText(t("hub.coupons.invalid.code"))).toBeVisible();
  await expect(form.getByText(t("hub.coupons.invalid.percent"))).toBeVisible();
  await expect(preview(page)).toHaveCount(0);

  await form.getByLabel(t("hub.coupons.code")).fill(" summer-5 ");
  await form.getByLabel(t("hub.coupons.percent")).fill("12.5");
  await form.getByLabel(t("hub.coupons.limit")).fill("40");
  await form.getByLabel(t("hub.coupons.minimum")).fill("200");
  await form.getByLabel(t("hub.coupons.from")).fill("2026-11-01");
  await form.getByRole("checkbox", { name: "VR" }).check();
  await formSend(form).click();
  const p = preview(page);
  await expect(p.getByText(`${t("owner.cmd.kind.coupon")} · SUMMER-5`)).toBeVisible();
  await sendIn(p).click();
  await expect(p).toHaveCount(0);
  const sent = posts(calls);
  expect(sent[0].kind).toBe("coupon.upsert");
  expect(sent[0].payload).toEqual({
    code: "SUMMER-5",
    percent_bp: 1250,
    valid_from: "2026-11-01",
    usage_limit: 40,
    min_bill_minor: 20_000,
    targets: [{ kind: "group", id: "g2" }],
    active: true,
  });
});

test("editing a coupon prefills it and sends its id and base_rev; an amount coupon sends amount_minor", async ({
  page,
}) => {
  const calls = await open(page, ONE, "/manage?tab=coupons");
  const c = item(page, "coupon", "WELCOME10");
  await expect(c).toContainText("10% off");
  await c.getByRole("button", { name: t("hub.coupons.edit", { code: "WELCOME10" }) }).click();
  const form = page.getByRole("form", { name: t("hub.coupons.edit", { code: "WELCOME10" }) });
  await expect(form.getByLabel(t("hub.coupons.code"))).toHaveValue("WELCOME10");
  await expect(form.getByLabel(t("hub.coupons.percent"))).toHaveValue("10");
  await expect(form.getByRole("checkbox", { name: "Row A" })).toBeChecked();
  await form.getByRole("radio", { name: t("hub.coupons.kind.amount") }).check();
  await form.getByLabel(t("hub.coupons.amount")).fill("50");
  await formSend(form).click();
  await sendIn(preview(page)).click();
  await expect(preview(page)).toHaveCount(0);
  const sent = posts(calls);
  expect(sent[0]).toMatchObject({ branch_id: "b1", kind: "coupon.upsert", base_rev: 1 });
  expect(sent[0].payload).toEqual({
    id: "c1",
    code: "WELCOME10",
    amount_minor: 5000,
    min_bill_minor: 0,
    targets: [{ kind: "group", id: "g1" }],
    active: true,
  });
});

test("a new plan checks every field with the Hub's copy, then sends plan.upsert", async ({
  page,
}) => {
  const calls = await open(page);
  await page.getByRole("button", { name: t("hub.plans.add"), exact: true }).click();
  const form = page.getByRole("form", { name: t("hub.plans.add") });
  await formSend(form).click();
  for (const key of [
    "hub.plans.invalid.name",
    "hub.plans.invalid.price",
    "hub.plans.invalid.hours",
    "hub.plans.invalid.validity",
    "hub.plans.invalid.groups",
  ] as const) {
    await expect(form.getByText(t(key), { exact: true })).toBeVisible();
  }
  await expect(preview(page)).toHaveCount(0);

  await form.getByLabel(t("hub.plans.name")).fill("Five hours");
  await form.getByLabel(t("hub.plans.price")).fill("300.50");
  await form.getByLabel(t("hub.plans.hours")).fill("5");
  await form.getByLabel(t("hub.plans.validity")).fill("7");
  await form.getByRole("checkbox", { name: "Row A" }).check();
  await formSend(form).click();
  const p = preview(page);
  await sendIn(p).click();
  await expect(p).toHaveCount(0);
  const sent = posts(calls);
  expect(sent[0]).not.toHaveProperty("base_rev");
  expect(sent[0].payload).toEqual({
    plan_kind: "pack",
    name: "Five hours",
    price_minor: 30_050,
    minutes: 300,
    validity_days: 7,
    group_ids: ["g1"],
    active: true,
  });
});

test("a block and a pass need a window with a start and an end that differ", async ({ page }) => {
  await open(page);
  await page.getByRole("button", { name: t("hub.plans.add"), exact: true }).click();
  const form = page.getByRole("form", { name: t("hub.plans.add") });
  await form.getByRole("radio", { name: t("hub.plans.kind.block") }).check();
  await form.getByLabel(t("hub.plans.name")).fill("Late block");
  await form.getByLabel(t("hub.plans.price")).fill("200");
  await form.getByLabel(t("hub.plans.window_start")).fill("22:00");
  await form.getByLabel(t("hub.plans.window_end")).fill("22:00");
  await form.getByRole("checkbox", { name: "Row A" }).check();
  await formSend(form).click();
  await expect(form.getByText(t("hub.plans.invalid.window"))).toBeVisible();
  await form.getByLabel(t("hub.plans.window_end")).fill("06:00");
  await formSend(form).click();
  await expect(preview(page)).toBeVisible();
});

test("console and PC groups cannot share one plan", async ({ page }) => {
  await stubCloud(page, ONE, {
    live: {
      b1: liveWith("b1", {
        groups: [
          row("g1", "Row A", undefined, { detail: "pc", active: undefined }),
          row("g5", "PS5", undefined, { detail: "console", active: undefined }),
        ],
      }),
    },
  });
  await page.goto("/manage");
  await page.getByRole("button", { name: t("hub.plans.add"), exact: true }).click();
  const form = page.getByRole("form", { name: t("hub.plans.add") });
  await form.getByLabel(t("hub.plans.name")).fill("Mixed");
  await form.getByLabel(t("hub.plans.price")).fill("100");
  await form.getByLabel(t("hub.plans.hours")).fill("1");
  await form.getByLabel(t("hub.plans.validity")).fill("3");
  await form.getByRole("checkbox", { name: "Row A" }).check();
  await form.getByRole("checkbox", { name: "PS5" }).check();
  await formSend(form).click();
  await expect(form.getByText(t("hub.plans.kind_mismatch"))).toBeVisible();
  await expect(preview(page)).toHaveCount(0);
});

test("with no signal the per-branch changes are queued, one per branch", async ({ page }) => {
  const calls = await open(page, TWO);
  await item(page, "rate", "Row A")
    .getByRole("button", { name: t("hub.groups.rate_edit", { name: "Row A" }) })
    .click();
  const form = page.getByRole("form", { name: t("hub.groups.rate_edit", { name: "Row A" }) });
  await form.getByLabel(t("hub.groups.rate")).fill("90");
  await formSend(form).click();
  await expect(preview(page)).toBeVisible();
  await page.route("**/v1/commands", (route) =>
    route.request().method() === "POST" ? route.abort("internetdisconnected") : route.fallback(),
  );
  await sendIn(preview(page)).click();
  await expect(preview(page)).toHaveCount(0);
  await expect(page.getByText(t("owner.cmd.state.queued"))).toHaveCount(2);
  expect(posts(calls)).toHaveLength(0);
});

for (const theme of ["light", "dark"] as const) {
  test(`the form and the preview pass the accessibility checks (${theme})`, async ({ page }) => {
    await page.addInitScript((v) => localStorage.setItem("arena.theme", v), theme);
    await open(page, TWO, "/manage?tab=rules");
    await item(page, "rule", "VR weekend")
      .getByRole("button", { name: t("owner.pricing.edit", { name: "VR weekend" }) })
      .click();
    await formSend(
      page.getByRole("form", { name: t("hub.rules.edit", { name: "VR weekend" }) }),
    ).click();
    await expect(preview(page).locator("[data-excluded]")).toBeVisible();
    await page.evaluate(() => document.fonts.ready);
    const small = await page.locator('button, a, input, select, [role="tab"]').evaluateAll((els) =>
      els
        .map((el) => [el.outerHTML.slice(0, 70), el.getBoundingClientRect()] as const)
        .filter(([, r]) => r.width > 0 && (r.width < 44 || r.height < 44))
        .map(([h, r]) => `${h} ${r.width}x${r.height}`),
    );
    expect(small, small.join("\n")).toEqual([]);
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

test("a branch that has sent no prices yet says so", async ({ page }) => {
  await stubCloud(page, ONE, {
    live: { b1: { ...liveWith("b1", {}), summary: null, as_of: null } },
  });
  await page.goto("/manage");
  await expect(page.getByText(t("owner.pricing.age_none", { branch: B1 }))).toBeVisible();
  await expect(page.getByText(t("owner.pricing.rates.empty"))).toBeVisible();
});

test("when no read of a branch works and nothing is cached, Pricing says so and offers Retry", async ({
  page,
}) => {
  await stubCloud(page, ONE);
  let down = true;
  await page.route("**/v1/branches/b1/live", (route) =>
    down ? route.abort("internetdisconnected") : route.fallback(),
  );
  await page.goto("/manage");
  await expect(page.getByText(t("owner.unavailable"))).toBeVisible();
  down = false;
  await page.getByRole("button", { name: t("common.retry") }).click();
  await expect(page.getByText(t("owner.unavailable"))).toHaveCount(0);
  await expect(page.getByText(t("owner.pricing.rates.empty"))).toBeVisible();
});
