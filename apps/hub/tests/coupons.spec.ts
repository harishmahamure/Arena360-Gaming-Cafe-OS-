import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type {
  CouponView,
  HubSnapshot,
  PlanView,
  Product,
  SessionStaff,
  SnapshotGroup,
} from "@arena/ui/generated";
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
  coupons: CouponView[];
  intents: { payload: Payload; base_rev?: number; ticket?: string }[];
  elevations: { items: { intent_kind: string }[]; pin: string }[];
  /** The field a save is refused on (`VALIDATION_FAILED`), if any. */
  refuse?: string;
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

const PLAN: PlanView = {
  id: "PL1",
  plan_kind: "pack",
  name: "5 hour pack",
  price_minor: 30000,
  minutes: 300,
  validity_days: 7,
  group_ids: ["G1"],
  active: true,
  rev: 1,
};

const COLA = {
  id: "P1",
  name: "Cola",
  category: "",
  price_minor: 4000,
  tax_rate_bp: 0,
  hsn: "",
  track_stock: false,
  image: "",
  barcode: "",
  kitchen: false,
  favourite: false,
  active: true,
  bundle: false,
  components: [],
  rev: 1,
  quantity: 0,
  avg_cost_minor: 0,
  available: true,
} satisfies Product;

const newHub = (): Hub => ({ coupons: [], intents: [], elevations: [], stale: false });

const diwali = (over: Partial<CouponView> = {}): CouponView => ({
  id: "C1",
  code: "DIWALI-10",
  percent_bp: 1000,
  valid_from: "2027-01-01",
  valid_to: "2027-02-01",
  usage_limit: 10,
  used: 3,
  min_bill_minor: 10000,
  targets: [{ kind: "product", id: "P1" }],
  active: true,
  rev: 5,
  ...over,
});

const snapshot = (): HubSnapshot => ({
  branch: { name: "Dev Branch", branch_code: "DEV01" },
  groups: [group("G1", "Basic"), group("G2", "VIP")],
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
        return json(route, 200, snapshot());
      case "/ui/me":
        return json(route, 200, STAFF[auth]);
      case "/ui/coupons":
        return json(route, 200, hub.coupons);
      case "/ui/plans":
        return json(route, 200, [PLAN]);
      case "/ui/products":
        return json(route, 200, [COLA]);
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
        if (hub.refuse) {
          return json(route, 422, {
            code: "VALIDATION_FAILED",
            message: "no",
            field: hub.refuse,
          });
        }
        if (hub.stale) {
          return json(route, 409, { code: "STALE", message: "changed", field: "base_rev" });
        }
        const existing = hub.coupons.find((c) => c.id === payload.id);
        const coupon: CouponView = {
          id: (payload.id as string | undefined) ?? `C${hub.coupons.length + 1}`,
          code: (payload.code as string).toUpperCase(),
          percent_bp: payload.percent_bp as number | undefined,
          amount_minor: payload.amount_minor as number | undefined,
          valid_from: payload.valid_from as string | undefined,
          valid_to: payload.valid_to as string | undefined,
          usage_limit: payload.usage_limit as number | undefined,
          used: existing?.used ?? 0,
          min_bill_minor: payload.min_bill_minor as number,
          targets: payload.targets as CouponView["targets"],
          active: payload.active as boolean,
          rev: (existing?.rev ?? 0) + 1,
        };
        if (existing) Object.assign(existing, coupon);
        else hub.coupons.push(coupon);
        return json(route, 200, { applied: { intent_id: "I", changes: [] } });
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/settings/coupons");
  await expect(page.getByTestId("coupon-form")).toBeVisible();
}

async function expectNoSeriousAxe(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  expect(bad.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual(
    [],
  );
}

const add = (page: Page) => page.getByRole("button", { name: en["hub.coupons.add"] });
const status = (page: Page) => page.getByTestId("coupon-status");

test("a manager creates a percent coupon with a window, a limit, a minimum and a target", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub);
  await expectNoSeriousAxe(page);

  await page.getByLabel(en["hub.coupons.code"]).fill(" diwali-10 ");
  await page.getByLabel(en["hub.coupons.percent"]).fill("12.5");
  await page.getByLabel(en["hub.coupons.from"]).fill("2027-01-01");
  await page.getByLabel(en["hub.coupons.to"]).fill("2027-02-01");
  await page.getByLabel(en["hub.coupons.limit"]).fill("10");
  await page.getByLabel(en["hub.coupons.minimum"]).fill("100");
  await page.getByTestId("coupon-targets").getByLabel("Cola").check();
  await add(page).click();
  await expect(status(page)).toHaveText(en["hub.settings.saved"]);
  expect(hub.intents[0].payload).toEqual({
    kind: "coupon.upsert",
    code: "DIWALI-10",
    percent_bp: 1250,
    valid_from: "2027-01-01",
    valid_to: "2027-02-01",
    usage_limit: 10,
    min_bill_minor: 10000,
    targets: [{ kind: "product", id: "P1" }],
    active: true,
  });
  expect(hub.intents[0].base_rev).toBeUndefined();

  const row = page.getByTestId("coupon-row");
  await expect(row).toHaveCount(1);
  await expect(page.getByTestId("coupon-code")).toHaveText("DIWALI-10");
  await expect(page.getByTestId("coupon-off")).toHaveText("12.5% off");
  await expect(page.getByTestId("coupon-window")).toHaveText("From 2027-01-01 to 2027-02-01");
  await expect(page.getByTestId("coupon-used")).toHaveText("0 of 10 used");
  await expect(row).toContainText("Bills of ₹100.00 or more");
  await expect(page.getByTestId("coupon-targets-text")).toHaveText("Cola");
  await expectNoSeriousAxe(page);
});

