import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type { HubSnapshot, RuleView, SessionStaff, SnapshotGroup } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { ORIGIN } from "./origin";

const HUB = "http://127.0.0.1:7409";
const CORS = {
  "access-control-allow-origin": ORIGIN,
  "access-control-allow-methods": "GET, POST",
  "access-control-allow-headers": "authorization, content-type",
};
const STAFF: Record<string, SessionStaff> = {
  "manager-token": {
    staff_id: "01JMANAGER0000000000000000",
    display_name: "Manager",
    role_name: "Manager",
    permissions: ["catalog.edit"],
  },
  "counter-token": {
    staff_id: "01JCOUNTER0000000000000000",
    display_name: "Counter",
    role_name: "Counter",
    permissions: ["session.start"],
  },
};

type Payload = Record<string, unknown> & { kind: string };
type Hub = {
  groups: SnapshotGroup[];
  rules: RuleView[];
  intents: { payload: Payload; base_rev?: number; ticket?: string }[];
  elevations: { items: { intent_kind: string }[]; pin: string }[];
  stale: boolean;
};

const group = (id: string, name: string): SnapshotGroup => ({
  id,
  name,
  kind: "pc",
  colour: "teal",
  sort_order: 1,
  rev: 1,
});

const newHub = (): Hub => ({
  groups: [group("G1", "Basic"), group("G2", "VIP")],
  rules: [],
  intents: [],
  elevations: [],
  stale: false,
});

const happy = (over: Partial<RuleView> = {}): RuleView => ({
  id: "R1",
  name: "Happy hour",
  group_id: "G1",
  rule_kind: "weekly",
  days_mask: 31,
  start_min: 960,
  end_min: 1080,
  adjust_kind: "percent_bp",
  adjust_value: -2000,
  priority: 0,
  active: true,
  rev: 3,
  conflicts: [],
  ...over,
});

const snapshot = (hub: Hub): HubSnapshot => ({
  branch: { name: "Dev Branch", branch_code: "DEV01" },
  groups: hub.groups,
  zones: [],
  devices: [],
  locale: {
    currency: "INR",
    exponent: 2,
    symbol: "₹",
    symbol_position: "before",
    number_format: "indian",
    tz: "Asia/Kolkata",
    day_start_min: 360,
    fy_start_month: 4,
    pending: null,
  },
  ui: { idle_lock_minutes: 5, hc: false },
  connectivity: { state: "online", checked_at: null },
  lan: null,
  paired: true,
});

const json = (route: Route, status: number, body: unknown) =>
  route.fulfill({
    status,
    contentType: "application/json",
    headers: CORS,
    body: JSON.stringify(body),
  });

async function open(page: Page, hub: Hub, token = "manager-token") {
  await page.addInitScript((t) => sessionStorage.setItem("arena.hub.token", t), token);
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    const auth = (req.headers()["authorization"] ?? "").replace(/^Bearer /, "");
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot":
        return json(route, 200, snapshot(hub));
      case "/ui/me":
        return json(route, 200, STAFF[auth]);
      case "/ui/rules":
        return json(route, 200, hub.rules);
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: "retry: 300\n\n",
        });
      case "/ui/elevate": {
        const body = req.postDataJSON() as Hub["elevations"][number];
        hub.elevations.push(body);
        if (body.pin !== "2222") return json(route, 403, { code: "PIN_REJECTED", message: "no" });
        return json(route, 200, { ticket: "T1", items: body.items, expires_in_s: 60 });
      }
      case "/ui/intent": {
        const body = req.postDataJSON() as {
          intent: { payload: Payload; base_rev?: number };
          elevation_ticket?: string;
        };
        const { payload } = body.intent;
        hub.intents.push({
          payload,
          base_rev: body.intent.base_rev,
          ticket: body.elevation_ticket,
        });
        if (hub.stale) {
          return json(route, 409, { code: "STALE", message: "changed", field: "base_rev" });
        }
        const existing = hub.rules.find((r) => r.id === payload.id);
        const rule = {
          id: (payload.id as string | undefined) ?? `R${hub.rules.length + 1}`,
          name: payload.name as string,
          group_id: payload.group_id as string,
          rule_kind: payload.rule_kind as string,
          days_mask: payload.days_mask as number | undefined,
          start_min: payload.start_min as number | undefined,
          end_min: payload.end_min as number | undefined,
          date_from: payload.date_from as string | undefined,
          date_to: payload.date_to as string | undefined,
          adjust_kind: payload.adjust_kind as string,
          adjust_value: payload.adjust_value as number,
          priority: payload.priority as number,
          active: payload.active as boolean,
          rev: (existing?.rev ?? 0) + 1,
          conflicts: existing?.conflicts ?? [],
        };
        if (existing) Object.assign(existing, rule);
        else hub.rules.push(rule);
        return json(route, 200, { applied: { intent_id: "I", changes: [] } });
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/settings/rules");
  await expect(page.getByTestId("rule-form")).toBeVisible();
}

