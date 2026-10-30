import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type { AuditItem, AuditSearchPage } from "@arena/ui/generated";
import { PAID, stubCloud, t } from "./stub";

const ONE = [PAID[0]];
const DUBAI = { ...PAID[0], id: "b3", name: "Pixel Arena Dubai", branch_code: "DXB" };
const TWO = [PAID[0], DUBAI];

type Call = { method: string; branch: string; build: boolean; query: string };
type Audit = NonNullable<Parameters<typeof stubCloud>[2]>["audit"];

const monthNow = () =>
  new Intl.DateTimeFormat("en-CA", {
    timeZone: "Asia/Kolkata",
    year: "numeric",
    month: "2-digit",
  })
    .format(Date.now())
    .slice(0, 7);

const item = (seq: number, over: Partial<AuditItem> = {}): AuditItem => ({
  epoch: "E1",
  seq,
  at: Date.parse("2026-10-20T06:30:00Z") - seq * 60_000,
  business_day: "2026-10-20",
  actor: { kind: "staff", id: "S1", name: "Meera" },
  elevated_by: null,
  module: "settings",
  intent_kind: "settings.set",
  intent_id: `I${seq}`,
  tbl: "settings",
  row_id: "currency",
  op: "update",
  ...over,
});

const pageOf = (items: AuditItem[], over: Partial<AuditSearchPage> = {}): AuditSearchPage => ({
  branch_id: "b1",
  month: monthNow(),
  built_at: Date.now() - 60_000,
  items,
  orphaned: [],
  next_cursor: null,
  ...over,
});

const building = (progress: number) => ({
  status: 202,
  body: { branch_id: "b1", month: monthNow(), state: "building", progress, code: "AUDIT_BUILDING" },
});
const failed = { status: 502, body: { code: "AUDIT_BUILD_FAILED", message: "" } };
const missing = { status: 404, body: { code: "NOT_FOUND", message: "" } };
const ok = (page: AuditSearchPage) => ({ status: 200, body: page });

const openAudit = async (page: Page) => {
  await page.goto("/reports");
  await page.getByRole("tab", { name: t("owner.reports.tab.audit") }).click();
};
const search = (page: Page) => page.getByRole("button", { name: t("owner.audit.search") }).click();
const calls = (log: { bodies: Record<string, unknown[]> }) => (log.bodies["audit"] ?? []) as Call[];
const searches = (log: { bodies: Record<string, unknown[]> }) =>
  calls(log).filter((c) => c.method === "GET");
const builds = (log: { bodies: Record<string, unknown[]> }) => calls(log).filter((c) => c.build);

test("the first search of a month shows Preparing audit… with progress, then the first page", async ({
  page,
}) => {
  let reads = 0;
  const audit: Audit = ({ method }) => {
    if (method === "POST") return building(5);
    reads++;
    if (reads === 1) return missing;
    if (reads === 2) return building(60);
    return ok(pageOf([item(2), item(1)]));
  };
  const log = await stubCloud(page, ONE, { audit });
  await openAudit(page);
  await search(page);

  const preparing = page.getByTestId("audit-preparing");
  await expect(preparing).toContainText(t("owner.audit.preparing"));
  await expect(preparing.getByRole("progressbar")).toBeVisible();
  await expect(page.getByRole("row", { name: /Meera/ })).toHaveCount(2, { timeout: 10_000 });
  await expect(preparing).toHaveCount(0);
  expect(builds(log)).toHaveLength(1);
  expect(builds(log)[0].branch).toBe("b1");
  expect(searches(log)[0].query).toContain(`month=${monthNow()}`);
  expect(searches(log)[0].query).toContain("q=recent");
});