test("a fixed-amount coupon takes group and plan targets", async ({ page }) => {
  const hub = newHub();
  await open(page, hub);
  await page.getByLabel(en["hub.coupons.code"]).fill("flat50");
  await page.getByRole("radio", { name: en["hub.coupons.kind.amount"] }).check();
  await page.getByLabel(en["hub.coupons.amount"], { exact: true }).fill("50");
  const targets = page.getByTestId("coupon-targets");
  await targets.getByLabel("VIP").check();
  await targets.getByLabel("5 hour pack").check();
  await add(page).click();
  expect(hub.intents[0].payload).toEqual({
    kind: "coupon.upsert",
    code: "FLAT50",
    amount_minor: 5000,
    min_bill_minor: 0,
    targets: [
      { kind: "group", id: "G2" },
      { kind: "plan", id: "PL1" },
    ],
    active: true,
  });
  await expect(page.getByTestId("coupon-off")).toHaveText("₹50.00 off");
  await expect(page.getByTestId("coupon-targets-text")).toHaveText("VIP, 5 hour pack");
});

test("bad input shows its message in place and sends nothing", async ({ page }) => {
  const hub = newHub();
  await open(page, hub);
  await page.getByLabel(en["hub.coupons.code"]).fill("AB");
  await page.getByLabel(en["hub.coupons.percent"]).fill("10");
  await add(page).click();
  await expect(page.getByLabel(en["hub.coupons.code"])).toHaveAttribute("aria-invalid", "true");
  await expect(page.getByTestId("coupon-form")).toContainText(en["hub.coupons.invalid.code"]);

  await page.getByLabel(en["hub.coupons.code"]).fill("OK-CODE");
  await page.getByLabel(en["hub.coupons.percent"]).fill("101");
  await add(page).click();
  await expect(page.getByTestId("coupon-form")).toContainText(en["hub.coupons.invalid.percent"]);

  await page.getByLabel(en["hub.coupons.percent"]).fill("10");
  await page.getByLabel(en["hub.coupons.from"]).fill("2027-02-01");
  await page.getByLabel(en["hub.coupons.to"]).fill("2027-01-01");
  await add(page).click();
  await expect(page.getByTestId("coupon-form")).toContainText(en["hub.coupons.invalid.dates"]);
  expect(hub.intents).toHaveLength(0);
});

test("the Hub's field refusal lands on that field", async ({ page }) => {
  const hub = newHub();
  hub.refuse = "code";
  await open(page, hub);
  await page.getByLabel(en["hub.coupons.code"]).fill("TAKEN");
  await page.getByLabel(en["hub.coupons.percent"]).fill("10");
  await add(page).click();
  await expect(page.getByTestId("coupon-form")).toContainText(en["hub.coupons.invalid.code"]);
  expect(hub.intents).toHaveLength(1);
});

test("editing sends the rev, keeps the use count and can deactivate", async ({ page }) => {
  const hub = newHub();
  hub.coupons = [diwali()];
  await open(page, hub);
  await expect(page.getByTestId("coupon-used")).toHaveText("3 of 10 used");
  await page.getByRole("button", { name: "Edit DIWALI-10" }).click();
  await expect(page.getByLabel(en["hub.coupons.code"])).toHaveValue("DIWALI-10");
  await expect(page.getByLabel(en["hub.coupons.percent"])).toHaveValue("10");
  await expect(page.getByLabel(en["hub.coupons.minimum"])).toHaveValue("100.00");
  await page.getByLabel(en["hub.coupons.percent"]).fill("15");
  await page.getByLabel(en["hub.coupons.active"]).uncheck();
  await page.getByRole("button", { name: en["hub.coupons.save"] }).click();
  await expect(status(page)).toHaveText(en["hub.settings.saved"]);
  expect(hub.intents[0].base_rev).toBe(5);
  expect(hub.intents[0].payload).toMatchObject({
    kind: "coupon.upsert",
    id: "C1",
    percent_bp: 1500,
    active: false,
  });
  await expect(page.getByTestId("coupon-row")).toContainText(en["hub.coupons.badge.inactive"]);
  await expect(page.getByTestId("coupon-used")).toHaveText("3 of 10 used");
});

test("a stale edit reloads the coupon and asks to try again", async ({ page }) => {
  const hub = newHub();
  hub.coupons = [diwali()];
  hub.stale = true;
  await open(page, hub);
  await page.getByRole("button", { name: "Edit DIWALI-10" }).click();
  await page.getByRole("button", { name: en["hub.coupons.save"] }).click();
  await expect(status(page)).toContainText(en["hub.coupons.stale"]);
});

test("a Counter's save asks a manager's PIN in place and is applied with the ticket", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub, "counter-token");
  await page.getByLabel(en["hub.coupons.code"]).fill("SAVE10");
  await page.getByLabel(en["hub.coupons.percent"]).fill("10");
  await add(page).click();
  await expect(page.getByTestId("coupon-approval")).toBeVisible();
  expect(hub.intents).toHaveLength(0);
  await page.keyboard.type("2222");
  await page.keyboard.press("Enter");
  await expect(status(page)).toHaveText(en["hub.settings.saved"]);
  expect(hub.elevations.at(-1)).toEqual({
    items: [{ intent_kind: "coupon.upsert" }],
    pin: "2222",
  });
  expect(hub.intents[0].ticket).toBe("T1");
});