async function expectNoSeriousAxe(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  expect(bad.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual(
    [],
  );
}

const add = (page: Page) => page.getByRole("button", { name: en["hub.rules.add"] });
const day = (page: Page, name: string) => page.getByTestId("rule-days").getByLabel(name);
const groupRadio = (page: Page, name: string) =>
  page.getByTestId("rule-groups").getByRole("radio", { name });

async function fillWeekly(page: Page) {
  await page.getByLabel(en["hub.rules.name"]).fill("Happy hour");
  await groupRadio(page, "Basic").check();
  for (const d of ["mon", "tue", "wed", "thu", "fri"] as const) {
    await day(page, en[`hub.rules.day.${d}`]).check();
  }
  await page.getByLabel(en["hub.rules.start"]).fill("16:00");
  await page.getByLabel(en["hub.rules.end"]).fill("18:00");
  await page.getByLabel(en["hub.rules.percent"]).fill("-20");
}

test("a manager creates a weekly happy hour and a date rule", async ({ page }) => {
  const hub = newHub();
  await open(page, hub);
  await expectNoSeriousAxe(page);

  await fillWeekly(page);
  await add(page).click();
  await expect(page.getByTestId("rule-status")).toHaveText(en["hub.settings.saved"]);
  expect(hub.intents[0].payload).toEqual({
    kind: "rule.upsert",
    name: "Happy hour",
    group_id: "G1",
    rule_kind: "weekly",
    days_mask: 31,
    start_min: 960,
    end_min: 1080,
    adjust_kind: "percent_bp",
    adjust_value: -2000,
    priority: 0,
    active: true,
  });
  expect(hub.intents[0].base_rev).toBeUndefined();

  await page.getByRole("radio", { name: en["hub.rules.type.date"] }).check();
  await page.getByLabel(en["hub.rules.name"]).fill("Match day");
  await groupRadio(page, "VIP").check();
  await page.getByLabel(en["hub.rules.from"]).fill("2027-03-10");
  await page.getByLabel(en["hub.rules.to"]).fill("2027-03-12");
  await page.getByRole("radio", { name: en["hub.rules.adjust.override"] }).check();
  await page.getByLabel(en["hub.rules.rate"]).fill("150");
  await page.getByLabel(en["hub.rules.priority"]).fill("5");
  await add(page).click();
  expect(hub.intents[1].payload).toEqual({
    kind: "rule.upsert",
    name: "Match day",
    group_id: "G2",
    rule_kind: "date",
    date_from: "2027-03-10",
    date_to: "2027-03-12",
    adjust_kind: "override_minor",
    adjust_value: 15000,
    priority: 5,
    active: true,
  });
  await expect(page.getByTestId("rule-row")).toHaveCount(2);

  await page.getByRole("radio", { name: en["hub.rules.type.date"] }).check();
  await page.getByLabel(en["hub.rules.name"]).fill("Holi");
  await groupRadio(page, "Basic").check();
  await page.getByLabel(en["hub.rules.from"]).fill("2027-03-22");
  await page.getByLabel(en["hub.rules.percent"]).fill("10");
  await add(page).click();
  expect(hub.intents[2].payload).toMatchObject({ date_from: "2027-03-22", date_to: "2027-03-22" });

  const schedule = page.getByTestId("rule-schedule");
  await expect(schedule).toHaveCount(3);
  await expect(schedule.nth(0)).toHaveText("Mon, Tue, Wed, Thu, Fri, 16:00–18:00");
  await expect(schedule.nth(1)).toHaveText("From 2027-03-10 to 2027-03-12");
  await expect(schedule.nth(2)).toHaveText("On 2027-03-22");
  const adjust = page.getByTestId("rule-adjust");
  await expect(adjust.nth(0)).toHaveText("−20%");
  await expect(adjust.nth(1)).toHaveText("₹150.00 per hour");
  await expect(adjust.nth(2)).toHaveText("+10%");
  await expectNoSeriousAxe(page);
});

test("no days, an equal window, a discount past 100% and reversed dates send nothing", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub);
  await fillWeekly(page);
  for (const d of ["mon", "tue", "wed", "thu", "fri"] as const) {
    await day(page, en[`hub.rules.day.${d}`]).uncheck();
  }
  await add(page).click();
  await expect(page.getByText(en["hub.rules.invalid.days"])).toBeVisible();

  await day(page, en["hub.rules.day.sat"]).check();
  await page.getByLabel(en["hub.rules.end"]).fill("16:00");
  await add(page).click();
  await expect(page.getByText(en["hub.rules.invalid.window"])).toBeVisible();

  await page.getByLabel(en["hub.rules.end"]).fill("18:00");
  await page.getByLabel(en["hub.rules.percent"]).fill("-100.01");
  await add(page).click();
  await expect(page.getByText(en["hub.rules.invalid.adjust"])).toBeVisible();

  await page.getByLabel(en["hub.rules.percent"]).fill("-20");
  await groupRadio(page, "Basic").check();
  await page.getByLabel(en["hub.rules.name"]).fill("   ");
  await add(page).click();
  await expect(page.getByText(en["hub.rules.invalid.name"])).toBeVisible();

  await page.getByLabel(en["hub.rules.name"]).fill("Reversed");
  await page.getByRole("radio", { name: en["hub.rules.type.date"] }).check();
  await page.getByLabel(en["hub.rules.from"]).fill("2027-03-12");
  await page.getByLabel(en["hub.rules.to"]).fill("2027-03-10");
  await add(page).click();
  await expect(page.getByText(en["hub.rules.invalid.dates"])).toBeVisible();
  expect(hub.intents).toEqual([]);
  await expectNoSeriousAxe(page);
});

