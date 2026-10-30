import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type {
  HubSnapshot,
  PlayerEditForm,
  PlayerForm,
  PlayerHit,
  PlayerPasswordForm,
  SessionStaff,
  WalletDisposal,
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
  "counter-token": {
    staff_id: "01JCOUNTER0000000000000000",
    display_name: "Counter",
    role_name: "Counter",
    permissions: ["player.manage", "player.view"],
  },
  "viewer-token": {
    staff_id: "01JVIEWER00000000000000000",
    display_name: "Viewer",
    role_name: "Viewer",
    permissions: ["player.view"],
  },
  "kitchen-token": {
    staff_id: "01JKITCHEN0000000000000000",
    display_name: "Kitchen",
    role_name: "Kitchen",
    permissions: ["kitchen.use"],
  },
};

type Sent<F = PlayerForm> = { form: F; idempotency_key: string; elevation_ticket?: string };
type EraseSent = {
  player_id: string;
  wallet?: WalletDisposal;
  note?: string;
  idempotency_key: string;
  elevation_ticket?: string;
};
type Hub = {
  sent: Sent[];
  elevations: { items: { intent_kind: string }[]; pin: string }[];
  fail: { status: number; code: string; field?: string } | null;
  players: PlayerHit[];
  queries: string[];
  rev: number;
  staleNext: boolean;
  edits: Sent<PlayerEditForm>[];
  resets: Sent<PlayerPasswordForm>[];
  erases: EraseSent[];
  walletMinor: number;
  eraseFail: { status: number; code: string; message: string } | null;
};

const ASHA: PlayerHit = {
  player_id: "P1",
  name: "Asha Rao",
  username: "asha_9",
  phone: "+919876543210",
  tags: ["vip"],
  outstanding_minor: 0,
};

const newHub = (): Hub => ({
  sent: [],
  elevations: [],
  fail: null,
  players: [
    { ...ASHA },
    { ...ASHA, player_id: "P2", name: "Ravi", username: "ravi", phone: "+919876500000", tags: [] },
  ],
  queries: [],
  rev: 3,
  staleNext: false,
  edits: [],
  resets: [],
  erases: [],
  walletMinor: 0,
  eraseFail: null,
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
        return json(route, 200, {
          player,
          consent: { granted: true, policy_version: "1" },
          rev: hub.rev,
          email: "asha@example.com",
          dob: "2000-01-02",
          notes: "corner seat",
          wallet_minor: hub.walletMinor,
        });
      }
      case "/ui/players/erase": {
        const body = req.postDataJSON() as EraseSent;
        hub.erases.push(body);
        const allowed = STAFF[auth].permissions.includes("player.manage");
        if (!allowed && !body.elevation_ticket) {
          return json(route, 403, { code: "PERMISSION_DENIED", message: "denied" });
        }
        if (hub.eraseFail) return json(route, hub.eraseFail.status, hub.eraseFail);
        hub.players = hub.players.filter((p) => p.player_id !== body.player_id);
        return json(route, 200, {
          applied: { intent_id: "I", changes: [{ tbl: "players", row_id: body.player_id }] },
        });
      }
      case "/ui/players/update":
      case "/ui/players/password": {
        const body = req.postDataJSON() as Sent<PlayerEditForm & PlayerPasswordForm>;
        const allowed = STAFF[auth].permissions.includes("player.manage");
        if (!allowed && !body.elevation_ticket) {
          return json(route, 403, { code: "PERMISSION_DENIED", message: "denied" });
        }
        const applied = {
          applied: { intent_id: "I", changes: [{ tbl: "players", row_id: "P1" }] },
        };
        if (new URL(req.url()).pathname.endsWith("/password")) {
          hub.resets.push(body);
          return json(route, 200, applied);
        }
        hub.edits.push(body);
        const player = hub.players.find((p) => p.player_id === body.form.player_id)!;
        if (hub.staleNext) {
          hub.staleNext = false;
          hub.rev += 1;
          player.name = "Asha Changed";
          return json(route, 409, { code: "STALE", message: "stale", field: "base_rev" });
        }
        if (body.form.base_rev !== hub.rev) {
          return json(route, 409, { code: "STALE", message: "stale", field: "base_rev" });
        }
        hub.rev += 1;
        Object.assign(player, {
          name: body.form.name,
          phone: body.form.phone,
          tags: body.form.tags,
        });
        return json(route, 200, applied);
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

const pickAsha = async (page: Page) => {
  await lookupField(page).fill("98765");
  await lookupField(page).press("Enter");
  await expect(page.getByTestId("player-profile")).toContainText("Asha Rao");
};

const openEdit = async (page: Page) => {
  await pickAsha(page);
  await page.getByRole("button", { name: en["hub.players.edit"] }).click();
  await expect(page.getByTestId("player-edit-form")).toBeVisible();
};

test("Edit profile shows the current details and sends only the edit with base_rev", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub);
  await openEdit(page);
  const form = page.getByTestId("player-edit-form");
  await expect(form.getByLabel(en["hub.players.phone"])).toHaveValue("+919876543210");
  await expect(form.getByLabel(en["hub.players.email"])).toHaveValue("asha@example.com");
  await expect(form.getByLabel(en["hub.players.notes"])).toHaveValue("corner seat");
  await expect(form.getByLabel(en["hub.players.tags"])).toHaveValue("vip");
  await expect(form.getByLabel(en["player.signup.consent"])).toBeChecked();
  await expectNoSeriousAxe(page);
  await form.getByLabel(en["hub.players.name"], { exact: true }).fill("Asha Verma");
  await form.getByLabel(en["hub.players.phone"]).fill("+91 91111 11111");
  await form.getByLabel(en["player.signup.consent"]).uncheck();
  await form.getByRole("button", { name: en["hub.players.edit.save"] }).click();
  await expect(page.getByTestId("player-status")).toContainText(en["hub.players.edit.done"]);
  expect(hub.edits).toHaveLength(1);
  expect(hub.edits[0].form).toMatchObject({
    player_id: "P1",
    base_rev: 3,
    name: "Asha Verma",
    phone: "+91 91111 11111",
    tags: ["vip"],
    consent: false,
  });
  await expect(page.getByTestId("player-profile")).toContainText("Asha Verma");
  await expect(page.getByTestId("player-edit-form")).toHaveCount(0);
});

