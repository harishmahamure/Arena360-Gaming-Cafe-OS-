import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type { HubSnapshot, SessionStaff } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { ORIGIN } from "./origin";

// Staff sign-in: the gate, the header, the generic failure, the lockout countdown, the inline
// refusal on Settings and sign-out. The Hub's loopback API is stubbed with `page.route`.

const HUB = "http://127.0.0.1:7409";
const SNAPSHOT: HubSnapshot = {
  branch: { name: "Dev Branch", branch_code: "DEV01" },
  groups: [],
  devices: [],
  locale: null,
  ui: { idle_lock_minutes: 5, hc: false },
  connectivity: { state: "online", checked_at: null },
  lan: null,
  paired: true,
};
const CORS = {
  "access-control-allow-origin": ORIGIN,
  "access-control-allow-methods": "GET, POST",
  "access-control-allow-headers": "authorization, content-type",
};
const STAFF: Record<string, SessionStaff & { password: string }> = {
  counter: {
    staff_id: "01JCOUNTER0000000000000000",
    display_name: "Counter",
    role_name: "Counter",
    permissions: ["session.start", "pos.sell"],
    password: "dev-counter-pass",
  },
  manager: {
    staff_id: "01JMANAGER0000000000000000",
    display_name: "Manager",
    role_name: "Manager",
    permissions: ["settings.edit", "user.manage"],
    password: "dev-manager-pass",
  },
};
const AUTH_FAILED = { code: "AUTH_FAILED", message: "sign-in failed" };

type Hub = {
  /** token → login */
  sessions: Map<string, string>;
  /** When set, sign-in answers AUTH_LOCKED with this many seconds. */
  lockedFor: number | null;
  signIns: number;
  intents: { authorization: string | null; body: unknown }[];
  /** The `receipt.footer` row the stub serves; an applied save updates it. */
  setting: { value: string; rev: number };
  /** When set, `/ui/me` answers this status. */
  meStatus: number | null;
  /** One-shot answers that replace the next sign-in or intent reply. */
  nextSignIn: { status: number; body: unknown } | null;
  nextIntent: { status: number; body: unknown } | null;
  signOuts: (string | null)[];
};

function json(route: Route, status: number, body: unknown) {
  return route.fulfill({
    status,
    contentType: "application/json",
    headers: CORS,
    body: JSON.stringify(body),
  });
}

/** Stubs every loopback endpoint the Hub UI uses. */
async function stubHub(page: Page): Promise<Hub> {
  const hub: Hub = {
    sessions: new Map(),
    lockedFor: null,
    signIns: 0,
    intents: [],
    setting: { value: "", rev: 1 },
    meStatus: null,
    nextSignIn: null,
    nextIntent: null,
    signOuts: [],
  };
  const who = (route: Route) => {
    const auth = route.request().headers()["authorization"] ?? "";
    return hub.sessions.get(auth.replace(/^Bearer /, ""));
  };
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    const path = new URL(req.url()).pathname;
    switch (path) {
      case "/ui/snapshot":
        return json(route, 200, SNAPSHOT);
      case "/ui/auth/sign-in": {
        hub.signIns++;
        if (hub.nextSignIn) {
          const answer = hub.nextSignIn;
          hub.nextSignIn = null;
          return json(route, answer.status, answer.body);
        }
        if (hub.lockedFor !== null) {
          return json(route, 429, {
            code: "AUTH_LOCKED",
            message: "locked",
            retry_after_s: hub.lockedFor,
          });
        }
        const { login, password } = req.postDataJSON() as { login: string; password: string };
        const staff = STAFF[login.toLowerCase()];
        if (!staff || staff.password !== password) return json(route, 401, AUTH_FAILED);
        const token = `token-${hub.sessions.size + 1}`;
        hub.sessions.set(token, login.toLowerCase());
        const { password: _pw, ...me } = staff;
        return json(route, 200, { token, staff: me });
      }
      case "/ui/auth/sign-out": {
        const auth = req.headers()["authorization"] ?? "";
        hub.signOuts.push(auth || null);
        hub.sessions.delete(auth.replace(/^Bearer /, ""));
        return route.fulfill({ status: 204, headers: CORS });
      }
      case "/ui/me": {
        if (hub.meStatus !== null) return json(route, hub.meStatus, { code: "INTERNAL" });
        const login = who(route);
        if (!login) return json(route, 401, AUTH_FAILED);
        const { password: _pw, ...me } = STAFF[login];
        return json(route, 200, me);
      }
      case "/ui/settings":
        if (!who(route)) return json(route, 401, AUTH_FAILED);
        return json(route, 200, [{ key: "receipt.footer", ...hub.setting }]);
      case "/ui/intent": {
        hub.intents.push({
          authorization: req.headers()["authorization"] ?? null,
          body: req.postDataJSON(),
        });
        const login = who(route);
        if (!login) return json(route, 401, AUTH_FAILED);
        if (hub.nextIntent) {
          const answer = hub.nextIntent;
          hub.nextIntent = null;
          return json(route, answer.status, answer.body);
        }
        if (login === "counter") {
          return json(route, 403, {
            code: "PERMISSION_DENIED",
            message: "you don't have permission to do this",
          });
        }
        const sent = (req.postDataJSON() as { intent: { payload: { value: string } } }).intent;
        hub.setting = { value: sent.payload.value, rev: hub.setting.rev + 1 };
        return json(route, 200, { applied: { intent_id: "I", changes: [] } });
      }
      case "/ui/events":
        // An empty push stream that asks the browser to wait before reconnecting.
        if (!hub.sessions.has(new URL(req.url()).searchParams.get("token") ?? "")) {
          return json(route, 401, AUTH_FAILED);
        }
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: "retry: 60000\n\n",
        });
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  return hub;
}

