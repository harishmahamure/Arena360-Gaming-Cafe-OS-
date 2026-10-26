import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type { HubSnapshot, PlayerForm, PlayerHit, SessionStaff } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { ORIGIN } from "./origin";

const HUB = "http://127.0.0.1:7409";
const CORS = {
  "access-control-allow-origin": ORIGIN,
  "access-control-allow-methods": "GET, POST",
  "access-control-allow-headers": "authorization, content-type",
};
const STAFF: Record<string, SessionStaff> = {
  "counter-token": {
    staff_id: "01JCOUNTER0000000000000000",
    display_name: "Counter",
    role_name: "Counter",
    permissions: ["player.manage", "player.view"],
  },
  "kitchen-token": {
    staff_id: "01JKITCHEN0000000000000000",
    display_name: "Kitchen",
    role_name: "Kitchen",
    permissions: ["kitchen.use"],
  },
};

type Sent = { form: PlayerForm; idempotency_key: string; elevation_ticket?: string };
type Hub = {
  sent: Sent[];
  elevations: { items: { intent_kind: string }[]; pin: string }[];
  fail: { status: number; code: string; field?: string } | null;
  players: PlayerHit[];
  queries: string[];
};

const ASHA: PlayerHit = {
  player_id: "P1",
  name: "Asha Rao",
  username: "asha_9",
  phone: "+919876543210",
  tags: ["vip"],
};

const newHub = (): Hub => ({
  sent: [],
  elevations: [],
  fail: null,
  players: [
    ASHA,
    { ...ASHA, player_id: "P2", name: "Ravi", username: "ravi", phone: "+919876500000", tags: [] },
  ],
  queries: [],
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

async function open(page: Page, hub: Hub, token = "counter-token") {
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
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: `retry: 300\n\n`,
        });
      case "/ui/elevate": {
        const body = req.postDataJSON() as Hub["elevations"][number];
        hub.elevations.push(body);
        if (body.pin !== "2222") return json(route, 403, { code: "PIN_REJECTED", message: "no" });
        return json(route, 200, { ticket: "T1", items: body.items, expires_in_s: 60 });
      }
      case "/ui/players": {
        if (req.method() === "GET") {
          const q = new URL(req.url()).searchParams.get("q") ?? "";
          hub.queries.push(q);
          const digits = q.replace(/\D/g, "");
          const hits = digits ? hub.players.filter((p) => p.phone.includes(digits)) : [];
          return json(route, 200, hits);
        }
        const body = req.postDataJSON() as Sent;
        hub.sent.push(body);
        if (hub.fail) return json(route, hub.fail.status, { message: "no", ...hub.fail });
        const allowed = STAFF[auth].permissions.includes("player.manage");
        if (!allowed && !body.elevation_ticket) {
          return json(route, 403, { code: "PERMISSION_DENIED", message: "denied" });
        }
        return json(route, 200, {
          applied: {
            intent_id: "I",
            changes: [
              { tbl: "players", row_id: "P1" },
              { tbl: "consents", row_id: "C1" },
            ],
          },
        });
      }
      case "/ui/players/profile": {
        const id = new URL(req.url()).searchParams.get("id");
        const player = hub.players.find((p) => p.player_id === id);
        if (!player) return json(route, 404, { code: "NOT_FOUND", message: "no" });
        return json(route, 200, { player, consent: { granted: true, policy_version: "1" } });
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/players");
  await expect(page.getByRole("heading", { level: 1, name: en["hub.nav.players"] })).toBeVisible();
}

const openForm = async (page: Page) => {
  await page.getByRole("button", { name: en["hub.players.create"] }).click();
  await expect(page.getByTestId("player-form")).toBeVisible();
};

const fill = async (page: Page, over: Partial<Record<string, string>> = {}) => {
  const values = {
    username: "Asha_9",
    password: "secret1",
    phone: "+91 98765 43210",
    name: "Asha Rao",
    ...over,
  };
  await page.getByLabel(en["hub.players.username"]).fill(values.username);
  await page.getByLabel(en["hub.players.password"], { exact: true }).fill(values.password);
  await page.getByLabel(en["hub.players.phone"]).fill(values.phone);
  await page.getByLabel(en["hub.players.name"], { exact: true }).fill(values.name);
};

const save = (page: Page) => page.getByRole("button", { name: en["hub.players.submit"] }).click();

async function expectNoSeriousAxe(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  expect(bad.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual(
    [],
  );
}

test("Create profile opens the form with focus in username, consent unticked and +91", async ({
  page,
}) => {
  await open(page, newHub());
  await openForm(page);
  await expect(page.getByLabel(en["hub.players.username"])).toBeFocused();
  await expect(page.getByLabel(en["hub.players.phone"])).toHaveValue("+91");
  await expect(page.getByLabel(en["player.signup.consent"])).not.toBeChecked();
  await expect(page.getByLabel(en["hub.players.password"], { exact: true })).toHaveAttribute(
    "type",
    "password",
  );
  await page.getByRole("button", { name: en["ui.password.show"] }).click();
  await expect(page.getByLabel(en["hub.players.password"], { exact: true })).toHaveAttribute(
    "type",
    "text",
  );
  await expectNoSeriousAxe(page);
});

test("the required fields create a profile in under 15 seconds", async ({ page }) => {
  const hub = newHub();
  await open(page, hub);
  const started = Date.now();
  await openForm(page);
  await fill(page);
  await save(page);
  await expect(page.getByTestId("player-status")).toContainText(
    en["hub.players.created"].replace("{username}", "asha_9"),
  );
  expect(Date.now() - started).toBeLessThan(15_000);
  expect(hub.sent).toHaveLength(1);
  expect(hub.sent[0].form).toEqual({
    username: "Asha_9",
    password: "secret1",
    phone: "+91 98765 43210",
    name: "Asha Rao",
    email: "",
    dob: "",
    notes: "",
    tags: [],
    consent: false,
    policy_version: en["player.policy_version"],
  });
  expect(hub.sent[0].idempotency_key).toMatch(/^[0-9A-HJKMNP-TV-Z]{26}$/);
  expect(hub.sent[0].elevation_ticket).toBeUndefined();
  await expect(page.getByTestId("player-form")).toHaveCount(0);
  await expect(page.getByRole("button", { name: en["hub.players.create"] })).toBeFocused();
  await expectNoSeriousAxe(page);
});

test("optional details and the ticked consent are sent", async ({ page }) => {
  const hub = newHub();
  await open(page, hub);
  await openForm(page);
  await fill(page);
  await page.getByLabel(en["hub.players.email"]).fill("asha@example.com");
  await page.getByLabel(en["hub.players.dob"]).fill("2000-01-02");
  await page.getByLabel(en["hub.players.notes"]).fill("corner seat");
  await page.getByLabel(en["hub.players.tags"]).fill("vip, early ,");
  await page.getByLabel(en["player.signup.consent"]).check();
  await save(page);
  await expect(page.getByTestId("player-status")).toContainText("asha_9");
  expect(hub.sent[0].form).toMatchObject({
    email: "asha@example.com",
    dob: "2000-01-02",
    notes: "corner seat",
    tags: ["vip", "early"],
    consent: true,
  });
});

test("a short password is refused before anything is sent and focus moves to it", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub);
  await openForm(page);
  await fill(page, { password: "12345", phone: "+91 12" });
  await save(page);
  await expect(page.getByText(en["hub.players.invalid.password"])).toBeVisible();
  await expect(page.getByText(en["hub.players.invalid.phone"])).toBeVisible();
  await expect(page.getByLabel(en["hub.players.password"], { exact: true })).toBeFocused();
  expect(hub.sent).toEqual([]);
  await expectNoSeriousAxe(page);
});

