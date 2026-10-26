import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type { HubSnapshot, SessionStaff, SnapshotGroup } from "@arena/ui/generated";
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
    permissions: ["device.manage"],
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
  intents: { payload: Payload; ticket?: string }[];
  elevations: { items: { intent_kind: string }[]; pin: string }[];
};

const newHub = (): Hub => ({
  groups: [{ id: "G1", name: "PCs", kind: "pc", colour: "teal", sort_order: 1, rev: 1 }],
  intents: [],
  elevations: [],
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
          intent: { payload: Payload };
          elevation_ticket?: string;
        };
        const { payload } = body.intent;
        hub.intents.push({ payload, ticket: body.elevation_ticket });
        if (payload.price_per_hour_minor === 999_999_999_999_999) {
          return json(route, 422, {
            code: "VALIDATION_FAILED",
            message: "too big",
            field: "price_per_hour_minor",
          });
        }
        const g = hub.groups.find((x) => x.id === payload.group_id)!;
        g.rate_minor = payload.price_per_hour_minor as number;
        return json(route, 200, { applied: { intent_id: "I", changes: [] } });
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/settings/layout");
  await expect(page.getByTestId("group-form")).toBeVisible();
}

async function expectNoSeriousAxe(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  expect(bad.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual(
    [],
  );
}

const editRate = (page: Page) =>
  page.getByRole("button", { name: en["hub.groups.rate_edit"].replace("{name}", "PCs") });

test("a manager sets a rate and later changes it; the group shows each in the branch format", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub);
  const rate = page.getByTestId("group-rate");
  await expect(rate).toHaveText(en["hub.groups.rate_none"]);

  await editRate(page).click();
  await expectNoSeriousAxe(page);
  await page.getByLabel(en["hub.groups.rate"]).fill("120");
  await page.getByRole("button", { name: en["hub.groups.rate_save"] }).click();
  await expect(rate).toHaveText(en["hub.groups.rate_value"].replace("{rate}", "₹120.00"));
  expect(hub.intents[0].payload).toEqual({
    kind: "rate.upsert",
    group_id: "G1",
    price_per_hour_minor: 12000,
  });
  await expect(page.getByTestId("rate-form")).toHaveCount(0);

  await editRate(page).click();
  await expect(page.getByLabel(en["hub.groups.rate"])).toHaveValue("120.00");
  await page.getByLabel(en["hub.groups.rate"]).fill("150.5");
  await page.getByRole("button", { name: en["hub.groups.rate_save"] }).click();
  await expect(rate).toHaveText(en["hub.groups.rate_value"].replace("{rate}", "₹150.50"));
  expect(hub.intents.at(-1)?.payload.price_per_hour_minor).toBe(15050);
});

test("zero, negative and non-numeric rates show a field error and send nothing", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub);
  await editRate(page).click();
  for (const bad of ["0", "-5", "abc", "1.234"]) {
    await page.getByLabel(en["hub.groups.rate"]).fill(bad);
    await page.getByRole("button", { name: en["hub.groups.rate_save"] }).click();
    await expect(page.getByText(en["hub.groups.rate_invalid"])).toBeVisible();
  }
  expect(hub.intents).toEqual([]);
  await expectNoSeriousAxe(page);
});

test("a refusal from the Hub shows the same field error", async ({ page }) => {
  const hub = newHub();
  await open(page, hub);
  await editRate(page).click();
  await page.getByLabel(en["hub.groups.rate"]).fill("9999999999999.99");
  await page.getByRole("button", { name: en["hub.groups.rate_save"] }).click();
  await expect(page.getByText(en["hub.groups.rate_invalid"])).toBeVisible();
  expect(hub.groups[0].rate_minor).toBeUndefined();
});

test("a Counter's rate change asks for a manager's PIN in place", async ({ page }) => {
  const hub = newHub();
  await open(page, hub, "counter-token");
  await editRate(page).click();
  await page.getByLabel(en["hub.groups.rate"]).fill("90");
  await page.getByRole("button", { name: en["hub.groups.rate_save"] }).click();
  await expect(page.getByTestId("group-approval")).toBeVisible();
  expect(hub.intents).toEqual([]);
  await page.keyboard.type("2222");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("group-rate")).toHaveText(
    en["hub.groups.rate_value"].replace("{rate}", "₹90.00"),
  );
  expect(hub.elevations.at(-1)).toEqual({ items: [{ intent_kind: "rate.upsert" }], pin: "2222" });
  expect(hub.intents[0].ticket).toBe("T1");
});
