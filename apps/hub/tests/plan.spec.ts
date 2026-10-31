import { expect, test, type Page } from "@playwright/test";
import type { SnapshotAlert, Tier } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { CORS, HUB, MANAGER, device, json, snapshot } from "./floor-fixture";

// The plan a paid licence gives (Story 10.6): System shows "Plan: Basic · Monthly" and the
// licence's end in branch time, and a move onto a paid plan raises one silent "Plan updated"
// notice in the alert list. The Hub's loopback API is stubbed with `page.route`.

type Plan = {
  tier: Tier;
  plan_cycle?: "monthly" | "yearly";
  licence_exp?: number;
  alerts?: SnapshotAlert[];
};

async function open(page: Page, plan: Plan, path: string) {
  await page.addInitScript(() => {
    sessionStorage.setItem("arena.hub.token", "manager-token");
    const w = window as unknown as { __plays: string[] };
    w.__plays = [];
    HTMLMediaElement.prototype.play = function (this: HTMLMediaElement) {
      w.__plays.push(this.src);
      return Promise.resolve();
    };
  });
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot": {
        const snap = snapshot([device(1, {})]);
        snap.tier = plan.tier;
        snap.plan_cycle = plan.plan_cycle;
        snap.licence_exp = plan.licence_exp;
        snap.alerts = plan.alerts ?? [];
        return json(route, snap);
      }
      case "/ui/me":
        return json(route, { ...MANAGER, locked: false, has_pin: true });
      case "/ui/settings":
        return json(route, []);
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
  await page.goto(path);
}

test("System names a paid plan with its cycle and the licence end", async ({ page }) => {
  await open(
    page,
    { tier: "basic", plan_cycle: "monthly", licence_exp: Date.UTC(2027, 0, 20, 12, 0) },
    "/system/connection",
  );
  await expect(page.getByTestId("system-plan-name")).toHaveText(
    en["hub.system.plan_cycle"]
      .replace("{tier}", en["hub.plan.tier.basic"])
      .replace("{cycle}", en["hub.plan.cycle.monthly"]),
  );
  await expect(page.getByTestId("system-plan-expires")).toContainText(
    en["hub.system.plan_expires"].replace(" {when}", ""),
  );
  await expect(page.getByTestId("system-plan-expires")).toContainText("2027");
});

test("a yearly Premium plan and a Free Hub read as they are", async ({ page }) => {
  await open(page, { tier: "premium", plan_cycle: "yearly" }, "/system/connection");
  await expect(page.getByTestId("system-plan-name")).toHaveText(
    en["hub.system.plan_cycle"]
      .replace("{tier}", en["hub.plan.tier.premium"])
      .replace("{cycle}", en["hub.plan.cycle.yearly"]),
  );
  await expect(page.getByTestId("system-plan-expires")).toHaveCount(0);
});

test("a Free Hub shows its plan with no cycle and no end", async ({ page }) => {
  await open(page, { tier: "free", plan_cycle: "monthly", licence_exp: 1 }, "/system/connection");
  await expect(page.getByTestId("system-plan-name")).toHaveText(
    en["hub.system.plan"].replace("{tier}", en["hub.plan.tier.free"]),
  );
  await expect(page.getByTestId("system-plan-expires")).toHaveCount(0);
});

test("the Plan updated notice names the plan now in force and makes no sound", async ({ page }) => {
  const notice = {
    id: "P1",
    kind: "plan_updated",
    device_id: null,
    raised_at: Date.now() - 60_000,
  } as SnapshotAlert;
  await open(page, { tier: "basic", plan_cycle: "monthly", alerts: [notice] }, "/floor");
  await expect(page.locator("[data-component=floor-tile]").first()).toBeVisible();
  await page.getByTestId("hub-alerts").click();
  await expect(page.getByTestId("alert-row")).toContainText(
    en["hub.alerts.kind.plan_updated"].replace("{tier}", en["hub.plan.tier.basic"]),
  );
  await page.waitForTimeout(300);
  expect(await page.evaluate(() => (window as unknown as { __plays: string[] }).__plays)).toEqual(
    [],
  );
});