test("an overlapping rule is listed with a conflict flag naming the winner", async ({ page }) => {
  const hub = newHub();
  hub.rules = [
    happy({
      conflicts: [{ rule_id: "R2", name: "Early bird", other_wins: true }],
    }),
    happy({
      id: "R2",
      name: "Early bird",
      start_min: 1020,
      end_min: 1200,
      priority: 3,
      conflicts: [{ rule_id: "R1", name: "Happy hour", other_wins: false }],
    }),
  ];
  await open(page, hub);
  const flags = page.getByTestId("rule-conflicts");
  await expect(flags).toHaveCount(2);
  await expect(flags.nth(0)).toContainText("Conflicts with Early bird. Early bird wins.");
  await expect(flags.nth(1)).toContainText("Conflicts with Happy hour. This rule wins.");
  await expectNoSeriousAxe(page);
});

test("an edit carries base_rev, a stale edit reloads, and a rule can be switched off", async ({
  page,
}) => {
  const hub = newHub();
  hub.rules = [happy()];
  await open(page, hub);
  await page
    .getByRole("button", { name: en["hub.rules.edit"].replace("{name}", "Happy hour") })
    .click();
  await expect(page.getByLabel(en["hub.rules.percent"])).toHaveValue("-20");
  await expect(page.getByLabel(en["hub.rules.start"])).toHaveValue("16:00");
  await expect(day(page, en["hub.rules.day.fri"])).toBeChecked();
  await expect(day(page, en["hub.rules.day.sat"])).not.toBeChecked();

  hub.stale = true;
  await page.getByLabel(en["hub.rules.percent"]).fill("-25");
  await page.getByRole("button", { name: en["hub.rules.save"] }).click();
  await expect(page.getByTestId("rule-status")).toContainText(en["hub.rules.stale"]);
  expect(hub.intents[0].base_rev).toBe(3);

  hub.stale = false;
  await page
    .getByRole("button", { name: en["hub.rules.edit"].replace("{name}", "Happy hour") })
    .click();
  await page.getByLabel(en["hub.rules.active"]).uncheck();
  await page.getByRole("button", { name: en["hub.rules.save"] }).click();
  await expect(page.getByTestId("rule-status")).toHaveText(en["hub.settings.saved"]);
  expect(hub.intents.at(-1)?.payload).toMatchObject({ id: "R1", active: false });
  expect(hub.intents.at(-1)?.base_rev).toBe(3);
  await expect(page.getByTestId("rule-row")).toContainText(en["hub.rules.badge.inactive"]);
});

test("a Counter's rule change asks for a manager's PIN in place", async ({ page }) => {
  const hub = newHub();
  await open(page, hub, "counter-token");
  await fillWeekly(page);
  await add(page).click();
  await expect(page.getByTestId("rule-approval")).toBeVisible();
  expect(hub.intents).toEqual([]);
  await page.keyboard.type("2222");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("rule-status")).toHaveText(en["hub.settings.saved"]);
  expect(hub.elevations.at(-1)).toEqual({ items: [{ intent_kind: "rule.upsert" }], pin: "2222" });
  expect(hub.intents[0].ticket).toBe("T1");
});
