import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type { SessionStaff, ShiftState, SnapshotShift } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { floatText, parseFloat } from "../src/lib/shift";
import { CORS, HUB, device, json as ok, snapshot } from "./floor-fixture";

// Shift (Story 5.10): open a shift with a confirmed float. The Hub's loopback API is stubbed with
// `page.route`; the stub opens the shift when `shift.open` arrives, as the Hub would.

const COUNTER: SessionStaff = {
  staff_id: "01JCOUNTER0000000000000000",
  display_name: "Asha",
  role_name: "Counter",
  permissions: ["shift.open"],
};

type Payload = Record<string, unknown> & { kind: string };
type Hub = {
  shift: ShiftState;
  intents: Payload[];
  fail: { status: number; code: string; field?: string; message?: string } | null;
};

const OPEN: SnapshotShift = {
  id: "SH1",
  staff_id: "01JMANAGER0000000000000000",
  staff_name: "Ravi",
  opened_at: Date.UTC(2026, 9, 28, 9, 30),
  float_minor: 5000,
  events: 3,
};

const newHub = (carried_minor = 5000): Hub => ({
  shift: { open: null, carried_minor },
  intents: [],
  fail: null,
});

async function open(page: Page, hub: Hub, path = "/shift") {
  await page.addInitScript(() => sessionStorage.setItem("arena.hub.token", "tok"));
  await page.route(`${HUB}/**`, async (route: Route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot": {
        const snap = snapshot([device(1, {})]);
        snap.shift = hub.shift;
        return ok(route, snap);
      }
      case "/ui/me":
        return ok(route, { ...COUNTER, locked: false, has_pin: true });
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: `retry: 300\n\n`,
        });
      case "/ui/intent": {
        const { intent } = req.postDataJSON() as { intent: { payload: Payload } };
        hub.intents.push(intent.payload);
        if (hub.fail) {
          return route.fulfill({
            status: hub.fail.status,
            contentType: "application/json",
            headers: CORS,
            body: JSON.stringify({ message: "no", ...hub.fail }),
          });
        }
        hub.shift = {
          open: {
            ...OPEN,
            staff_id: COUNTER.staff_id,
            staff_name: "Asha",
            float_minor: intent.payload.float_minor as number,
            events: 1,
          },
          carried_minor: hub.shift.carried_minor,
        };
        return ok(route, { applied: { intent_id: "I", changes: [] } });
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto(path);
}