async function signIn(page: Page, login: string, password: string) {
  await page.getByLabel(en["hub.sign_in.login"]).fill(login);
  await page.getByLabel(en["hub.sign_in.password"], { exact: true }).fill(password);
  await page.getByRole("button", { name: en["hub.sign_in.submit"] }).click();
}

test("signed out, every screen goes to sign-in", async ({ page }) => {
  await stubHub(page);
  await page.goto("/settings/receipt");
  await expect(page).toHaveURL(/\/sign-in(\?|$)/);
  await expect(page.locator("main h1")).toHaveText(en["hub.sign_in.title"]);
  // No rail before sign-in.
  await expect(page.locator("nav")).toHaveCount(0);
});

test("sign-in success shows the name and role in the header and opens the floor", async ({
  page,
}) => {
  await stubHub(page);
  await page.goto("/sign-in");
  await signIn(page, "counter", "dev-counter-pass");
  await expect(page).toHaveURL(/\/floor$/);
  await expect(page.getByTestId("hub-staff")).toHaveText("Counter · Counter");
  await expect(page.locator("header")).toContainText("Dev Branch");
  // Signed in, /sign-in sends you back to the floor.
  await page.goto("/sign-in");
  await expect(page).toHaveURL(/\/floor$/);
});

test("a failed sign-in shows the one generic message", async ({ page }) => {
  await stubHub(page);
  await page.goto("/sign-in");
  await signIn(page, "counter", "bad");
  const error = page.getByTestId("sign-in-error");
  await expect(error).toContainText(en["hub.sign_in.failed"]);
  await signIn(page, "nobody", "x");
  await expect(error).toContainText(en["hub.sign_in.failed"]);
  // The message never singles out a field.
  for (const field of ["hub.sign_in.login", "hub.sign_in.password"] as const) {
    await expect(page.getByLabel(en[field], { exact: true })).not.toHaveAttribute(
      "aria-invalid",
      "true",
    );
  }
  await expect(page).toHaveURL(/\/sign-in(\?|$)/);
});

test("a locked sign-in counts down, then lets you try again", async ({ page }) => {
  await page.clock.install({ time: new Date("2026-10-23T10:00:00") });
  const hub = await stubHub(page);
  hub.lockedFor = 3;
  await page.goto("/sign-in");
  // Time stands still unless the test moves it.
  await page.clock.pauseAt(new Date("2026-10-23T10:00:10"));
  await signIn(page, "counter", "dev-counter-pass");
  const locked = page.getByTestId("sign-in-locked");
  await expect(locked).toContainText(en["hub.sign_in.locked"].replace("{seconds}", "3"));
  const submit = page.getByRole("button", { name: en["hub.sign_in.submit"] });
  await expect(submit).toHaveAttribute("aria-disabled", "true");
  await expect(page.getByText(en["hub.sign_in.paused"])).toBeVisible();
  await page.clock.runFor(1_000);
  await expect(locked).toContainText(en["hub.sign_in.locked"].replace("{seconds}", "2"));
  // Enter in the form while paused sends nothing.
  const tries = hub.signIns;
  await page.getByLabel(en["hub.sign_in.password"], { exact: true }).press("Enter");
  await submit.click({ force: true });
  expect(hub.signIns).toBe(tries);
  await page.clock.runFor(2_500);
  await expect(locked).toHaveCount(0);
  hub.lockedFor = null;
  await signIn(page, "counter", "dev-counter-pass");
  await expect(page).toHaveURL(/\/floor$/);
});

