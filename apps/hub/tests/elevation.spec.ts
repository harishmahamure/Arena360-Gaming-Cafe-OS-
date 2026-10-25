import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type { HubSnapshot, SessionStaff } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { ORIGIN } from "./origin";

// Inline manager PIN elevation on Settings → Branch: a Counter edits, Save opens the PIN in
// place, `/ui/elevate` answers a ticket and every `settings.set` carries it. Keyboard only,
// Esc, a rejected PIN, a PIN rotated between elevate and intent (PIN_REQUIRED), and the
// lockout countdown. The Hub's loopback API is stubbed with `page.route`; the manager PIN in
// the stub is 2222.

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
    permissions: ["settings.edit", "user.manage"],
  },
  "counter-token": {
    staff_id: "01JCOUNTER0000000000000000",
    display_name: "Counter",
    role_name: "Counter",
    permissions: ["session.start", "pos.sell"],
  },
};
const AUTH_FAILED = { code: "AUTH_FAILED", message: "sign-in failed" };
const PIN_REJECTED = { code: "PIN_REJECTED", message: "that PIN can't approve this" };
const PIN_REQUIRED = { code: "PIN_REQUIRED", message: "enter the PIN again" };

type Item = { intent_kind: string; key?: string };
/** The approved items not yet used. */
type Ticket = { items: Item[] };
type Hub = {
  settings: Map<string, { value: string; rev: number }>;
  intents: { key: string; value: string; ticket: string | undefined; elevated: boolean }[];
  elevations: { items: Item[]; pin: string }[];
  tickets: Map<string, Ticket>;
  dropped: string[];
  failures: number;
  /** The manager's PIN changes after the next elevation (the ticket's version goes stale). */
  rotateAfterElevate: boolean;
  managerPin: string;
  /** Holds the next `/ui/elevate` answer until this resolves. */
  holdElevate: Promise<void> | null;
  /** One-shot answer replacing the next `/ui/elevate` reply. */
  nextElevate: { status: number; body: unknown } | null;
  /** One-shot answer replacing the next elevated `/ui/intent` reply (for `key` only, if set). */
  nextIntent: { status: number; body: unknown; key?: string } | null;
  /** Ticket lifetime the stub reports. */
  expiresInS: number;
};

function newHub(): Hub {
  return {
    settings: new Map([
      ["receipt.footer", { value: "", rev: 1 }],
      ["branch.currency", { value: "INR", rev: 1 }],
      ["branch.symbol_position", { value: "before", rev: 1 }],
      ["branch.number_format", { value: "indian", rev: 1 }],
      ["branch.tz", { value: "Asia/Kolkata", rev: 1 }],
      ["branch.day_start_min", { value: "360", rev: 1 }],
      ["branch.fy_start_month", { value: "4", rev: 1 }],
      ["idle_lock_minutes", { value: "5", rev: 1 }],
    ]),
    intents: [],
    elevations: [],
    tickets: new Map(),
    dropped: [],
    holdElevate: null,
    nextElevate: null,
    nextIntent: null,
    expiresInS: 60,
    failures: 0,
    rotateAfterElevate: false,
    managerPin: "2222",
  };
}

function snapshot(): HubSnapshot {
  return {
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
  };
}

function json(route: Route, status: number, body: unknown) {
  return route.fulfill({
    status,
    contentType: "application/json",
    headers: CORS,
    body: JSON.stringify(body),
  });
}

