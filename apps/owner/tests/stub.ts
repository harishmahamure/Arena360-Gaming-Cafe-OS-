import type { Page } from "@playwright/test";
import type { BranchSummary, OwnerBranch, OwnerLive, OwnerRange } from "@arena/ui/generated";
import { t } from "../../../packages/ui/src/messages/index.js";

export { t };

export type Branch = OwnerBranch;

const base = {
  timezone: "Asia/Kolkata",
  live: true,
  live_at: 1_700_000_000_000,
  setup: { steps_done: 10, steps_total: 10, next_step: "go_live" },
  hub: { paired: false },
  waiting: false,
} as const;

/** The branch as it is before Go live: `live_at` null, `done` steps finished. */
export const inSetup = (
  b: Branch,
  done = 4,
  next: Branch["setup"]["next_step"] = "devices",
): Branch => ({
  ...b,
  live: false,
  live_at: null,
  setup: { steps_done: done, steps_total: 10, next_step: next },
});

export const PAID: Branch[] = [
  {
    ...base,
    id: "b1",
    name: "Pixel Arena MG Road",
    branch_code: "MGR",
    tier: "basic",
    flags: ["tunnel"],
  },
  {
    ...base,
    id: "b2",
    name: "Pixel Arena Indiranagar",
    branch_code: "IND",
    tier: "free",
    flags: [],
  },
];
export const FREE: Branch[] = [
  { ...base, id: "b1", name: "Pixel Arena MG Road", branch_code: "MGR", tier: "free", flags: [] },
];

/** A branch summary as the Hub publishes it, `ageMs` old, with overrides. */
export function summaryOf(
  id: string,
  over: Partial<BranchSummary> = {},
  ageMs = 120_000,
): BranchSummary {
  return {
    schema: 1,
    branch_id: id,
    as_of: Date.now() - ageMs,
    business_day: "2026-10-30",
    open: true,
    sessions_live: 7,
    revenue_so_far_minor: 12_500_050,
    currency: "INR",
    cash_in_drawer_minor: 3_400_000,
    shifts: [
      { shift_id: "sh2", staff_name: "Ravi", opened_at: 1, closed_at: 3, variance_minor: -2500 },
    ],
    staff_on_shift: ["Meera", "Dev"],
    attention: [],
    catalogue: {
      groups: [],
      rates: [],
      plans: [],
      pricing_rules: [],
      coupons: [],
      products: [],
      bundles: [],
      staff: [],
      roles: [],
      devices: [],
    },
    ...over,
  };
}

/** The `live` answer of a branch that is online and was heard from a minute ago. */
export function liveOf(
  id: string,
  over: Partial<OwnerLive> = {},
  summary?: Partial<BranchSummary>,
): OwnerLive {
  const s = summaryOf(id, summary);
  return {
    branch_id: id,
    currency: "INR",
    as_of: s.as_of,
    branch_online: true,
    last_heartbeat_at: Date.now() - 60_000,
    summary: s,
    ...over,
  };
}

/** The `summary` answer for a branch: one rollup-less range unless `over` says otherwise. */
export function rangeOfBranch(id: string, over: Partial<OwnerRange> = {}): OwnerRange {
  return {
    branch_id: id,
    currency: "INR",
    from: "2026-10-26",
    to: "2026-10-30",
    group_by: "day",
    totals: {
      days: 0,
      revenue: {
        total: 0,
        by_type: { play: 0, products: 0, plans: 0, other: 0 },
        by_method: { cash: 0, upi: 0, card: 0, wallet: 0, credit: 0 },
      },
      refunds: { count: 0, amount: 0 },
      discounts: { count: 0, amount: 0 },
      voids: { count: 0, amount: 0 },
      sessions: { count: 0, minutes: 0 },
    },
    rows: [],
    missing_days: [],
    shifts: [],
    ...over,
  };
}

export type StubOptions = {
  /** The `summary` answer per branch id; a function gets the request's query string. */
  range?: Record<string, OwnerRange | ((query: URLSearchParams) => OwnerRange)>;
  /** The `live` answer per branch id (a function is called on every request); default: online. */
  live?: Record<string, OwnerLive | (() => OwnerLive)>;
};

export async function stubCloud(
  page: Page,
  branches: Branch[] | (() => Branch[]) = PAID,
  opts: StubOptions = {},
) {
  const list = () => (typeof branches === "function" ? branches() : branches);
  const calls: string[] = [];
  const bodies: Record<string, unknown[]> = {};
  await page.route("**/v1/**", (route) => {
    const req = route.request();
    const path = new URL(req.url()).pathname;
    calls.push(`${req.method()} ${path}`);
    const json = (body: unknown, status = 200) =>
      route.fulfill({ status, contentType: "application/json", body: JSON.stringify(body) });
    if (path === "/v1/session")
      return json({
        owner: {
          name: "Asha",
          cafe_name: "Pixel Arena Gaming",
          city: "Bengaluru",
          mfa_state: "enrolled",
          has_password: true,
          backup_codes_acked: true,
          sms_fallback: false,
        },
      });
    if (path === "/v1/branches" && req.method() === "GET") return json(list());
    if (path === "/v1/branches" && req.method() === "POST")
      return json({ ...list()[0], id: "b9", name: req.postDataJSON().name });
    if (path.startsWith("/v1/branches/") && req.method() === "PATCH")
      return json({ ...list()[0], ...req.postDataJSON() });
    const live = path.match(/^\/v1\/branches\/([^/]+)\/live$/);
    if (live && req.method() === "GET") {
      const given = opts.live?.[live[1]];
      return json(typeof given === "function" ? given() : (given ?? liveOf(live[1])));
    }
    const range = path.match(/^\/v1\/branches\/([^/]+)\/summary$/);
    if (range && req.method() === "GET") {
      const given = opts.range?.[range[1]];
      const query = new URL(req.url()).searchParams;
      return json(typeof given === "function" ? given(query) : (given ?? rangeOfBranch(range[1])));
    }
    const age = path.match(/^\/v1\/branches\/([^/]+)\/data-age$/);
    if (age && req.method() === "POST") {
      (bodies[age[1]] ??= []).push(req.postDataJSON());
      return route.fulfill({ status: 204 });
    }
    if (path === "/v1/meta/timezones") return json(["Asia/Dubai", "Asia/Kolkata"]);
    if (path === "/v1/wizard/install-hub/code")
      return json({ code: "ABCD2345", expires_at: Date.now() + 900_000, server_now: Date.now() });
    if (path === "/v1/signout") return json(null, 200);
    return json({ code: "NOT_FOUND", message: "" }, 404);
  });
  return Object.assign(calls, { bodies });
}

export const cacheCount = (page: Page) =>
  page.evaluate(
    () =>
      new Promise<number>((ok) => {
        const req = indexedDB.open("arena-owner", 1);
        req.onupgradeneeded = () => req.transaction!.abort();
        req.onerror = () => ok(-1);
        req.onsuccess = () => {
          const c = req.result.transaction("payloads").objectStore("payloads").count();
          c.onsuccess = () => ok(c.result);
        };
      }),
  );