test("a Counter saving a setting gets the refusal inline, with no dialog", async ({ page }) => {
  const hub = await stubHub(page);
  await page.goto("/sign-in");
  await signIn(page, "counter", "dev-counter-pass");
  await expect(page).toHaveURL(/\/floor$/);
  await page.goto("/settings/receipt");
  const form = page.getByTestId("settings-receipt-footer");
  await form.getByLabel(en["hub.settings.receipt_footer"]).fill("Thanks for playing");
  await form.getByRole("button", { name: en["hub.settings.save"] }).click();
  const status = page.getByTestId("settings-status");
  await expect(status).toContainText(en["hub.refusal.permission"]);
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(status).not.toContainText("Counter");
  // The intent carried base_rev and the bearer token; the actor is the Hub's to decide.
  const sent = hub.intents.at(-1);
  expect(sent?.authorization).toMatch(/^Bearer token-/);
  const intent = (sent?.body as { intent: Record<string, unknown> }).intent;
  expect(intent.kind).toBe("settings.set");
  expect(intent.base_rev).toBe(1);
  expect(intent.payload).toEqual({
    kind: "settings.set",
    key: "receipt.footer",
    value: "Thanks for playing",
  });
});

test("a Manager saves the setting", async ({ page }) => {
  await stubHub(page);
  await page.goto("/sign-in");
  await signIn(page, "manager", "dev-manager-pass");
  await expect(page).toHaveURL(/\/floor$/);
  await page.goto("/settings/receipt");
  await page.getByRole("button", { name: en["hub.settings.save"] }).click();
  await expect(page.getByTestId("settings-status")).toContainText(en["hub.settings.saved"]);
});

test("sign-out returns to sign-in, and a revoked session does too", async ({ page }) => {
  const hub = await stubHub(page);
  await page.goto("/sign-in");
  await signIn(page, "manager", "dev-manager-pass");
  await expect(page).toHaveURL(/\/floor$/);
  await page.getByRole("button", { name: en["hub.header.sign_out"] }).click();
  await expect(page).toHaveURL(/\/sign-in(\?|$)/);
  expect(hub.sessions.size).toBe(0);

  // Signed in again, the Hub forgets the session (sign-out elsewhere, a Hub restart): the next
  // intent gets 401 and the UI goes to sign-in.
  await signIn(page, "manager", "dev-manager-pass");
  await expect(page).toHaveURL(/\/floor$/);
  await page.goto("/settings/receipt");
  await expect(page.locator("main h1")).toHaveText(en["hub.nav.settings"]);
  await expect(page.getByRole("button", { name: en["hub.settings.save"] })).not.toHaveAttribute(
    "aria-disabled",
    "true",
  );
  hub.sessions.clear();
  await page.getByRole("button", { name: en["hub.settings.save"] }).click();
  await expect(page).toHaveURL(/\/sign-in(\?|$)/);
  expect(hub.intents.at(-1)?.authorization).toMatch(/^Bearer token-/);
});