async function stub(page: Page, hub: Hub, token: string) {
  await page.addInitScript((t) => sessionStorage.setItem("arena.hub.token", t), token);
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    const url = new URL(req.url());
    const auth = (req.headers()["authorization"] ?? "").replace(/^Bearer /, "");
    switch (url.pathname) {
      case "/ui/snapshot":
        return json(route, 200, snapshot());
      case "/ui/me":
        return STAFF[auth] ? json(route, 200, STAFF[auth]) : json(route, 401, AUTH_FAILED);
      case "/ui/settings":
        if (!STAFF[auth]) return json(route, 401, AUTH_FAILED);
        return json(
          route,
          200,
          [...hub.settings].map(([key, row]) => ({ key, ...row })),
        );
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: "retry: 60000\n\n",
        });
      case "/ui/elevate": {
        if (!STAFF[auth]) return json(route, 401, AUTH_FAILED);
        const body = req.postDataJSON() as { items: Item[]; pin: string };
        hub.elevations.push(body);
        if (hub.holdElevate) {
          const hold = hub.holdElevate;
          hub.holdElevate = null;
          await hold;
        }
        if (hub.nextElevate) {
          const answer = hub.nextElevate;
          hub.nextElevate = null;
          return json(route, answer.status, answer.body);
        }
        if (hub.failures >= 5) {
          return json(route, 429, {
            code: "AUTH_LOCKED",
            message: "too many wrong PINs",
            retry_after_s: 60,
          });
        }
        if (body.pin !== hub.managerPin) {
          hub.failures++;
          if (hub.failures >= 5) {
            return json(route, 429, {
              code: "AUTH_LOCKED",
              message: "too many wrong PINs",
              retry_after_s: 60,
            });
          }
          return json(route, 403, PIN_REJECTED);
        }
        hub.failures = 0;
        const ticket = `T${hub.tickets.size + 1}`;
        hub.tickets.set(ticket, { items: [...body.items] });
        if (hub.rotateAfterElevate) {
          hub.rotateAfterElevate = false;
          hub.managerPin = "2468";
          hub.tickets.clear(); // every ticket now names a stale verifier version
        }
        return json(route, 200, { ticket, items: body.items, expires_in_s: hub.expiresInS });
      }
      case "/ui/elevate/drop": {
        const { ticket } = req.postDataJSON() as { ticket: string };
        hub.dropped.push(ticket);
        hub.tickets.delete(ticket);
        return route.fulfill({ status: 204, headers: CORS });
      }
      case "/ui/intent": {
        if (!STAFF[auth]) return json(route, 401, AUTH_FAILED);
        const body = req.postDataJSON() as {
          intent: {
            base_rev?: number;
            elevated_by?: unknown;
            payload: { key: string; value: string };
          };
          elevation_ticket?: string;
        };
        const { key, value } = body.intent.payload;
        const ticketId = body.elevation_ticket;
        let elevated = false;
        if (ticketId !== undefined) {
          const t = hub.tickets.get(ticketId);
          const at = t?.items.findIndex((i) => i.intent_kind === "settings.set" && i.key === key);
          if (!t || at === undefined || at < 0) {
            hub.intents.push({ key, value, ticket: ticketId, elevated: false });
            return json(route, 403, PIN_REQUIRED);
          }
          t.items.splice(at, 1);
          elevated = true;
        }
        hub.intents.push({ key, value, ticket: ticketId, elevated });
        if (elevated && hub.nextIntent && (hub.nextIntent.key ?? key) === key) {
          const answer = hub.nextIntent;
          hub.nextIntent = null;
          return json(route, answer.status, answer.body);
        }
        if (!elevated && !STAFF[auth].permissions.includes("settings.edit")) {
          return json(route, 403, { code: "PERMISSION_DENIED", message: "denied" });
        }
        const row = hub.settings.get(key)!;
        if (body.intent.base_rev !== row.rev) {
          return json(route, 409, { code: "STALE", message: "stale", field: "base_rev" });
        }
        hub.settings.set(key, { value, rev: row.rev + 1 });
        return json(route, 200, { applied: { intent_id: "I", changes: [] } });
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
}

async function openBranch(page: Page, hub: Hub, token = "counter-token") {
  await stub(page, hub, token);
  await page.goto("/settings/branch");
  await expect(page.getByLabel(en["hub.settings.branch.currency"])).toHaveValue("INR");
}

const save = (page: Page) => page.getByRole("button", { name: en["hub.settings.save"] });
const status = (page: Page) => page.getByTestId("branch-status");
const approval = (page: Page) => page.getByTestId("branch-approval");
const pinInput = (page: Page) => approval(page).getByLabel(en["ui.pin.label"]);

/** Keyboard only: type-to-select another currency and FY month on the focused selects. */
async function editTwoFieldsByKeyboard(page: Page) {
  await page.getByLabel(en["hub.settings.branch.currency"]).focus();
  await page.keyboard.type("USD");
  await page.keyboard.press("Tab");
  await page.getByLabel(en["hub.settings.branch.fy_start"]).focus();
  await page.keyboard.type("Jan");
  await page.keyboard.press("Tab");
}