async function expectNoSeriousAxe(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  expect(bad.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual(
    [],
  );
}

test("the float field parses whole and decimal amounts, zero included", () => {
  expect(parseFloat("0", 2)).toBe(0);
  expect(parseFloat("500", 2)).toBe(50000);
  expect(parseFloat(" 50.5 ", 2)).toBe(5050);
  expect(parseFloat("1.234", 2)).toBeNull();
  expect(parseFloat("-5", 2)).toBeNull();
  expect(parseFloat("", 2)).toBeNull();
  expect(parseFloat("abc", 2)).toBeNull();
  expect(parseFloat("12.5", 0)).toBeNull();
  expect(floatText(0, 2)).toBe("0.00");
  expect(floatText(5000, 2)).toBe("50.00");
  expect(floatText(7, 2)).toBe("0.07");
  expect(floatText(12, 0)).toBe("12");
});

test("with no open shift, Open shift prefills the last float and confirming opens it", async ({
  page,
}) => {
  const hub = newHub(5000);
  await open(page, hub);
  await expect(page.getByTestId("shift-none")).toHaveText(en["hub.shift.none"]);
  await expectNoSeriousAxe(page);
  await page.getByRole("button", { name: en["hub.shift.open"] }).click();
  await expect(page.getByLabel(en["hub.shift.float"])).toHaveValue("50.00");
  await expect(page.getByTestId("shift-carried")).toContainText("₹50.00");
  await page.getByRole("button", { name: en["hub.shift.confirm"] }).click();
  await expect(page.getByTestId("shift-open")).toBeVisible();
  expect(hub.intents).toEqual([{ kind: "shift.open", float_minor: 5000 }]);
  await expect(page.getByTestId("shift-status")).toHaveText(en["hub.shift.opened"]);
});

test("the first shift prefills zero and a different float opens with the entered amount", async ({
  page,
}) => {
  const hub = newHub(0);
  await open(page, hub);
  await page.getByRole("button", { name: en["hub.shift.open"] }).click();
  await expect(page.getByLabel(en["hub.shift.float"])).toHaveValue("0.00");
  await page.getByLabel(en["hub.shift.float"]).fill("75.50");
  await page.getByRole("button", { name: en["hub.shift.confirm"] }).click();
  await expect(page.getByTestId("shift-float")).toHaveText("₹75.50");
  expect(hub.intents).toEqual([{ kind: "shift.open", float_minor: 7550 }]);
});

test("a float that is not an amount is refused before sending", async ({ page }) => {
  const hub = newHub();
  await open(page, hub);
  await page.getByRole("button", { name: en["hub.shift.open"] }).click();
  await page.getByLabel(en["hub.shift.float"]).fill("abc");
  await page.getByRole("button", { name: en["hub.shift.confirm"] }).click();
  await expect(page.getByText(en["hub.shift.err.float"])).toBeVisible();
  expect(hub.intents).toEqual([]);
  await expectNoSeriousAxe(page);
});

test("an open shift shows who opened it, when, the float and the count — never the drawer total", async ({
  page,
}) => {
  const hub = newHub();
  hub.shift = { open: OPEN, carried_minor: 5000 };
  await open(page, hub);
  await expect(page.getByTestId("shift-opened-by")).toHaveText("Ravi");
  await expect(page.getByTestId("shift-opened-at")).toContainText("28");
  await expect(page.getByTestId("shift-float")).toHaveText("₹50.00");
  await expect(page.getByTestId("shift-events")).toHaveText("3");
  await expect(page.getByTestId("shift-blind")).toHaveText(en["hub.shift.blind"]);
  await expect(page.getByRole("button", { name: en["hub.shift.open"] })).toHaveCount(0);
  await expect(page.getByText(/expected/i)).toHaveCount(0);
  await expectNoSeriousAxe(page);
});

test("opening while another shift is open names whose it is and shows it", async ({ page }) => {
  const hub = newHub();
  hub.fail = {
    status: 409,
    code: "SHIFT_ALREADY_OPEN",
    field: undefined,
    message: "Ravi has a shift open.",
  };
  await open(page, hub);
  await page.getByRole("button", { name: en["hub.shift.open"] }).click();
  await page.getByRole("button", { name: en["hub.shift.confirm"] }).click();
  await expect(page.getByTestId("shift-refusal")).toContainText("Ravi has a shift open.");
});

test("a refused permission and a failed request say so", async ({ page }) => {
  const hub = newHub();
  hub.fail = { status: 403, code: "PERMISSION_DENIED" };
  await open(page, hub);
  await page.getByRole("button", { name: en["hub.shift.open"] }).click();
  await page.getByRole("button", { name: en["hub.shift.confirm"] }).click();
  await expect(page.getByTestId("shift-refusal")).toContainText(en["hub.refusal.permission"]);
});

test("a Hub that sends no shift shows a retry, not a fake empty state", async ({ page }) => {
  await page.addInitScript(() => sessionStorage.setItem("arena.hub.token", "tok"));
  await page.route(`${HUB}/**`, async (route: Route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot":
        return ok(route, snapshot([device(1, {})]));
      case "/ui/me":
        return ok(route, { ...COUNTER, locked: false, has_pin: true });
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: `retry: 300\n\n`,
        });
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/shift");
  await expect(page.getByTestId("shift-load-failed")).toHaveText(en["hub.shift.load_failed"]);
  await expect(page.getByRole("button", { name: en["common.retry"] })).toBeVisible();
});