test("a month that is ready answers at once, without the preparing state or a build", async ({
  page,
}) => {
  const log = await stubCloud(page, ONE, { audit: () => ok(pageOf([item(1)])) });
  await openAudit(page);
  await search(page);
  await expect(page.getByRole("row", { name: /Meera/ })).toHaveCount(1);
  await expect(page.getByTestId("audit-preparing")).toHaveCount(0);
  expect(builds(log)).toHaveLength(0);
  await expect(page.getByText(t("hub.audit.loaded_all"))).toBeVisible();
});

test("a failed build says so and Retry starts a new attempt", async ({ page }) => {
  let retried = false;
  const audit: Audit = ({ method }) => {
    if (method === "POST") {
      retried = true;
      return building(0);
    }
    return retried ? ok(pageOf([item(1)])) : failed;
  };
  const log = await stubCloud(page, ONE, { audit });
  await openAudit(page);
  await search(page);
  await expect(page.getByText(t("owner.audit.failed"))).toBeVisible();
  await page.getByRole("button", { name: t("common.retry") }).click();
  await expect(page.getByRole("row", { name: /Meera/ })).toHaveCount(1, { timeout: 10_000 });
  expect(builds(log)).toHaveLength(1);
});

test("rows show time, who, the action, the record and who approved; orphaned rows sit apart", async ({
  page,
}) => {
  const pin = item(3, {
    elevated_by: { kind: "pin", id: "S2", name: "Priya", mode: "hub" },
    intent_kind: "refund.create",
    module: "refund",
    tbl: "refunds",
    row_id: "R9",
    op: "insert",
  });
  const mfa = item(2, {
    actor: { kind: "owner", id: "O1", name: "hub.audit.actor.owner" },
    elevated_by: { kind: "mfa", id: "O1", name: "Asha", mode: "hub" },
  });
  const system = item(1, {
    actor: { kind: "system", id: "sys", name: "hub.audit.actor.system" },
    intent_kind: "odd.thing",
    module: "odd",
  });
  const lost = item(4, { epoch: "E0", intent_id: "L", row_id: "orphan-row" });
  await stubCloud(page, ONE, {
    audit: () => ok(pageOf([pin, mfa, system], { orphaned: [lost] })),
  });
  await openAudit(page);
  await search(page);

  const row = (seq: number) => page.locator(`tr[data-seq="${seq}"]`).first();
  await expect(row(3)).toContainText("Meera");
  await expect(row(3)).toContainText(t("hub.audit.intent.refund.create"));
  await expect(row(3)).toContainText(t("owner.audit.elevated.pin", { name: "Priya" }));
  await expect(row(3)).toContainText("refunds / R9");
  await expect(row(2)).toContainText(t("hub.audit.actor.owner"));
  await expect(row(2)).toContainText(t("owner.audit.elevated.mfa"));
  await expect(row(1)).toContainText(t("hub.audit.actor.system"));
  await expect(row(1)).toContainText("odd.thing");

  const orphans = page.getByTestId("audit-orphaned");
  await expect(orphans).toContainText(t("owner.audit.orphaned"));
  await expect(orphans).toContainText("orphan-row");
  await expect(page.getByTestId("audit-orphaned").locator("tr")).toHaveCount(2);
});

test("Load more continues from the cursor", async ({ page }) => {
  const log = await stubCloud(page, ONE, {
    audit: ({ query }) =>
      query.get("cursor")
        ? ok(pageOf([item(1, { row_id: "second-page" })]))
        : ok(pageOf([item(2)], { next_cursor: "c1" })),
  });
  await openAudit(page);
  await search(page);
  await expect(page.getByRole("row", { name: /Meera/ })).toHaveCount(1);
  await page.getByRole("button", { name: t("hub.audit.load_more") }).click();
  await expect(page.getByRole("row", { name: /Meera/ })).toHaveCount(2);
  await expect(page.getByText("second-page")).toBeVisible();
  expect(searches(log)[1].query).toContain("cursor=c1");
});