test("a Counter edits, Save asks for a manager's PIN in place, and every intent carries the ticket", async ({
  page,
}) => {
  const hub = newHub();
  await openBranch(page, hub);
  let navigations = 0;
  page.on("framenavigated", () => navigations++);
  // The fields are editable; Save says it needs approval.
  await expect(page.getByLabel(en["hub.settings.branch.currency"])).toBeEnabled();
  await expect(page.getByTestId("branch-needs-approval")).toHaveText(
    en["hub.settings.branch.needs_approval"],
  );
  await editTwoFieldsByKeyboard(page);
  const currency = await page.getByLabel(en["hub.settings.branch.currency"]).inputValue();
  expect(currency).not.toBe("INR");
  await save(page).focus();
  await page.keyboard.press("Enter");
  // The PIN opens in the panel, focused, without navigating.
  await expect(approval(page)).toBeVisible();
  await expect(pinInput(page)).toBeFocused();
  expect(hub.intents).toEqual([]);
  // The panel says what the PIN approves.
  await expect(page.getByTestId("branch-approval-items")).toHaveText(
    en["hub.settings.branch.approves"].replace(
      "{fields}",
      [en["hub.settings.branch.currency"], en["hub.settings.branch.fy_start"]].join(", "),
    ),
  );
  await page.keyboard.type("2222");
  await page.keyboard.press("Enter");
  await expect(status(page)).toContainText(en["hub.settings.saved"]);
  await expect(approval(page)).toHaveCount(0);
  await expect(save(page)).toBeFocused();
  // Every approval was used: nothing to give up.
  expect(hub.dropped).toEqual([]);
  expect(navigations).toBe(0);
  // One elevation for exactly the two fields; each settings.set redeemed the same ticket.
  expect(hub.elevations).toEqual([
    {
      items: [
        { intent_kind: "settings.set", key: "branch.currency" },
        { intent_kind: "settings.set", key: "branch.fy_start_month" },
      ],
      pin: "2222",
    },
  ]);
  expect(hub.intents.map((i) => [i.key, i.ticket, i.elevated])).toEqual([
    ["branch.currency", "T1", true],
    ["branch.fy_start_month", "T1", true],
  ]);
  expect(hub.settings.get("branch.currency")?.value).toBe(currency);
  // The PIN is kept nowhere on the client.
  const stored = await page.evaluate(() => JSON.stringify({ ...sessionStorage, ...localStorage }));
  expect(stored).not.toContain("2222");
});

test("a wrong PIN is refused inline with the generic message and nothing is saved", async ({
  page,
}) => {
  const hub = newHub();
  await openBranch(page, hub);
  await page.getByLabel(en["hub.settings.branch.currency"]).selectOption("USD");
  await save(page).click();
  await pinInput(page).focus();
  await page.keyboard.type("9999");
  await page.keyboard.press("Enter");
  await expect(approval(page)).toContainText(en["ui.pin.rejected"]);
  await expect(approval(page).locator(".cell.filled")).toHaveCount(0);
  await expect(pinInput(page)).toHaveValue("");
  await expect(pinInput(page)).toBeFocused();
  expect(hub.intents).toEqual([]);
  // The edit is still there; the right PIN saves it.
  await page.keyboard.type("2222");
  await page.keyboard.press("Enter");
  await expect(status(page)).toContainText(en["hub.settings.saved"]);
  expect(hub.settings.get("branch.currency")?.value).toBe("USD");
});

test("Esc clears the PIN, closes it and returns focus to Save, keeping the edits", async ({
  page,
}) => {
  const hub = newHub();
  await openBranch(page, hub);
  await page.getByLabel(en["hub.settings.branch.currency"]).selectOption("USD");
  await save(page).focus();
  await page.keyboard.press("Enter");
  await expect(pinInput(page)).toBeFocused();
  await page.keyboard.type("22");
  await page.keyboard.press("Escape");
  await expect(approval(page)).toHaveCount(0);
  await expect(save(page)).toBeFocused();
  await expect(page.getByLabel(en["hub.settings.branch.currency"])).toHaveValue("USD");
  expect(hub.elevations).toEqual([]);
  expect(hub.intents).toEqual([]);
  // Opening it again starts empty.
  await page.keyboard.press("Enter");
  await expect(approval(page).locator(".cell.filled")).toHaveCount(0);
});

