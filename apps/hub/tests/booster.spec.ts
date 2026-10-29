import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type { DeviceLaunchTimes, HubSnapshot, SessionStaff } from "@arena/ui/generated";
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
    permissions: ["settings.edit", "games.manage"],
  },
  "counter-token": {
    staff_id: "01JCOUNTER0000000000000000",
    display_name: "Counter",
    role_name: "Counter",
    permissions: ["session.start"],
  },
};
const CLOSE = "kiosk.booster_close";
const PRIORITY = "kiosk.booster_priority";
const POWER = "kiosk.booster_power_plan";

type Row = { value: string; rev: number };
type Hub = {
  settings: Map<string, Row>;
  intents: { key: string; value: string; base_rev?: number; ticket?: string }[];
  times: DeviceLaunchTimes[];
  timesDown: boolean;
  /** A save is refused on the field of this key (`VALIDATION_FAILED`). */
  refuse?: string;
};

function newHub(): Hub {
  return {
    settings: new Map([
      [CLOSE, { value: "", rev: 1 }],
      [PRIORITY, { value: "above_normal", rev: 1 }],
      [POWER, { value: "false", rev: 1 }],
    ]),
    intents: [],
    times: [],
    timesDown: false,
  };
}

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

async function open(page: Page, token = "manager-token") {
  const hub = newHub();
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
      case "/ui/library":
        return json(route, 200, { entries: [] });
      case "/ui/library/scans":
        return json(route, 200, { devices: [] });
      case "/ui/library/proposals":
        return json(route, 200, { proposals: [], dismissed: 0 });
      case "/ui/settings":
        return json(
          route,
          200,
          [...hub.settings].map(([key, row]) => ({ key, ...row })),
        );
      case "/ui/games/launch-times":
        return hub.timesDown
          ? json(route, 500, { code: "INTERNAL", message: "no" })
          : json(route, 200, { devices: hub.times });
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: "retry: 400\n\n",
        });
      case "/ui/elevate":
        return json(route, 200, { ticket: "T1", items: [], expires_in_s: 60 });
      case "/ui/elevate/drop":
        return route.fulfill({ status: 204, headers: CORS });
      case "/ui/intent": {
        const { intent, elevation_ticket } = req.postDataJSON() as {
          intent: { base_rev?: number; payload: { key: string; value: string } };
          elevation_ticket?: string;
        };
        const { key, value } = intent.payload;
        hub.intents.push({ key, value, base_rev: intent.base_rev, ticket: elevation_ticket });
        if (!STAFF[auth].permissions.includes("settings.edit") && !elevation_ticket) {
          return json(route, 403, { code: "PIN_REQUIRED", message: "pin" });
        }
        if (hub.refuse === key) {
          return json(route, 422, { code: "VALIDATION_FAILED", message: "no", field: key });
        }
        const row = hub.settings.get(key)!;
        hub.settings.set(key, { value, rev: row.rev + 1 });
        return json(route, 200, { applied: { intent_id: "I", changes: [] } });
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/settings/games");
  await expect(page.getByTestId("booster-form")).toHaveAttribute("aria-busy", "false");
  return hub;
}

const save = (page: Page) => page.getByRole("button", { name: en["hub.settings.save"] });
const close = (page: Page) => page.getByLabel(en["hub.games.booster.close"]);
const power = (page: Page) => page.getByLabel(en["hub.games.booster.power"]);
const status = (page: Page) => page.getByTestId("booster-status");
const priority = (page: Page, key: "normal" | "above_normal" | "high") =>
  page.getByRole("radio", { name: en[`hub.games.booster.priority.${key}`], exact: true });

const pc = (name: string, games: DeviceLaunchTimes["games"]): DeviceLaunchTimes => ({
  device_id: `D-${name}`,
  name,
  last: null,
  games,
});

test("the Booster shows an empty list, Above normal and the power plan off, and axe passes", async ({
  page,
}) => {
  await open(page);
  await expect(close(page)).toHaveValue("");
  await expect(priority(page, "above_normal")).toBeChecked();
  await expect(priority(page, "normal")).not.toBeChecked();
  await expect(priority(page, "high")).not.toBeChecked();
  await expect(power(page)).not.toBeChecked();
  await expect(page.getByText(en["hub.games.booster.close_help"])).toBeVisible();
  await expect(page.getByText(en["hub.games.booster.power_help"])).toBeVisible();
  await expect(save(page)).toHaveAttribute("aria-disabled", "true");
  const results = await new AxeBuilder({ page }).analyze();
  expect(
    results.violations.filter((v) => v.impact === "serious" || v.impact === "critical"),
  ).toEqual([]);
});