test("sign-in: landmarks, one h1 and no serious axe violations", async ({ page }) => {
  await stubHub(page);
  await page.goto("/sign-in");
  await expect(page.locator("h1")).toHaveCount(1);
  await expect(page.locator("header")).toHaveCount(1);
  await expect(page.locator("main")).toHaveCount(1);
  await signIn(page, "counter", "bad");
  await expect(page.getByTestId("sign-in-error")).toBeVisible();
  const results = await new AxeBuilder({ page }).analyze();
  const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  expect(bad.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual(
    [],
  );
});

async function signedInAs(page: Page, login: "manager" | "counter", path = "/sign-in") {
  await page.goto(path);
  await signIn(page, login, STAFF[login].password);
}

test("sign-in returns to the screen that sent you there", async ({ page }) => {
  await stubHub(page);
  await page.goto("/players");
  await expect(page).toHaveURL(/\/sign-in\?redirectTo=%2Fplayers$/);
  await signIn(page, "counter", "dev-counter-pass");
  await expect(page).toHaveURL(/\/players$/);
});

test("a /ui/me failure shows the retry state, not the shell", async ({ page }) => {
  const hub = await stubHub(page);
  await signedInAs(page, "manager");
  await expect(page).toHaveURL(/\/floor$/);
  hub.meStatus = 500;
  await page.reload();
  await expect(page.getByTestId("hub-unreachable")).toBeVisible();
  await expect(page.locator("nav")).toHaveCount(0);
  await expect(page.getByTestId("hub-staff")).toHaveCount(0);
  hub.meStatus = null;
  await page.getByRole("button", { name: en["common.retry"] }).click();
  await expect(page.getByTestId("hub-staff")).toHaveText("Manager · Manager");
});

test("a malformed sign-in answer signs nothing in and ends its token", async ({ page }) => {
  const hub = await stubHub(page);
  await page.goto("/sign-in");
  hub.nextSignIn = { status: 200, body: { token: "half-token" } };
  await signIn(page, "manager", "dev-manager-pass");
  await expect(page.getByTestId("sign-in-error")).toContainText(en["hub.sign_in.unreachable"]);
  await expect(page).toHaveURL(/\/sign-in(\?|$)/);
  expect(hub.signOuts).toContain("Bearer half-token");
  expect(await page.evaluate(() => sessionStorage.getItem("arena.hub.token"))).toBeNull();
});

test("a STALE save keeps the typed text and shows the Hub's value", async ({ page }) => {
  const hub = await stubHub(page);
  await signedInAs(page, "manager");
  await expect(page).toHaveURL(/\/floor$/);
  await page.goto("/settings/receipt");
  const field = page.getByLabel(en["hub.settings.receipt_footer"]);
  await expect(page.getByRole("button", { name: en["hub.settings.save"] })).not.toHaveAttribute(
    "aria-disabled",
    "true",
  );
  // Someone else saved meanwhile.
  hub.setting = { value: "from elsewhere", rev: 2 };
  hub.nextIntent = { status: 409, body: { code: "STALE", message: "stale", field: "base_rev" } };
  await field.fill("my text");
  await page.getByRole("button", { name: en["hub.settings.save"] }).click();
  await expect(page.getByTestId("settings-status")).toContainText(en["hub.settings.stale"]);
  await expect(field).toHaveValue("my text");
  await expect(page.getByTestId("settings-server-value")).toContainText("from elsewhere");
  // Saving again sends the reloaded rev.
  await page.getByRole("button", { name: en["hub.settings.save"] }).click();
  await expect(page.getByTestId("settings-status")).toContainText(en["hub.settings.saved"]);
  const last = hub.intents.at(-1)?.body as { intent: { base_rev: number } };
  expect(last.intent.base_rev).toBe(2);
});

test("two saves send base_rev from the reloaded settings", async ({ page }) => {
  const hub = await stubHub(page);
  await signedInAs(page, "manager");
  await expect(page).toHaveURL(/\/floor$/);
  await page.goto("/settings/receipt");
  const field = page.getByLabel(en["hub.settings.receipt_footer"]);
  const save = page.getByRole("button", { name: en["hub.settings.save"] });
  for (const text of ["first", "second"]) {
    await expect(save).not.toHaveAttribute("aria-disabled", "true");
    await field.fill(text);
    await save.click();
    await expect(page.getByTestId("settings-status")).toContainText(en["hub.settings.saved"]);
    await expect.poll(() => hub.setting.value).toBe(text);
  }
  const revs = hub.intents.map((i) => (i.body as { intent: { base_rev: number } }).intent.base_rev);
  expect(revs).toEqual([1, 2]);
  const keys = hub.intents.map(
    (i) => (i.body as { intent: { idempotency_key: string } }).intent.idempotency_key,
  );
  expect(new Set(keys).size).toBe(2);
});

test("settings that fail to load offer Retry", async ({ page }) => {
  await stubHub(page);
  await signedInAs(page, "manager");
  await expect(page).toHaveURL(/\/floor$/);
  let fail = true;
  await page.route(`${HUB}/ui/settings`, (route) =>
    fail ? json(route, 500, { code: "INTERNAL" }) : route.fallback(),
  );
  await page.goto("/settings/receipt");
  const failed = page.getByTestId("settings-load-failed");
  await expect(failed).toContainText(en["hub.settings.load_failed"]);
  fail = false;
  await failed.getByRole("button", { name: en["common.retry"] }).click();
  await expect(page.getByRole("button", { name: en["hub.settings.save"] })).toBeVisible();
});
