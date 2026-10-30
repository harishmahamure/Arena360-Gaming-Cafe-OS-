import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type { OverdueView, PlayerHit, SessionStaff } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { CORS, HUB, json, snapshot } from "./floor-fixture";

const COUNTER: SessionStaff = {
  staff_id: "01JCOUNTER0000000000000000",
  display_name: "Counter",
  role_name: "Counter",
  permissions: ["pos.sell", "player.view", "session.start", "credit.collect"],
};

const ASHA: PlayerHit = {
  player_id: "P1",
  name: "Asha Rao",
  username: "asha_9",
  phone: "+919876543210",
  tags: [],
  outstanding_minor: 25_000,
};

const VIEW: OverdueView = {
  as_of_day: "2026-10-25",
  reminders: true,
  rows: [
    {
      player_id: "P1",
      name: "Asha Rao",
      erased: false,
      outstanding_minor: 25_000,
      oldest_unpaid_day: "2026-10-05",
      age_days: 20,
      bucket: "15_plus",
      consent: true,
      last_reminder_day: "2026-10-19",
    },
    {
      player_id: "P2",
      name: "Ravi Nair",
      erased: false,
      outstanding_minor: 4_000,
      oldest_unpaid_day: "2026-10-22",
      age_days: 3,
      bucket: "0_7",
      consent: false,
      last_reminder_day: null,
    },
  ],
};

async function serve(page: Page, overdue: OverdueView) {
  await page.addInitScript(() => sessionStorage.setItem("arena.hub.token", "tok"));
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot": {
        const snap = snapshot([]);
        snap.sessions = [];
        snap.bills = [];
        return json(route, snap);
      }
      case "/ui/me":
        return json(route, COUNTER);
      case "/ui/settings":
        return json(route, []);
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: `retry: 300\n\n`,
        });
      case "/ui/players/overdue":
        return json(route, overdue);
      case "/ui/players/profile":
        return json(route, {
          player: ASHA,
          consent: { granted: true, policy_version: "1" },
          rev: 3,
          wallet_minor: 0,
          email: "",
          dob: "",
          notes: "",
        });
      case "/ui/players/wallet":
        return json(route, { player_id: "P1", balance_minor: 0, txns: [] });
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
}

const serious = (violations: { impact?: string | null }[]) =>
  violations.filter((v) => v.impact === "serious" || v.impact === "critical");

test("the Overdue view lists players oldest first with amount, bucket, consent and last reminder", async ({
  page,
}) => {
  await serve(page, VIEW);
  await page.goto("/players");
  await page.getByTestId("players-view-overdue").click();
  const rows = page.getByTestId("overdue-row");
  await expect(rows).toHaveCount(2);
  await expect(rows.first()).toContainText("Asha Rao");
  await expect(rows.first().getByTestId("overdue-amount")).toHaveText("₹250.00");
  await expect(rows.first()).toContainText(en["hub.credit.aging.b15"]);
  await expect(rows.first()).toContainText("2026-10-05");
  await expect(rows.first().getByTestId("overdue-consent")).toHaveText(
    en["hub.players.overdue.consent.granted"],
  );
  await expect(rows.first().getByTestId("overdue-last")).toHaveText("Last reminder 2026-10-19");
  await expect(rows.nth(1).getByTestId("overdue-consent")).toHaveText(
    en["hub.players.overdue.consent.withdrawn"],
  );
  await expect(rows.nth(1).getByTestId("overdue-last")).toHaveText(en["hub.players.overdue.never"]);
  await expect(page.getByTestId("overdue-lock")).toHaveCount(0);
  expect(serious((await new AxeBuilder({ page }).analyze()).violations)).toEqual([]);
});

test("tapping a row opens the player profile", async ({ page }) => {
  await serve(page, VIEW);
  await page.goto("/players?view=overdue");
  await page.getByTestId("overdue-row").first().click();
  await expect(page.getByTestId("player-profile")).toBeVisible();
  await expect(page.getByTestId("player-profile")).toContainText("Asha Rao");
});

test("a Free branch sees the list with a tier lock and no reminder column", async ({ page }) => {
  await serve(page, { ...VIEW, reminders: false });
  await page.goto("/players?view=overdue");
  await expect(page.getByTestId("overdue-row")).toHaveCount(2);
  await expect(page.getByTestId("overdue-lock")).toContainText(en["hub.players.overdue.locked"]);
  await expect(page.getByTestId("overdue-last")).toHaveCount(0);
  expect(serious((await new AxeBuilder({ page }).analyze()).violations)).toEqual([]);
});

test("an empty list says no one owes credit", async ({ page }) => {
  await serve(page, { ...VIEW, rows: [] });
  await page.goto("/players?view=overdue");
  await expect(page.getByText(en["hub.players.overdue.empty"])).toBeVisible();
});
