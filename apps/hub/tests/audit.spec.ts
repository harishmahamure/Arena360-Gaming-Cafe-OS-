import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type {
  AuditDetail,
  AuditItem,
  AuditPage,
  HubSnapshot,
  SessionStaff,
} from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { ORIGIN } from "./origin";

// System → Audit: filters, newest-first pages with "Load more" and scroll-to-load, the
// details panel (before/after, "Changed" markers, "Created", hidden secrets, the approver),
// the empty state, a read-only screen, and a Counter's manager PIN read approval (nothing is
// fetched before it, or after a cancel). The Hub's loopback API is stubbed with `page.route`;
// the manager PIN in the stub is 2222 and the branch time zone is Asia/Kolkata.

const HUB = "http://127.0.0.1:7409";
const CORS = {
  "access-control-allow-origin": ORIGIN,
  "access-control-allow-methods": "GET, POST",
  "access-control-allow-headers": "authorization, content-type, x-arena-elevation",
};
const MANAGER_ID = "01JMANAGER0000000000000000";
const COUNTER_ID = "01JCOUNTER0000000000000000";
const STAFF: Record<string, SessionStaff> = {
  "manager-token": {
    staff_id: MANAGER_ID,
    display_name: "Manager",
    role_name: "Manager",
    permissions: ["settings.edit", "user.manage", "audit.view"],
  },
  "counter-token": {
    staff_id: COUNTER_ID,
    display_name: "Counter",
    role_name: "Counter",
    permissions: ["session.start", "pos.sell", "player.view"],
  },
};
const KITCHEN: SessionStaff = {
  staff_id: "01JKITCHEN0000000000000000",
  display_name: "Kitchen",
  role_name: "Kitchen",
  permissions: ["kitchen.use"],
};
/** Whose PIN unlocks a locked screen (another member's switches it to them). */
const UNLOCK_PINS: Record<string, SessionStaff> = {
  "2222": STAFF["manager-token"]!,
  "3333": STAFF["counter-token"]!,
  "4444": KITCHEN,
};
const AUTH_FAILED = { code: "AUTH_FAILED", message: "sign-in failed" };
const DENIED = { code: "PERMISSION_DENIED", message: "needs approval" };
/** 2026-10-21 12:00 in Kolkata. */
const NEWEST = Date.UTC(2026, 9, 21, 6, 30);

/** 120 changes, newest first, one a minute apart. */
function changes(): AuditItem[] {
  const out: AuditItem[] = [];
  for (let i = 0; i < 120; i++) {
    const seq = 120 - i;
    const pick = i % 3;
    const base = {
      epoch: "1:01JHUB",
      seq,
      at: NEWEST - i * 60_000,
      business_day: null,
      intent_id: `I${seq}`,
      op: "update" as const,
    };
    if (i === 0) {
      out.push({
        ...base,
        actor: { kind: "staff", id: COUNTER_ID, name: "Counter" },
        elevated_by: { kind: "pin", id: MANAGER_ID, name: "Manager", mode: "hub" },
        module: "settings",
        intent_kind: "settings.set",
        tbl: "settings",
        row_id: "01JBRANCH/branch.currency",
      });
    } else if (pick === 0) {
      out.push({
        ...base,
        actor: { kind: "staff", id: MANAGER_ID, name: "Manager" },
        elevated_by: null,
        module: "settings",
        intent_kind: "settings.set",
        tbl: "settings",
        row_id: "01JBRANCH/receipt.footer",
      });
    } else if (pick === 1) {
      out.push({
        ...base,
        actor: { kind: "owner", id: "owner", name: "hub.audit.actor.owner" },
        elevated_by: null,
        module: "staff",
        intent_kind: "staff.upsert",
        tbl: "staff",
        row_id: COUNTER_ID,
      });
    } else {
      out.push({
        ...base,
        op: "insert",
        actor: { kind: "system", id: "hub", name: "hub.audit.actor.system" },
        elevated_by: null,
        module: "branch",
        intent_kind: "branch.bootstrap",
        tbl: "branch",
        row_id: "01JBRANCH",
      });
    }
  }
  return out;
}