test("each filter asks one catalogue entry with only the parameters it takes", async ({ page }) => {
  const log = await stubCloud(page, TWO, { audit: () => ok(pageOf([item(1)])) });
  await openAudit(page);
  const by = page.getByLabel(t("owner.audit.by"));
  const last = () => new URLSearchParams(searches(log).at(-1)?.query);

  await by.selectOption("actor");
  await page.getByLabel(t("owner.audit.actor"), { exact: true }).selectOption("staff");
  await page.getByLabel(t("owner.audit.actor_id")).fill("S1");
  await search(page);
  await expect.poll(() => searches(log).length).toBe(1);
  expect(Object.fromEntries(last())).toEqual({
    month: monthNow(),
    q: "actor",
    actor_kind: "staff",
    actor_id: "S1",
  });

  await by.selectOption("module");
  await page.getByLabel(t("owner.audit.module"), { exact: true }).selectOption("stock");
  await page.getByLabel(t("owner.audit.branch")).selectOption("b3");
  await search(page);
  await expect.poll(() => searches(log).length).toBe(2);
  expect(Object.fromEntries(last())).toEqual({ month: monthNow(), q: "module", module: "stock" });
  expect(searches(log).at(-1)?.branch).toBe("b3");

  await by.selectOption("intent");
  await page.getByLabel(t("owner.audit.intent")).fill("Settings.Set");
  await search(page);
  await expect.poll(() => searches(log).length).toBe(3);
  expect(Object.fromEntries(last())).toEqual({
    month: monthNow(),
    q: "intent",
    intent: "settings.set",
  });

  await by.selectOption("player");
  await page.getByLabel(t("owner.audit.player")).fill("P-7");
  await search(page);
  await expect.poll(() => searches(log).length).toBe(4);
  expect(Object.fromEntries(last())).toEqual({
    month: monthNow(),
    q: "entity",
    tbl: "players",
    row_id: "P-7",
  });
});

test("a search that cannot be valid is stopped before it is sent", async ({ page }) => {
  const log = await stubCloud(page, ONE, { audit: () => ok(pageOf([])) });
  await openAudit(page);
  await page.getByLabel(t("owner.audit.by")).selectOption("intent");
  await page.getByLabel(t("owner.audit.intent")).fill("drop table;");
  await search(page);
  await expect(page.getByRole("alert")).toContainText(t("owner.audit.problem.intent"));
  await page.getByLabel(t("owner.audit.by")).selectOption("player");
  await search(page);
  await expect(page.getByRole("alert")).toContainText(t("owner.audit.problem.player"));
  expect(calls(log)).toHaveLength(0);
});

test("no matching change reads as empty, and a refused search names the cause", async ({
  page,
}) => {
  let rate = false;
  await stubCloud(page, ONE, {
    audit: () =>
      rate ? { status: 429, body: { code: "RATE_LIMITED", message: "" } } : ok(pageOf([])),
  });
  await openAudit(page);
  await search(page);
  await expect(page.getByText(t("hub.audit.empty"))).toBeVisible();
  rate = true;
  await search(page);
  await expect(page.getByRole("alert")).toContainText(t("owner.audit.error.rate_limited"));
});

test("Audit has no serious accessibility violations while preparing and with results", async ({
  page,
}) => {
  let ready = false;
  await stubCloud(page, ONE, {
    audit: () => (ready ? ok(pageOf([item(2), item(1)], { orphaned: [item(3)] })) : building(40)),
  });
  await openAudit(page);
  await search(page);
  await expect(page.getByTestId("audit-preparing")).toBeVisible();
  const blocking = async () =>
    (await new AxeBuilder({ page }).analyze()).violations
      .filter((v) => v.impact === "serious" || v.impact === "critical")
      .map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`);
  expect(await blocking()).toEqual([]);
  ready = true;
  await expect(page.getByRole("row", { name: /Meera/ })).toHaveCount(3, { timeout: 10_000 });
  expect(await blocking()).toEqual([]);
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth > document.documentElement.clientWidth,
    ),
  ).toBe(false);
});