test("a PIN rotated between elevate and intent asks for the PIN again", async ({ page }) => {
  const hub = newHub();
  hub.rotateAfterElevate = true;
  await openBranch(page, hub);
  await page.getByLabel(en["hub.settings.branch.currency"]).selectOption("USD");
  await save(page).click();
  await pinInput(page).fill("2222");
  await pinInput(page).press("Enter");
  // The Hub answered PIN_REQUIRED: the PIN reopens with its message, the edit is kept.
  await expect(approval(page)).toContainText(en["ui.pin.again"]);
  await expect(pinInput(page)).toBeFocused();
  await expect(page.getByLabel(en["hub.settings.branch.currency"])).toHaveValue("USD");
  expect(hub.settings.get("branch.currency")?.value).toBe("INR");
  await page.keyboard.type("2468");
  await page.keyboard.press("Enter");
  await expect(status(page)).toContainText(en["hub.settings.saved"]);
  expect(hub.settings.get("branch.currency")?.value).toBe("USD");
});

test("five wrong PINs pause entry with a countdown", async ({ page }) => {
  await page.clock.install();
  const hub = newHub();
  await openBranch(page, hub);
  await page.getByLabel(en["hub.settings.branch.currency"]).selectOption("USD");
  await save(page).click();
  for (let i = 0; i < 5; i++) {
    await pinInput(page).focus();
    await page.keyboard.type("9999");
    await page.keyboard.press("Enter");
    await expect.poll(() => hub.elevations.length).toBe(i + 1);
    // The answer is in once the PIN accepts input again (or entry is paused).
    if (i < 4) await expect(pinInput(page)).not.toHaveAttribute("readonly");
  }
  const locked = (s: number) => en["ui.pin.locked"].replace("{seconds}", String(s));
  await expect(approval(page)).toContainText(locked(60));
  const key = approval(page).getByRole("button", { name: "1", exact: true });
  await expect(key).toHaveAttribute("aria-disabled", "true");
  // Typing does nothing while paused.
  await pinInput(page).focus();
  await page.keyboard.type("2222");
  await expect(approval(page).locator(".cell.filled")).toHaveCount(0);
  await page.clock.runFor(1000);
  await expect(approval(page)).toContainText(locked(59));
  await page.clock.runFor(59_000);
  await expect(approval(page)).not.toContainText(locked(1));
  await expect(key).not.toHaveAttribute("aria-disabled", "true");
  expect(hub.intents).toEqual([]);
});

test("a manager saves without a PIN", async ({ page }) => {
  const hub = newHub();
  await openBranch(page, hub, "manager-token");
  await expect(page.getByTestId("branch-needs-approval")).toHaveCount(0);
  await page.getByLabel(en["hub.settings.branch.currency"]).selectOption("USD");
  await save(page).click();
  await expect(status(page)).toContainText(en["hub.settings.saved"]);
  await expect(approval(page)).toHaveCount(0);
  expect(hub.elevations).toEqual([]);
  expect(hub.intents).toEqual([
    { key: "branch.currency", value: "USD", ticket: undefined, elevated: false },
  ]);
});

