import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type {
  HubSnapshot,
  SessionStaff,
  SnapshotActiveRule,
  SnapshotBill,
  SnapshotSession,
} from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { CORS, HUB, device, json, snapshot } from "./floor-fixture";

const STAFF: SessionStaff = {
  staff_id: "01JLOOKUP00000000000000000",
  display_name: "Counter",
  role_name: "Counter",
  permissions: ["session.start", "session.extend", "player.view", "device.screen"],
};

/** 18:00 on 2027-03-10 in Asia/Kolkata. */
const SIX_PM = Date.UTC(2027, 2, 10, 12, 30);

const HAPPY: SnapshotActiveRule = {
  rule_id: "R1",
  name: "Happy hour",
  adjust_kind: "percent_bp",
  adjust_value: -2000,
  rate_minor: 8000,
  until: SIX_PM,
};

type Hub = {
  rule: SnapshotActiveRule | null;
  nextChange: number | null;
  loads: number;
  live?: { session: SnapshotSession; bill: SnapshotBill };
};

async function open(page: Page, hub: Hub, groups = 1) {
  await page.addInitScript(() => {
    sessionStorage.setItem("arena.hub.token", "tok");
    class FakeEventSource {
      static CLOSED = 2;
      readyState = 1;
      addEventListener() {}
      close() {}
    }
    (window as unknown as Record<string, unknown>).EventSource = FakeEventSource;
  });
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot": {
        hub.loads++;
        const snap: HubSnapshot = snapshot(
          [device(1, hub.live ? { status: "in_session" } : {}), device(2, { group_id: "G2" })],
          10000,
        );
        if (hub.live) {
          snap.sessions = [hub.live.session];
          snap.bills = [hub.live.bill];
        }
        snap.groups[0] = {
          ...snap.groups[0],
          ...(hub.rule ? { active_rule: hub.rule } : {}),
          ...(hub.nextChange ? { next_change_at: hub.nextChange } : {}),
        };
        if (groups > 1) {
          snap.groups.push({
            id: "G2",
            name: "VIP",
            kind: "pc",
            colour: "blue",
            sort_order: 1,
            rev: 1,
            rate_minor: 20000,
            active_rule: {
              rule_id: "R2",
              name: "Night owl",
              adjust_kind: "override_minor",
              adjust_value: 12000,
              rate_minor: 12000,
            },
          });
        }
        return json(route, snap);
      }
      case "/ui/me":
        return json(route, STAFF);
      case "/ui/settings":
        return json(route, []);
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/floor");
}

const chips = (page: Page) => page.getByTestId("hub-rule-chips").getByTestId("chip-rule");

test("the header chip names the rule, its change and when it ends", async ({ page }) => {
  await open(page, { rule: HAPPY, nextChange: null, loads: 0 });
  await expect(chips(page)).toHaveCount(1);
  await expect(chips(page)).toContainText("Happy hour −20% · until");
  await expect(chips(page)).toContainText(/6:00\s?pm|18:00/i);
  await expect(page.getByTestId("hub-rule-chips")).toHaveAccessibleName(en["hub.rules.chip.label"]);
  expect(
    (await new AxeBuilder({ page }).analyze()).violations.filter(
      (v) => v.impact === "serious" || v.impact === "critical",
    ),
  ).toEqual([]);
});

test("the chip is absent when no rule is in effect", async ({ page }) => {
  await open(page, { rule: null, nextChange: null, loads: 0 });
  await expect(page.getByTestId("hub-branch")).toBeVisible();
  await expect(page.getByTestId("hub-rule-chips")).toHaveCount(0);
});

test("with several groups affected each chip names its group", async ({ page }) => {
  await open(page, { rule: HAPPY, nextChange: null, loads: 0 }, 2);
  await expect(chips(page)).toHaveCount(2);
  await expect(chips(page).nth(0)).toContainText("Happy hour −20% · Gaming · until");
  await expect(chips(page).nth(1)).toContainText("Night owl");
  await expect(chips(page).nth(1)).toContainText("Night owl ₹120.00 an hour · VIP");
});

test("a free tile and the sheet show the rate the rule sets", async ({ page }) => {
  await open(page, { rule: HAPPY, nextChange: null, loads: 0 });
  const tile = page.locator("[data-component=floor-tile]", { hasText: "PC 1" });
  await expect(tile).toContainText("₹80.00");
  await tile.locator("[data-tile-face]").focus();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("start-rate")).toHaveText("Rate ₹80.00/hr");
  await expect(page.getByTestId("start-rule-chips")).toContainText("Happy hour −20%");
});

test("the chip and the rates change at the rule boundary without a reload", async ({ page }) => {
  const hub: Hub = { rule: HAPPY, nextChange: Date.now() + 2500, loads: 0 };
  await open(page, hub);
  await expect(chips(page)).toHaveCount(1);
  hub.rule = null;
  hub.nextChange = null;
  await expect(page.getByTestId("hub-rule-chips")).toHaveCount(0, { timeout: 10_000 });
  await expect(page.locator("[data-component=floor-tile]", { hasText: "PC 1" })).toContainText(
    "₹100.00",
  );
});

test("a bill that crossed a rule boundary shows one line per segment naming the rule", async ({
  page,
}) => {
  const base = Date.UTC(2027, 2, 10, 10, 0);
  const line = (from: number, to: number, minutes: number, rate: number, extra = {}) => ({
    device_id: device(1, {}).device_id,
    from_at: base + from * 60_000,
    to_at: base + to * 60_000,
    minutes,
    rate_minor_per_hour: rate,
    amount_minor: Math.round((minutes * rate) / 60),
    ...extra,
  });
  const hub: Hub = {
    rule: null,
    nextChange: null,
    loads: 0,
    live: {
      session: {
        id: "S1",
        device_id: device(1, {}).device_id,
        state: "active",
        started_at: base,
        paused_total_ms: 0,
        rev: 3,
        guest_no: 1,
      },
      bill: {
        session_id: "S1",
        device_id: device(1, {}).device_id,
        lines: [line(0, 19, 30, 10000), line(19, 30, 11, 8000, { rule_name: "Happy hour" })],
        subtotal_minor: 6467,
        paid_minor: 0,
        remaining_minor: 6467,
      },
    },
  };
  await open(page, hub);
  const tile = page.locator("[data-component=floor-tile]", { hasText: "PC 1" });
  await tile.locator("[data-tile-face]").focus();
  await page.keyboard.press("Enter");
  const lines = page.getByTestId("bill-breakdown").getByTestId("bill-line");
  await expect(lines).toHaveCount(2);
  await expect(lines.nth(0)).toContainText("30 min × ₹100.00/hr");
  await expect(lines.nth(0)).not.toContainText("(");
  await expect(lines.nth(1)).toContainText("11 min × ₹80.00/hr (Happy hour)");
});
