import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type { HubSnapshot, PlanView, SessionStaff, SnapshotGroup } from "@arena/ui/generated";
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
  plans: PlanView[];
  intents: { payload: Payload; base_rev?: number; ticket?: string }[];
  elevations: { items: { intent_kind: string }[]; pin: string }[];
  stale: boolean;
};

const group = (id: string, name: string, kind: string): SnapshotGroup => ({
  id,
  name,
  kind,
  colour: "teal",
  sort_order: 1,
  rev: 1,
});

const newHub = (): Hub => ({
  groups: [group("G1", "Basic", "pc"), group("G2", "VIP", "pc"), group("G3", "PS5", "console")],
  plans: [],
  intents: [],
  elevations: [],
  stale: false,
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
      case "/ui/plans":
        return json(route, 200, hub.plans);
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
        const ids = payload.group_ids as string[];
        const kinds = new Set(
          ids.map((id) => hub.groups.find((g) => g.id === id)!.kind === "console"),
        );
        if (kinds.size > 1) {
          return json(route, 422, {
            code: "GROUP_KIND_MISMATCH",
            message: "mix",
            field: "group_ids",
          });
        }
        const existing = hub.plans.find((p) => p.id === payload.id);
        const plan = {
          id: (payload.id as string | undefined) ?? `P${hub.plans.length + 1}`,
          plan_kind: payload.plan_kind as string,
          name: payload.name as string,
          price_minor: payload.price_minor as number,
          minutes: payload.minutes as number | undefined,
          validity_days: payload.validity_days as number | undefined,
          window_start_min: payload.window_start_min as number | undefined,
          window_end_min: payload.window_end_min as number | undefined,
          group_ids: ids,
          active: payload.active as boolean,
          rev: (existing?.rev ?? 0) + 1,
        };
        if (existing) Object.assign(existing, plan);
        else hub.plans.push(plan);
        return json(route, 200, { applied: { intent_id: "I", changes: [] } });
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/settings/plans");
  await expect(page.getByTestId("plan-form")).toBeVisible();
}

async function expectNoSeriousAxe(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  expect(bad.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual(
    [],
  );
}

const groupBox = (page: Page, name: string) => page.getByTestId("plan-groups").getByLabel(name);
const save = (page: Page) => page.getByRole("button", { name: en["hub.plans.add"] });

async function fillPack(page: Page) {
  await page.getByLabel(en["hub.plans.name"]).fill("5 h");
  await page.getByLabel(en["hub.plans.price"]).fill("300");
  await page.getByLabel(en["hub.plans.hours"]).fill("5");
  await page.getByLabel(en["hub.plans.validity"]).fill("7");
  await groupBox(page, "Basic").check();
}

test("a manager creates a pack, a block and a wrapping pass", async ({ page }) => {
  const hub = newHub();
  await open(page, hub);
  await expectNoSeriousAxe(page);

  await fillPack(page);
  await save(page).click();
  await expect(page.getByTestId("plan-status")).toHaveText(en["hub.settings.saved"]);
  expect(hub.intents[0].payload).toEqual({
    kind: "plan.upsert",
    plan_kind: "pack",
    name: "5 h",
    price_minor: 30000,
    minutes: 300,
    validity_days: 7,
    group_ids: ["G1"],
    active: true,
  });
  expect(hub.intents[0].base_rev).toBeUndefined();

  await page.getByRole("radio", { name: en["hub.plans.kind.block"] }).check();
  await page.getByLabel(en["hub.plans.name"]).fill("Evening block");
  await page.getByLabel(en["hub.plans.price"]).fill("200");
  await page.getByLabel(en["hub.plans.window_start"]).fill("18:00");
  await page.getByLabel(en["hub.plans.window_end"]).fill("23:30");
  await groupBox(page, "VIP").check();
  await save(page).click();
  expect(hub.intents[1].payload).toMatchObject({
    plan_kind: "block",
    window_start_min: 1080,
    window_end_min: 1410,
    group_ids: ["G2"],
  });
  expect(hub.intents[1].payload.minutes).toBeUndefined();
  expect(hub.intents[1].payload.validity_days).toBeUndefined();

  await page.getByRole("radio", { name: en["hub.plans.kind.pass"] }).check();
  await page.getByLabel(en["hub.plans.name"]).fill("Night pass");
  await page.getByLabel(en["hub.plans.price"]).fill("900");
  await page.getByLabel(en["hub.plans.validity"]).fill("30");
  await page.getByLabel(en["hub.plans.play_start"]).fill("22:00");
  await page.getByLabel(en["hub.plans.play_end"]).fill("06:00");
  await groupBox(page, "Basic").check();
  await groupBox(page, "VIP").check();
  await save(page).click();
  expect(hub.intents[2].payload).toMatchObject({
    plan_kind: "pass",
    validity_days: 30,
    window_start_min: 1320,
    window_end_min: 360,
    group_ids: ["G1", "G2"],
  });

  const terms = page.getByTestId("plan-terms");
  await expect(terms).toHaveCount(3);
  await expect(terms.nth(0)).toHaveText("5 h, valid 7 days");
  await expect(terms.nth(1)).toHaveText("Starts and ends within 18:00–23:30");
  await expect(terms.nth(2)).toHaveText("Plays 22:00–06:00, valid 30 days");
  await expect(page.getByTestId("plan-row").nth(0)).toContainText("₹300.00");
  await expectNoSeriousAxe(page);
});

test("zero price, zero hours, an equal window and no group show field errors and send nothing", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub);
  await fillPack(page);
  await page.getByLabel(en["hub.plans.price"]).fill("0");
  await save(page).click();
  await expect(page.getByText(en["hub.plans.invalid.price"])).toBeVisible();

  await page.getByLabel(en["hub.plans.price"]).fill("300");
  await page.getByLabel(en["hub.plans.hours"]).fill("0");
  await save(page).click();
  await expect(page.getByText(en["hub.plans.invalid.hours"])).toBeVisible();

  await page.getByLabel(en["hub.plans.hours"]).fill("5");
  await groupBox(page, "Basic").uncheck();
  await save(page).click();
  await expect(page.getByText(en["hub.plans.invalid.groups"])).toBeVisible();

  await page.getByRole("radio", { name: en["hub.plans.kind.block"] }).check();
  await groupBox(page, "Basic").check();
  await page.getByLabel(en["hub.plans.window_start"]).fill("10:00");
  await page.getByLabel(en["hub.plans.window_end"]).fill("10:00");
  await save(page).click();
  await expect(page.getByText(en["hub.plans.invalid.window"])).toBeVisible();
  expect(hub.intents).toEqual([]);
  await expectNoSeriousAxe(page);
});