test("a stale edit shows the current values and a notice", async ({ page }) => {
  const hub = newHub();
  hub.staleNext = true;
  await open(page, hub);
  await openEdit(page);
  const form = page.getByTestId("player-edit-form");
  await form.getByLabel(en["hub.players.name"], { exact: true }).fill("Mine");
  await form.getByRole("button", { name: en["hub.players.edit.save"] }).click();
  await expect(page.getByText(en["hub.players.edit.stale"])).toBeVisible();
  await expect(
    page.getByTestId("player-edit-form").getByLabel(en["hub.players.name"], { exact: true }),
  ).toHaveValue("Asha Changed");
  expect(hub.edits).toHaveLength(1);
});

test("an invalid edit is refused before anything is sent", async ({ page }) => {
  const hub = newHub();
  await open(page, hub);
  await openEdit(page);
  const form = page.getByTestId("player-edit-form");
  await form.getByLabel(en["hub.players.phone"]).fill("+91 12");
  await form.getByRole("button", { name: en["hub.players.edit.save"] }).click();
  await expect(page.getByText(en["hub.players.invalid.phone"])).toBeVisible();
  await expect(form.getByLabel(en["hub.players.phone"])).toBeFocused();
  expect(hub.edits).toEqual([]);
});

test("a differing or short new password shows an inline error and sends nothing", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub);
  await openEdit(page);
  const form = page.getByTestId("player-reset-form");
  const set = form.getByRole("button", { name: en["hub.players.reset.save"] });
  await form.getByLabel(en["hub.players.reset.new"], { exact: true }).fill("12345");
  await form.getByLabel(en["hub.players.reset.confirm"], { exact: true }).fill("12345");
  await set.click();
  await expect(page.getByText(en["hub.players.invalid.password"])).toBeVisible();
  await form.getByLabel(en["hub.players.reset.new"], { exact: true }).fill("newpass1");
  await set.click();
  await expect(page.getByText(en["hub.players.invalid.confirm"])).toBeVisible();
  await expect(form.getByLabel(en["hub.players.reset.confirm"], { exact: true })).toBeFocused();
  expect(hub.resets).toEqual([]);
});

test("typing the new password twice resets it", async ({ page }) => {
  const hub = newHub();
  await open(page, hub);
  await openEdit(page);
  const form = page.getByTestId("player-reset-form");
  await form.getByLabel(en["hub.players.reset.new"], { exact: true }).fill("newpass1");
  await form.getByLabel(en["hub.players.reset.confirm"], { exact: true }).fill("newpass1");
  await form.getByRole("button", { name: en["hub.players.reset.save"] }).click();
  await expect(page.getByTestId("player-status")).toContainText(en["hub.players.reset.done"]);
  expect(hub.resets).toHaveLength(1);
  expect(hub.resets[0].form).toEqual({
    player_id: "P1",
    password: "newpass1",
    confirm: "newpass1",
  });
});

