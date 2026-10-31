import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type { HubSnapshot, SessionStaff, StaffList, StaffView } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { ORIGIN } from "./origin";

const HUB = "http://127.0.0.1:7409";
const CORS = {
  "access-control-allow-origin": ORIGIN,
  "access-control-allow-methods": "GET, POST",
  "access-control-allow-headers": "authorization, content-type",
};
const OWNER_ID = "01JOWNER00000000000000000";
const STAFF: Record<string, SessionStaff> = {
  "owner-token": {
    staff_id: OWNER_ID,
    display_name: "Owner",
    role_name: "Owner",
    permissions: ["user.manage"],
  },
  "manager-token": {
    staff_id: "01JMANAGER0000000000000000",
    display_name: "Manager",
    role_name: "Manager",
    permissions: ["user.manage"],
  },
  "counter-token": {
    staff_id: "01JCOUNTER0000000000000000",
    display_name: "Counter",
    role_name: "Counter",
    permissions: ["session.start"],
  },
};

const member = (
  id: string,
  name: string,
  role: string,
  status: StaffView["status"],
): StaffView => ({
  id,
  name,
  login: name.toLowerCase(),
  role,
  status,
  rev: 1,
});

type Hub = {
  staff: StaffView[];
  limit: number;
  deactivations: { staff_id: string; pin: string }[];
};

const newHub = (limit = 3): Hub => ({
  staff: [
    member(OWNER_ID, "Owner", "Owner", "active"),
    member("S2", "Asha", "Counter", "active"),
    member("S3", "Ravi", "Kitchen", "invited"),
  ],
  limit,
  deactivations: [],
});

const snapshot = (): HubSnapshot => ({
  branch: { name: "Dev Branch", branch_code: "DEV01" },
  groups: [],
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

const list = (hub: Hub): StaffList => ({
  staff: hub.staff,
  used: hub.staff.length,
  limit: hub.limit,
});

async function open(page: Page, hub: Hub, token = "owner-token") {
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
      case "/ui/staff":
        if (!STAFF[auth]?.permissions.includes("user.manage")) {
          return json(route, 403, { code: "PERMISSION_DENIED", message: "no" });
        }
        return json(route, 200, list(hub));
      case "/ui/staff/deactivate": {
        const body = req.postDataJSON() as { staff_id: string; pin: string };
        hub.deactivations.push({ staff_id: body.staff_id, pin: body.pin });
        if (body.pin !== "1111") return json(route, 403, { code: "PIN_REJECTED", message: "no" });
        hub.staff = hub.staff.filter((m) => m.id !== body.staff_id);
        return json(route, 200, { applied: { intent_id: "I", changes: [] } });
      }
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: "retry: 300\n\n",
        });
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/settings/staff");
  await expect(page.getByTestId("staff-page")).toBeVisible();
}

async function expectNoSeriousAxe(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  expect(bad.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual(
    [],
  );
}

test("the list shows name, role and status with the used count, and a full Free branch is locked", async ({
  page,
}) => {
  await open(page, newHub());
  await expect(page.getByTestId("staff-count")).toHaveText("3 of 3 users");
  const rows = page.getByTestId("staff-row");
  await expect(rows).toHaveCount(3);
  await expect(rows.nth(1)).toContainText("Asha");
  await expect(rows.nth(1)).toContainText("Counter");
  await expect(rows.nth(1).getByTestId("staff-status-badge")).toHaveText(
    en["hub.staff.status.active"],
  );
  await expect(rows.nth(2).getByTestId("staff-status-badge")).toHaveText(
    en["hub.staff.status.invited"],
  );
  const lock = page.getByTestId("staff-limit").locator("[data-component=tier-lock]");
  await expect(lock).toContainText("Available on Basic");
  await expect(lock.getByRole("link", { name: en["staff.tier.upgrade"] })).toHaveAttribute(
    "href",
    /\/upgrade\?tier=basic$/,
  );
  await expectNoSeriousAxe(page);
});

test("below the limit there is no lock", async ({ page }) => {
  await open(page, newHub(10));
  await expect(page.getByTestId("staff-count")).toHaveText("3 of 10 users");
  await expect(page.getByTestId("staff-limit")).toHaveCount(0);
});

test("an Owner deactivates a member with their own PIN and the slot is freed", async ({ page }) => {
  const hub = newHub();
  await open(page, hub);
  // Not themselves.
  await expect(page.getByTestId("staff-row").first().getByRole("button")).toHaveCount(0);
  await page
    .getByTestId("staff-row")
    .nth(1)
    .getByRole("button", { name: en["hub.staff.deactivate"] })
    .click();
  await expect(page.getByTestId("staff-approval")).toContainText("Asha");
  await page.keyboard.type("2222");
  await page.keyboard.press("Enter");
  await expect(page.getByText(en["hub.staff.pin_rejected"])).toBeVisible();
  await page.keyboard.type("1111");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("staff-status")).toHaveText(
    en["hub.staff.deactivated"].replace("{name}", "Asha"),
  );
  expect(hub.deactivations).toEqual([
    { staff_id: "S2", pin: "2222" },
    { staff_id: "S2", pin: "1111" },
  ]);
  await expect(page.getByTestId("staff-count")).toHaveText("2 of 3 users");
  await expect(page.getByTestId("staff-row")).toHaveCount(2);
  await expect(page.getByTestId("staff-limit")).toHaveCount(0);
});

test("a Manager sees the list but has no deactivate button", async ({ page }) => {
  await open(page, newHub(), "manager-token");
  await expect(page.getByTestId("staff-row")).toHaveCount(3);
  await expect(page.getByRole("button", { name: en["hub.staff.deactivate"] })).toHaveCount(0);
});

test("a Counter is told the staff are not theirs to see", async ({ page }) => {
  await open(page, newHub(), "counter-token");
  await expect(page.getByTestId("staff-no-access")).toHaveText(en["hub.staff.no_access"]);
});