test("a manager saves the programs, the priority and the power plan with settings.set", async ({
  page,
}) => {
  const hub = await open(page);
  await close(page).fill("discord.exe\nOneDrive.exe");
  await priority(page, "high").check();
  await power(page).check();
  await save(page).click();
  await expect(status(page)).toContainText(en["hub.settings.saved"]);
  expect(hub.intents).toEqual([
    { key: CLOSE, value: "discord.exe,OneDrive.exe", base_rev: 1 },
    { key: PRIORITY, value: "high", base_rev: 1 },
    { key: POWER, value: "true", base_rev: 1 },
  ]);
  await expect(close(page)).toHaveValue("discord.exe\nOneDrive.exe");
  await expect(priority(page, "high")).toBeChecked();
  await expect(power(page)).toBeChecked();
  await expect(save(page)).toHaveAttribute("aria-disabled", "true");
});

test("a bad program name or a repeat shows its message in place and sends nothing", async ({
  page,
}) => {
  const hub = await open(page);
  await close(page).fill("discord");
  await save(page).click();
  await expect(page.getByText(en["hub.games.booster.invalid.close"]).first()).toBeVisible();
  await expect(close(page)).toHaveAttribute("aria-invalid", "true");
  await close(page).fill("a.exe\nA.exe");
  await save(page).click();
  await expect(page.getByText(en["hub.games.booster.invalid.close_duplicate"])).toBeVisible();
  expect(hub.intents).toEqual([]);
});

test("the Hub's refusal lands on the list", async ({ page }) => {
  const hub = await open(page);
  hub.refuse = CLOSE;
  await close(page).fill("discord.exe");
  await save(page).click();
  await expect(close(page)).toHaveAttribute("aria-invalid", "true");
  expect(hub.intents.length).toBe(1);
});

test("a Counter's save asks a manager's PIN in place and is applied with the ticket", async ({
  page,
}) => {
  const hub = await open(page, "counter-token");
  await expect(page.getByTestId("booster-needs-approval")).toBeVisible();
  await priority(page, "normal").check();
  await save(page).click();
  await expect(page.getByTestId("booster-approval")).toBeVisible();
  await page.keyboard.type("2222");
  await page.keyboard.press("Enter");
  await expect(status(page)).toContainText(en["hub.settings.saved"]);
  expect(hub.intents).toEqual([{ key: PRIORITY, value: "normal", base_rev: 1, ticket: "T1" }]);
});

test("launch times list each PC's games with the median and says when none was measured", async ({
  page,
}) => {
  const hub = await open(page);
  hub.times = [
    pc("PC 1", [
      { game_id: "g1", name: "CS2", launches: 3, median_ms: 14_200 },
      { game_id: "g2", name: "Dota 2", launches: 1, median_ms: null },
    ]),
    pc("PC 2", [{ game_id: "g1", name: "CS2", launches: 2, median_ms: 31_000 }]),
  ];
  await page.reload();
  await expect(page.getByTestId("launch-times-pc")).toHaveCount(2);
  const first = page.getByTestId("launch-times-pc").first();
  await expect(first.getByTestId("launch-times-name")).toHaveText("PC 1");
  await expect(first.getByTestId("launch-times-game").first()).toHaveText(
    en["hub.games.booster.times.game"]
      .replace("{game}", "CS2")
      .replace("{seconds}", "14.2")
      .replace("{count}", "3"),
  );
  await expect(first.getByTestId("launch-times-game").nth(1)).toHaveText(
    en["hub.games.booster.times.game_none"].replace("{game}", "Dota 2").replace("{count}", "1"),
  );
});

test("no launches yet, and launch times that cannot be loaded, say so", async ({ page }) => {
  const hub = await open(page);
  await expect(page.getByTestId("launch-times-empty")).toHaveText(
    en["hub.games.booster.times.empty"],
  );
  hub.timesDown = true;
  await page.reload();
  await expect(page.getByText(en["hub.games.booster.times.load_failed"])).toBeVisible();
});