test("a member without player.manage asks for a manager's PIN in place to edit", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub, "viewer-token");
  await openEdit(page);
  const form = page.getByTestId("player-edit-form");
  await form.getByLabel(en["hub.players.name"], { exact: true }).fill("Asha Verma");
  await form.getByRole("button", { name: en["hub.players.edit.save"] }).click();
  await expect(page.getByTestId("player-approval")).toBeVisible();
  expect(hub.edits).toEqual([]);
  await page.keyboard.type("2222");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("player-status")).toContainText(en["hub.players.edit.done"]);
  expect(hub.elevations.at(-1)).toEqual({ items: [{ intent_kind: "player.update" }], pin: "2222" });
  expect(hub.edits[0].elevation_ticket).toBe("T1");
});

const openErase = async (page: Page) => {
  await pickAsha(page);
  await page.getByRole("button", { name: en["hub.players.erase.open"] }).click();
  await expect(page.getByTestId("player-erase")).toBeVisible();
};

test("Erase shows what is erased and kept, then erases with no money to handle", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub);
  await openErase(page);
  const panel = page.getByTestId("player-erase");
  await expect(panel.getByTestId("erase-erased")).toContainText(
    en["hub.players.erase.erased.name"],
  );
  await expect(panel.getByTestId("erase-kept")).toContainText(en["hub.players.erase.kept.money"]);
  await expect(panel.getByTestId("erase-wallet")).toHaveCount(0);
  await expectNoSeriousAxe(page);
  await panel.getByRole("button", { name: en["hub.players.erase.confirm"] }).click();
  await expect(page.getByTestId("player-status")).toContainText(en["hub.players.erase.done"]);
  expect(hub.erases).toHaveLength(1);
  expect(hub.erases[0]).toMatchObject({ player_id: "P1" });
  expect(hub.erases[0].wallet).toBeUndefined();
  await expect(page.getByTestId("player-erased")).toContainText(en["hub.players.erased"]);
  await expect(page.getByTestId("player-profile")).toHaveCount(0);
});

test("a wallet balance needs a disposal and a reason before anything is sent", async ({ page }) => {
  const hub = newHub();
  hub.walletMinor = 50_000;
  await open(page, hub);
  await openErase(page);
  const panel = page.getByTestId("player-erase");
  await panel.getByRole("button", { name: en["hub.players.erase.confirm"] }).click();
  await expect(panel.getByTestId("erase-wallet")).toContainText(
    en["hub.players.erase.invalid.wallet"],
  );
  expect(hub.erases).toEqual([]);
  await panel.getByLabel(en["hub.players.erase.wallet.forfeited"]).check();
  await panel.getByRole("button", { name: en["hub.players.erase.confirm"] }).click();
  await expect(panel).toContainText(en["hub.players.erase.invalid.note"]);
  expect(hub.erases).toEqual([]);
  await panel.getByLabel(en["hub.players.erase.note"]).fill("left the area");
  await panel.getByRole("button", { name: en["hub.players.erase.confirm"] }).click();
  await expect(page.getByTestId("player-status")).toContainText(en["hub.players.erase.done"]);
  expect(hub.erases[0]).toMatchObject({
    player_id: "P1",
    wallet: "forfeited",
    note: "left the area",
  });
});

test("an open session blocks the erasure with its reason", async ({ page }) => {
  const hub = newHub();
  hub.eraseFail = { status: 409, code: "ERASE_BLOCKED", message: "End the session first" };
  await open(page, hub);
  await openErase(page);
  await page
    .getByTestId("player-erase")
    .getByRole("button", { name: en["hub.players.erase.confirm"] })
    .click();
  await expect(page.getByTestId("erase-refusal")).toContainText(
    en["hub.players.erase.blocked.session"],
  );
  await expect(page.getByTestId("player-erase")).toBeVisible();
});

test("a member without player.manage asks for a manager's PIN in place to erase", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub, "viewer-token");
  await openErase(page);
  await page
    .getByTestId("player-erase")
    .getByRole("button", { name: en["hub.players.erase.confirm"] })
    .click();
  await expect(page.getByTestId("player-approval")).toBeVisible();
  expect(hub.erases).toEqual([]);
  await page.keyboard.type("2222");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("player-status")).toContainText(en["hub.players.erase.done"]);
  expect(hub.elevations.at(-1)).toEqual({ items: [{ intent_kind: "player.erased" }], pin: "2222" });
  expect(hub.erases[0].elevation_ticket).toBe("T1");
});