test("axe: the open PIN on Settings → Branch", async ({ page }) => {
  const hub = newHub();
  await openBranch(page, hub);
  await page.getByLabel(en["hub.settings.branch.currency"]).selectOption("USD");
  await save(page).click();
  await expect(approval(page)).toBeVisible();
  const results = await new AxeBuilder({ page }).analyze();
  const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  expect(bad.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual(
    [],
  );
});

test("a 6-digit manager PIN approves, submitting on its last digit", async ({ page }) => {
  const hub = newHub();
  hub.managerPin = "246810";
  await openBranch(page, hub);
  await page.getByLabel(en["hub.settings.branch.currency"]).selectOption("USD");
  await save(page).click();
  await pinInput(page).focus();
  await page.keyboard.type("246810");
  await expect(status(page)).toContainText(en["hub.settings.saved"]);
  expect(hub.elevations.map((e) => e.pin)).toEqual(["246810"]);
  expect(hub.settings.get("branch.currency")?.value).toBe("USD");
});

test("Esc while the PIN is being checked does nothing, and the answer still saves", async ({
  page,
}) => {
  const hub = newHub();
  let release!: () => void;
  hub.holdElevate = new Promise((r) => (release = r));
  await openBranch(page, hub);
  await page.getByLabel(en["hub.settings.branch.currency"]).selectOption("USD");
  await save(page).click();
  await pinInput(page).focus();
  await page.keyboard.type("2222");
  await page.keyboard.press("Enter");
  await expect(approval(page)).toContainText(en["ui.pin.checking"]);
  // Esc and Cancel are held off while checking.
  await page.keyboard.press("Escape");
  const cancel = approval(page).getByRole("button", { name: en["ui.pin.cancel"] });
  await expect(cancel).toHaveAttribute("aria-disabled", "true");
  await expect(approval(page)).toBeVisible();
  expect(hub.intents).toEqual([]);
  release();
  await expect(status(page)).toContainText(en["hub.settings.saved"]);
  expect(hub.intents.map((i) => i.key)).toEqual(["branch.currency"]);
});

test("a refused elevated save gives up the unused approvals and reports what was saved", async ({
  page,
}) => {
  const hub = newHub();
  await openBranch(page, hub);
  await page.getByLabel(en["hub.settings.branch.currency"]).selectOption("USD");
  await page.getByLabel(en["hub.settings.branch.fy_start"]).selectOption("1");
  await save(page).click();
  await pinInput(page).fill("2222");
  // The first field saves; then the Hub answers PERMISSION_DENIED (the approver lost it).
  hub.nextIntent = {
    status: 403,
    body: { code: "PERMISSION_DENIED", message: "denied" },
    key: "branch.fy_start_month",
  };
  await pinInput(page).press("Enter");
  await expect(status(page)).toContainText(
    en["hub.settings.branch.partial"]
      .replace("{saved}", en["hub.settings.branch.currency"])
      .replace("{failed}", en["hub.settings.branch.fy_start"]),
  );
  await expect.poll(() => hub.dropped).toEqual(["T1"]);
  await expect(page.getByLabel(en["hub.settings.branch.fy_start"])).toHaveValue("1");
});

test("PERMISSION_DENIED on an elevated save says only a manager can change it", async ({
  page,
}) => {
  const hub = newHub();
  hub.nextIntent = { status: 403, body: { code: "PERMISSION_DENIED", message: "denied" } };
  await openBranch(page, hub);
  await page.getByLabel(en["hub.settings.branch.currency"]).selectOption("USD");
  await save(page).click();
  await pinInput(page).fill("2222");
  await pinInput(page).press("Enter");
  await expect(status(page)).toContainText(en["hub.settings.branch.manager_only"]);
  await expect.poll(() => hub.dropped).toEqual(["T1"]);
});

test("an approval that lapsed asks for the PIN again without sending anything", async ({
  page,
}) => {
  const hub = newHub();
  hub.expiresInS = 0;
  await openBranch(page, hub);
  await page.getByLabel(en["hub.settings.branch.currency"]).selectOption("USD");
  await save(page).click();
  await pinInput(page).fill("2222");
  await pinInput(page).press("Enter");
  await expect(approval(page)).toContainText(en["ui.pin.again"]);
  expect(hub.intents).toEqual([]);
  await expect.poll(() => hub.dropped).toEqual(["T1"]);
});

test("an unexpected elevate answer shows the generic failure, not 'unreachable'", async ({
  page,
}) => {
  const hub = newHub();
  hub.nextElevate = { status: 500, body: { code: "INTERNAL", message: "x" } };
  await openBranch(page, hub);
  await page.getByLabel(en["hub.settings.branch.currency"]).selectOption("USD");
  await save(page).click();
  await pinInput(page).fill("2222");
  await pinInput(page).press("Enter");
  await expect(approval(page)).toContainText(en["ui.pin.failed"]);
  await expect(approval(page)).not.toContainText(en["ui.pin.unreachable"]);
  expect(hub.intents).toEqual([]);
});