test("a taken username shows on its field and focus moves to it", async ({ page }) => {
  const hub = newHub();
  hub.fail = { status: 409, code: "USERNAME_TAKEN", field: "username" };
  await open(page, hub);
  await openForm(page);
  await fill(page);
  await save(page);
  await expect(page.getByText(en["player.signup.username_taken"])).toBeVisible();
  await expect(page.getByLabel(en["hub.players.username"])).toBeFocused();
  await expect(page.getByTestId("player-status")).toBeEmpty();
  await expectNoSeriousAxe(page);
});

test("a taken phone shows on its field and focus moves to it", async ({ page }) => {
  const hub = newHub();
  hub.fail = { status: 409, code: "PHONE_TAKEN", field: "phone" };
  await open(page, hub);
  await openForm(page);
  await fill(page);
  await save(page);
  await expect(page.getByText(en["player.signup.phone_taken"])).toBeVisible();
  await expect(page.getByLabel(en["hub.players.phone"])).toBeFocused();
});

test("a member without the permission asks for a manager's PIN in place", async ({ page }) => {
  const hub = newHub();
  await open(page, hub, "kitchen-token");
  await openForm(page);
  await fill(page);
  await save(page);
  await expect(page.getByTestId("player-approval")).toBeVisible();
  expect(hub.sent).toEqual([]);
  await page.keyboard.type("9999");
  await page.keyboard.press("Enter");
  await expect(page.getByText(en["ui.pin.rejected"])).toBeVisible();
  expect(hub.sent).toEqual([]);
  await page.keyboard.type("2222");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("player-status")).toContainText("asha_9");
  expect(hub.elevations.at(-1)).toEqual({ items: [{ intent_kind: "player.create" }], pin: "2222" });
  expect(hub.sent).toHaveLength(1);
  expect(hub.sent[0].elevation_ticket).toBe("T1");
});