function detailOf(item: AuditItem): AuditDetail {
  if (item.tbl === "branch") {
    return {
      item,
      before: null,
      after: { branch_id: "01JBRANCH", name: "Dev Branch", rev: 1 },
      changed: [],
    };
  }
  if (item.tbl === "staff") {
    return {
      item,
      before: { login: "counter", pin_hash: "[redacted]", password_hash: "[redacted]", rev: 1 },
      after: { login: "counter", pin_hash: "[redacted]", password_hash: "[redacted]", rev: 2 },
      changed: ["pin_hash", "rev"],
    };
  }
  return {
    item,
    before: { key: "branch.currency", value: "INR", rev: 1, updated_at: 1 },
    after: { key: "branch.currency", value: "USD", rev: 2, updated_at: 2 },
    changed: ["rev", "updated_at", "value"],
  };
}

type Hub = {
  all: AuditItem[];
  /** Every audit request: its path, query and elevation header. */
  requests: { path: string; query: URLSearchParams; ticket: string | undefined }[];
  elevations: { items: unknown[]; pin: string }[];
  tickets: Set<string>;
  dropped: string[];
  /** Who each token acts as now, and whether it is idle-locked. */
  sessions: Map<string, { staff: SessionStaff; locked: boolean }>;
  /** Items per page the stub answers, whatever the limit asked. */
  pageSize: number | null;
  /** `/ui/audit/item` answers 500 while set. */
  failItem: boolean;
  /** The next `/ui/audit` page with a cursor answers 500. */
  failNextMore: boolean;
};

const newHub = (): Hub => ({
  all: changes(),
  requests: [],
  elevations: [],
  tickets: new Set(),
  dropped: [],
  sessions: new Map(
    Object.entries(STAFF).map(([token, staff]) => [token, { staff, locked: false }]),
  ),
  pageSize: null,
  failItem: false,
  failNextMore: false,
});