test("a plan mixing console and PC groups is refused with a separate-plans message", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub);
  await fillPack(page);
  await groupBox(page, "PS5").check();
  await save(page).click();
  await expect(page.getByText(en["hub.plans.kind_mismatch"])).toBeVisible();
  expect(hub.plans).toEqual([]);

  await groupBox(page, "Basic").uncheck();
  await save(page).click();
  await expect(page.getByTestId("plan-status")).toHaveText(en["hub.settings.saved"]);
  expect(hub.plans[0].group_ids).toEqual(["G3"]);
});

test("an edit carries base_rev, a stale edit reloads, and a plan can be taken off sale", async ({
  page,
}) => {
  const hub = newHub();
  hub.plans = [
    {
      id: "P1",
      plan_kind: "pack",
      name: "5 h",
      price_minor: 30000,
      minutes: 300,
      validity_days: 7,
      group_ids: ["G1"],
      active: true,
      rev: 3,
    },
  ];
  await open(page, hub);
  await page.getByRole("button", { name: en["hub.plans.edit"].replace("{name}", "5 h") }).click();
  await expect(page.getByLabel(en["hub.plans.hours"])).toHaveValue("5");
  await expect(page.getByLabel(en["hub.plans.price"])).toHaveValue("300.00");
  await expect(page.getByRole("radio", { name: en["hub.plans.kind.pack"] })).toBeDisabled();

  hub.stale = true;
  await page.getByLabel(en["hub.plans.price"]).fill("320");
  await page.getByRole("button", { name: en["hub.plans.save"] }).click();
  await expect(page.getByTestId("plan-status")).toContainText(en["hub.plans.stale"]);
  expect(hub.intents[0].base_rev).toBe(3);

  hub.stale = false;
  await page.getByLabel(en["hub.plans.active"]).uncheck();
  await page.getByRole("button", { name: en["hub.plans.save"] }).click();
  await expect(page.getByTestId("plan-status")).toHaveText(en["hub.settings.saved"]);
  expect(hub.intents.at(-1)?.payload).toMatchObject({ id: "P1", active: false });
  expect(hub.intents.at(-1)?.base_rev).toBe(3);
  await expect(page.getByTestId("plan-row")).toContainText(en["hub.plans.badge.inactive"]);
});

test("a Counter's plan change asks for a manager's PIN in place", async ({ page }) => {
  const hub = newHub();
  await open(page, hub, "counter-token");
  await fillPack(page);
  await save(page).click();
  await expect(page.getByTestId("plan-approval")).toBeVisible();
  expect(hub.intents).toEqual([]);
  await page.keyboard.type("2222");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("plan-status")).toHaveText(en["hub.settings.saved"]);
  expect(hub.elevations.at(-1)).toEqual({ items: [{ intent_kind: "plan.upsert" }], pin: "2222" });
  expect(hub.intents[0].ticket).toBe("T1");
});