test("cancel closes the form without sending", async ({ page }) => {
  const hub = newHub();
  await open(page, hub);
  await openForm(page);
  await fill(page);
  await page.getByRole("button", { name: en["hub.players.cancel"] }).click();
  await expect(page.getByTestId("player-form")).toHaveCount(0);
  await expect(page.getByRole("button", { name: en["hub.players.create"] })).toBeFocused();
  expect(hub.sent).toEqual([]);
});

const lookupField = (page: Page) => page.getByLabel(en["hub.players.lookup.label"]);

test("typing digits lists matches, Enter picks the top one and shows its profile", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub);
  await expect(
    page.getByTestId("player-lookup").getByRole("button", { name: en["hub.players.lookup.guest"] }),
  ).toBeVisible();
  await lookupField(page).fill("+91 98765");
  const hits = page.getByTestId("player-hits").getByRole("button");
  await expect(hits).toHaveCount(2);
  await expect(hits.first()).toContainText("Asha Rao");
  await expect(hits.first()).toContainText("+919876543210");
  expect(hub.queries.at(-1)).toBe("9198765");
  await expectNoSeriousAxe(page);
  await lookupField(page).press("Enter");
  const profile = page.getByTestId("player-profile");
  await expect(profile).toContainText("Asha Rao");
  await expect(profile).toContainText("asha_9");
  await expect(profile).toContainText("+919876543210");
  await expect(profile).toContainText("vip");
  await expect(profile).toContainText(en["hub.players.profile.granted"].replace("{version}", "1"));
  await expectNoSeriousAxe(page);
});

test("clicking a match opens that player", async ({ page }) => {
  await open(page, newHub());
  await lookupField(page).fill("98765");
  await page.getByTestId("player-hits").getByRole("button", { name: /Ravi/ }).click();
  await expect(page.getByTestId("player-profile")).toContainText("ravi");
});

test("a complete number with no match offers Create profile with the phone and Guest", async ({
  page,
}) => {
  await open(page, newHub());
  await lookupField(page).fill("9123456789");
  const lookup = page.getByTestId("player-lookup");
  await expect(lookup).toContainText(en["hub.players.lookup.none"]);
  await expect(lookup.getByRole("button", { name: en["hub.players.lookup.guest"] })).toHaveCount(2);
  await lookup.getByRole("button", { name: en["hub.players.create"] }).click();
  await expect(page.getByTestId("player-form")).toBeVisible();
  await expect(page.getByLabel(en["hub.players.phone"])).toHaveValue("+919123456789");
});

test("a short number with no match shows no empty state", async ({ page }) => {
  await open(page, newHub());
  await lookupField(page).fill("91234");
  await expect(page.getByTestId("player-lookup")).not.toContainText(en["hub.players.lookup.none"]);
});

test("Guest is a choice beside the field", async ({ page }) => {
  await open(page, newHub());
  await page
    .getByTestId("player-lookup")
    .getByRole("button", { name: en["hub.players.lookup.guest"] })
    .click();
  await expect(page.getByText(en["hub.players.lookup.guest_picked"])).toBeVisible();
});

test("Ctrl+K opens Players with the lookup focused and is ignored inside a text field", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub);
  await page.locator("body").focus();
  await page.keyboard.press("Control+k");
  await expect(lookupField(page)).toBeFocused();
  await page.getByRole("button", { name: en["hub.players.create"] }).first().click();
  await expect(page.getByLabel(en["hub.players.username"])).toBeFocused();
  await page.keyboard.press("Control+k");
  await expect(page.getByLabel(en["hub.players.username"])).toBeFocused();
  await page.getByRole("button", { name: en["hub.players.cancel"] }).click();
  await page.locator("body").focus();
  await page.keyboard.press("Control+k");
  await expect(lookupField(page)).toBeFocused();
});

test("Ctrl+K from another screen opens Players with the lookup focused", async ({ page }) => {
  const hub = newHub();
  await open(page, hub);
  await page.goto("/floor");
  await page.locator("body").focus();
  await page.keyboard.press("Control+k");
  await expect(page).toHaveURL(/\/players\?lookup/);
  await expect(lookupField(page)).toBeFocused();
});