function snapshot(): HubSnapshot {
  return {
    branch: { name: "Dev Branch", branch_code: "DEV01" },
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

function filtered(hub: Hub, q: URLSearchParams): AuditItem[] {
  return hub.all.filter((i) => {
    const actor = q.get("actor");
    if (actor) {
      const [kind, id] = actor.split(":");
      if (i.actor.kind !== kind || (id !== undefined && i.actor.id !== id)) return false;
    }
    const module = q.get("module");
    if (module && i.module !== module) return false;
    const from = q.get("from");
    if (from && i.at < Number(from)) return false;
    const to = q.get("to");
    if (to && i.at >= Number(to)) return false;
    return true;
  });
}

async function stub(page: Page, hub: Hub, token: string) {
  await page.addInitScript((t) => sessionStorage.setItem("arena.hub.token", t), token);
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    const url = new URL(req.url());
    const headers = req.headers();
    const auth = (headers["authorization"] ?? "").replace(/^Bearer /, "");
    const session = hub.sessions.get(auth);
    const me = session?.staff;
    if (url.pathname.startsWith("/ui/audit")) {
      const ticket = headers["x-arena-elevation"];
      hub.requests.push({ path: url.pathname, query: url.searchParams, ticket });
      if (!me) return json(route, 401, AUTH_FAILED);
      if (session.locked) return json(route, 423, { code: "SESSION_LOCKED", message: "locked" });
      const allowed =
        me.permissions.includes("audit.view") || (ticket !== undefined && hub.tickets.has(ticket));
      if (!allowed) return json(route, 403, DENIED);
    }
    switch (url.pathname) {
      case "/ui/snapshot":
        return json(route, 200, snapshot());
      case "/ui/me":
        return me
          ? json(route, 200, { ...me, locked: session.locked, has_pin: true })
          : json(route, 401, AUTH_FAILED);
      case "/ui/lock":
        if (!session) return json(route, 401, AUTH_FAILED);
        session.locked = true;
        hub.tickets.clear(); // a lock drops the session's tickets
        return route.fulfill({ status: 204, headers: CORS });
      case "/ui/unlock": {
        if (!session) return json(route, 401, AUTH_FAILED);
        const { pin } = req.postDataJSON() as { pin: string };
        const match = UNLOCK_PINS[pin];
        if (!match) return json(route, 403, { code: "PIN_REJECTED", message: "no" });
        const switched = match.staff_id !== session.staff.staff_id;
        session.staff = match;
        session.locked = false;
        return json(route, 200, { staff: match, switched });
      }
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: "retry: 60000\n\n",
        });
      case "/ui/elevate": {
        if (!me) return json(route, 401, AUTH_FAILED);
        const body = req.postDataJSON() as { items: unknown[]; pin: string };
        hub.elevations.push(body);
        if (body.pin !== "2222") {
          return json(route, 403, { code: "PIN_REJECTED", message: "that PIN can't approve this" });
        }
        const ticket = `R${hub.tickets.size + 1}`;
        hub.tickets.add(ticket);
        return json(route, 200, { ticket, items: body.items, expires_in_s: 600 });
      }
      case "/ui/elevate/drop": {
        const { ticket } = req.postDataJSON() as { ticket: string };
        hub.dropped.push(ticket);
        hub.tickets.delete(ticket);
        return route.fulfill({ status: 204, headers: CORS });
      }
      case "/ui/audit/facets":
        return json(route, 200, {
          staff: [
            { id: COUNTER_ID, name: "Counter" },
            { id: MANAGER_ID, name: "Manager" },
          ],
          modules: ["branch", "settings", "device", "console", "staff", "role"],
        });
      case "/ui/audit": {
        const q = url.searchParams;
        if (q.get("cursor") && hub.failNextMore) {
          hub.failNextMore = false;
          return json(route, 500, { code: "INTERNAL", message: "boom" });
        }
        const rows = filtered(hub, q);
        const start = Number(q.get("cursor") ?? "0");
        const limit = hub.pageSize ?? Number(q.get("limit") ?? "50");
        const items = rows.slice(start, start + limit);
        const next = start + limit < rows.length ? String(start + limit) : null;
        const page: AuditPage = { items, next_cursor: next };
        return json(route, 200, page);
      }
      case "/ui/audit/item": {
        if (hub.failItem) return json(route, 500, { code: "INTERNAL", message: "boom" });
        const q = url.searchParams;
        const item = hub.all.find(
          (i) => i.epoch === q.get("epoch") && String(i.seq) === q.get("seq"),
        );
        return item
          ? json(route, 200, detailOf(item))
          : json(route, 422, {
              code: "VALIDATION_FAILED",
              message: "no such change",
              field: "seq",
            });
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
}

const rows = (page: Page) => page.getByTestId("audit-row");
const listRequests = (hub: Hub) => hub.requests.filter((r) => r.path === "/ui/audit");

async function openAudit(page: Page, hub: Hub, token = "manager-token") {
  await stub(page, hub, token);
  await page.goto("/system");
  await expect(page).toHaveURL(/\/system\/audit$/);
}

async function expectNoSeriousAxe(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  expect(bad.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual(
    [],
  );
}

test("System opens Audit: newest first, Load more, and the next page on scroll", async ({
  page,
}) => {
  const hub = newHub();
  await openAudit(page, hub);
  await expect(page.locator("main h1")).toHaveText(en["hub.nav.system"]);
  await expect(
    page
      .getByRole("navigation", { name: en["hub.system.sections"] })
      .getByRole("link", { name: en["hub.system.nav.audit"] }),
  ).toHaveAttribute("aria-current", "page");
  await expect(rows(page)).toHaveCount(50);
  // Newest first: the counter's elevated currency change, then older ones.
  await expect(rows(page).first()).toContainText("Counter");
  await expect(rows(page).first()).toContainText("settings.set");
  await expect(rows(page).first()).toContainText("settings / 01JBRANCH/branch.currency");
  // Fixed actors show their catalogue names, with the kind.
  await expect(rows(page).nth(1)).toContainText(en["hub.audit.actor.owner"]);
  await expect(rows(page).nth(2)).toContainText(en["hub.audit.actor.system"]);
  await expect(rows(page).nth(2)).toContainText(en["hub.audit.module.branch"]);
  await page.getByRole("button", { name: en["hub.audit.load_more"] }).click();
  await expect(rows(page)).toHaveCount(100);
  expect(listRequests(hub).at(-1)?.query.get("cursor")).toBe("50");
  // The last page loads when the end of the list scrolls into view.
  await page.getByTestId("audit-row").last().scrollIntoViewIfNeeded();
  await page.mouse.wheel(0, 4000);
  await expect(rows(page)).toHaveCount(120);
  await expect(page.getByText(en["hub.audit.loaded_all"])).toBeVisible();
  await expect(page.getByRole("button", { name: en["hub.audit.load_more"] })).toHaveCount(0);
  // Read only: no edit, delete or export controls.
  for (const word of [/edit/i, /delete/i, /export/i, /remove/i]) {
    await expect(page.getByRole("button", { name: word })).toHaveCount(0);
  }
  await expectNoSeriousAxe(page);
});

test("filters combine, dates are branch days, and Clear filters resets them all", async ({
  page,
}) => {
  const hub = newHub();
  await openAudit(page, hub);
  await expect(rows(page)).toHaveCount(50);
  await page
    .getByLabel(en["hub.audit.filter.actor"], { exact: true })
    .selectOption({ label: "Counter" });
  await page.getByLabel(en["hub.audit.filter.module"], { exact: true }).selectOption({
    label: en["hub.audit.module.settings"],
  });
  await page.getByLabel(en["hub.audit.filter.from"], { exact: true }).fill("2026-10-21");
  await page.getByLabel(en["hub.audit.filter.to"], { exact: true }).fill("2026-10-21");
  await expect(rows(page)).toHaveCount(1);
  const q = listRequests(hub).at(-1)!.query;
  expect(q.get("actor")).toBe(`staff:${COUNTER_ID}`);
  expect(q.get("module")).toBe("settings");
  // Kolkata midnight of the 21st, up to (not including) midnight of the 22nd.
  expect(q.get("from")).toBe(String(Date.UTC(2026, 9, 20, 18, 30)));
  expect(q.get("to")).toBe(String(Date.UTC(2026, 9, 21, 18, 30)));
  // Nothing matches: the empty-state line.
  await page.getByLabel(en["hub.audit.filter.module"], { exact: true }).selectOption({
    label: en["hub.audit.module.console"],
  });
  await expect(page.getByTestId("audit-empty")).toHaveText(en["hub.audit.empty"]);
  await expect(rows(page)).toHaveCount(0);
  await page.getByRole("button", { name: en["hub.audit.filter.clear"] }).click();
  await expect(rows(page)).toHaveCount(50);
  const cleared = listRequests(hub).at(-1)!.query;
  expect([...cleared.keys()]).toEqual(["limit"]);
  await expect(page.getByLabel(en["hub.audit.filter.actor"], { exact: true })).toHaveValue("");
  await expect(page.getByLabel(en["hub.audit.filter.module"], { exact: true })).toHaveValue("");
  await expect(page.getByLabel(en["hub.audit.filter.from"], { exact: true })).toHaveValue("");
  await expect(page.getByLabel(en["hub.audit.filter.to"], { exact: true })).toHaveValue("");
  // The Devices option filters by kind only.
  await page.getByLabel(en["hub.audit.filter.actor"], { exact: true }).selectOption({
    label: en["hub.audit.filter.devices"],
  });
  await expect(page.getByTestId("audit-empty")).toBeVisible();
  expect(listRequests(hub).at(-1)!.query.get("actor")).toBe("device");
});

test("details: before and after side by side, Changed markers, the approver; Esc returns focus", async ({
  page,
}) => {
  const hub = newHub();
  await openAudit(page, hub);
  const first = rows(page).first().getByRole("button");
  await first.focus();
  await page.keyboard.press("Enter");
  const panel = page.getByTestId("audit-detail");
  await expect(panel).toBeVisible();
  await expect(panel.getByRole("heading", { name: en["hub.audit.detail.title"] })).toBeFocused();
  await expect(
    panel.getByRole("columnheader", { name: en["hub.audit.detail.before"] }),
  ).toBeVisible();
  await expect(
    panel.getByRole("columnheader", { name: en["hub.audit.detail.after"] }),
  ).toBeVisible();
  const value = panel.getByRole("row", { name: /^value/ });
  await expect(value).toContainText("INR");
  await expect(value).toContainText("USD");
  // A text marker with its own label, not colour alone.
  const markers = panel.getByTestId("audit-changed");
  await expect(markers).toHaveCount(3);
  await expect(markers.first()).toHaveText(en["hub.audit.detail.changed"]);
  await expect(value.getByTestId("audit-changed")).toHaveAttribute(
    "aria-label",
    en["hub.audit.detail.changed_label"].replace("{field}", "value"),
  );
  await expect(panel.getByRole("row", { name: /^key/ }).getByTestId("audit-changed")).toHaveCount(
    0,
  );
  await expect(panel.getByTestId("audit-approved-by")).toHaveText(
    en["hub.audit.detail.approved_by"]
      .replace("{name}", "Manager")
      .replace("{mode}", en["hub.audit.mode.hub"]),
  );
  await expectNoSeriousAxe(page);
  await page.keyboard.press("Escape");
  await expect(panel).toHaveCount(0);
  await expect(first).toBeFocused();

  // A created row says so in the before column.
  await rows(page).nth(2).click();
  await expect(panel.getByTestId("audit-created")).toHaveText(en["hub.audit.detail.created"]);
  await expect(panel).toContainText("Dev Branch");
  await expect(panel.getByTestId("audit-approved-by")).toHaveCount(0);

  // Secrets show as hidden; their change shows only as "Changed".
  await rows(page).nth(1).getByRole("button").click();
  const pin = panel.getByRole("row", { name: /^pin_hash/ });
  await expect(pin).toContainText(en["hub.audit.detail.redacted"]);
  await expect(pin.getByTestId("audit-changed")).toHaveText(en["hub.audit.detail.changed"]);
  await expect(panel).not.toContainText("[redacted]");
  await panel.getByRole("button", { name: en["hub.audit.detail.close"] }).click();
  await expect(panel).toHaveCount(0);
});

test("a Counter sees the manager PIN in place and no data until it is approved", async ({
  page,
}) => {
  const hub = newHub();
  await openAudit(page, hub, "counter-token");
  const approval = page.getByTestId("audit-approval");
  await expect(approval).toBeVisible();
  await expect(approval).toContainText(en["hub.audit.needs_approval"]);
  const pin = approval.getByLabel(en["ui.pin.label"]);
  await expect(pin).toBeFocused();
  expect(hub.requests).toEqual([]);
  await expect(rows(page)).toHaveCount(0);
  // A wrong PIN: refused inline, still nothing fetched.
  await page.keyboard.type("9999");
  await page.keyboard.press("Enter");
  await expect(approval).toContainText(en["ui.pin.rejected"]);
  expect(hub.requests).toEqual([]);
  // Cancelled: no data, a way to ask again.
  await page.keyboard.press("Escape");
  await expect(approval).toHaveCount(0);
  await expect(page.getByTestId("audit-approval-cancelled")).toContainText(
    en["hub.audit.approval_cancelled"],
  );
  await expect(page.getByTestId("audit-filters")).toHaveCount(0);
  expect(hub.requests).toEqual([]);
  await page.getByRole("button", { name: en["hub.audit.ask_approval"] }).click();
  await expect(approval.getByLabel(en["ui.pin.label"])).toBeFocused();
  await page.keyboard.type("2222");
  await page.keyboard.press("Enter");
  await expect(rows(page)).toHaveCount(50);
  expect(hub.elevations.at(-1)).toEqual({ items: [{ permission: "audit.view" }], pin: "2222" });
  // Every audit call carried the read ticket.
  expect(hub.requests.length).toBeGreaterThan(0);
  expect(hub.requests.every((r) => r.ticket === "R1")).toBe(true);
  await rows(page).first().getByRole("button").click();
  await expect(page.getByTestId("audit-detail")).toBeVisible();
  const itemRequests = hub.requests.filter((r) => r.path === "/ui/audit/item");
  expect(itemRequests.map((r) => r.ticket)).toEqual(["R1"]);
  // The PIN and the ticket are kept nowhere on the client.
  const stored = await page.evaluate(() => JSON.stringify({ ...sessionStorage, ...localStorage }));
  expect(stored).not.toContain("2222");
  expect(stored).not.toContain("R1");
  await expectNoSeriousAxe(page);
});

test("a lapsed read approval clears the screen and asks for the PIN again", async ({ page }) => {
  const hub = newHub();
  await openAudit(page, hub, "counter-token");
  await page.getByTestId("audit-approval").getByLabel(en["ui.pin.label"]).focus();
  await page.keyboard.type("2222");
  await page.keyboard.press("Enter");
  await expect(rows(page)).toHaveCount(50);
  // The Hub dropped the ticket (expired, or a lock).
  hub.tickets.clear();
  await page.getByRole("button", { name: en["hub.audit.load_more"] }).click();
  await expect(page.getByTestId("audit-approval")).toContainText(en["ui.pin.again"]);
  await expect(rows(page)).toHaveCount(0);
});

test("a short first page loads the next ones on its own until the list fills the view", async ({
  page,
}) => {
  const hub = newHub();
  hub.pageSize = 1;
  await page.setViewportSize({ width: 1280, height: 1600 });
  await openAudit(page, hub);
  // No click: the end of the list is in view after each short page, so loading continues.
  await expect.poll(() => listRequests(hub).length).toBeGreaterThan(2);
  await expect.poll(async () => rows(page).count()).toBeGreaterThan(2);
  expect(listRequests(hub)[1]?.query.get("cursor")).toBe("1");
  // It stops once the list runs past the view.
  await page.waitForTimeout(500);
  const settled = listRequests(hub).length;
  await page.waitForTimeout(500);
  expect(listRequests(hub).length).toBe(settled);
  expect(settled).toBeLessThan(120);
});

test("a failed next page keeps the rows and retries only that page", async ({ page }) => {
  const hub = newHub();
  await openAudit(page, hub);
  await expect(rows(page)).toHaveCount(50);
  hub.failNextMore = true;
  await page.getByRole("button", { name: en["hub.audit.load_more"] }).click();
  await expect(page.getByTestId("audit-more-failed")).toContainText(
    en["hub.audit.load_more_failed"],
  );
  await expect(rows(page)).toHaveCount(50);
  await page
    .getByTestId("audit-more-failed")
    .locator("..")
    .getByRole("button", {
      name: en["common.retry"],
    })
    .click();
  await expect(rows(page)).toHaveCount(100);
  expect(listRequests(hub).at(-1)?.query.get("cursor")).toBe("50");
});

test("a detail that fails to load says so and keeps the list", async ({ page }) => {
  const hub = newHub();
  await openAudit(page, hub);
  await expect(rows(page)).toHaveCount(50);
  hub.failItem = true;
  await rows(page).first().getByRole("button").click();
  await expect(page.getByTestId("audit-detail-failed")).toContainText(
    en["hub.audit.detail.load_failed"],
  );
  await expect(rows(page)).toHaveCount(50);
  await expect(page.getByTestId("audit-load-failed")).toHaveCount(0);
  hub.failItem = false;
  await page
    .getByTestId("audit-detail-failed")
    .getByRole("button", { name: en["common.retry"] })
    .click();
  await expect(page.getByTestId("audit-detail")).toBeVisible();
});

test("From after To is refused inline before any request", async ({ page }) => {
  const hub = newHub();
  await openAudit(page, hub);
  await expect(rows(page)).toHaveCount(50);
  const before = listRequests(hub).length;
  await page.getByLabel(en["hub.audit.filter.to"], { exact: true }).fill("2026-10-20");
  await page.getByLabel(en["hub.audit.filter.from"], { exact: true }).fill("2026-10-21");
  await expect(page.getByTestId("audit-filter-error")).toContainText(
    en["hub.audit.filter.range_invalid"],
  );
  await expect(page.getByLabel(en["hub.audit.filter.from"], { exact: true })).toHaveAttribute(
    "aria-invalid",
    "true",
  );
  // Only the To change was sent; the bad range never was.
  expect(listRequests(hub).length).toBe(before + 1);
  // The Admin option filters by kind.
  await page.getByLabel(en["hub.audit.filter.from"], { exact: true }).fill("");
  await page
    .getByLabel(en["hub.audit.filter.actor"], { exact: true })
    .selectOption({ label: en["hub.audit.actor.admin"] });
  await expect.poll(() => listRequests(hub).at(-1)?.query.get("actor")).toBe("admin");
});

test("another member's PIN at the lock resets the screen to their rights", async ({ page }) => {
  const hub = newHub();
  await page.clock.install();
  await openAudit(page, hub, "counter-token");
  await page.getByTestId("audit-approval").getByLabel(en["ui.pin.label"]).focus();
  await page.keyboard.type("2222");
  await page.keyboard.press("Enter");
  await expect(rows(page)).toHaveCount(50);
  await page.clock.runFor(5 * 60_000);
  const overlay = page.getByRole("dialog", { name: en["hub.lock.title"] });
  await expect(overlay).toBeVisible();
  const fetched = hub.requests.length;
  await page.keyboard.type("4444");
  await page.keyboard.press("Enter");
  await expect(overlay).toHaveCount(0);
  // Kitchen holds no audit.view: nothing of the counter's approval stays.
  await expect(rows(page)).toHaveCount(0);
  await expect(page.getByTestId("audit-approval")).toContainText(en["hub.audit.needs_approval"]);
  expect(hub.requests.length).toBe(fetched);
  expect(hub.dropped).toContain("R1");
});
