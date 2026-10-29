import type { Page } from "@playwright/test";
import type { BranchSummary, OwnerBranch, OwnerLive } from "@arena/ui/generated";
import { t } from "../../../packages/ui/src/messages/index.js";

export { t };

export type Branch = OwnerBranch;

const base = {
  timezone: "Asia/Kolkata",
  live: false,
  hub: { paired: false },
  waiting: false,
} as const;

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

export type StubOptions = {
  /** The `live` answer per branch id (a function is called on every request); default: online. */
  live?: Record<string, OwnerLive | (() => OwnerLive)>;
};

export async function stubCloud(page: Page, branches: Branch[] = PAID, opts: StubOptions = {}) {
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
    if (path === "/v1/branches" && req.method() === "GET") return json(branches);
    if (path === "/v1/branches" && req.method() === "POST")
      return json({ ...branches[0], id: "b9", name: req.postDataJSON().name });
    if (path.startsWith("/v1/branches/") && req.method() === "PATCH")
      return json({ ...branches[0], ...req.postDataJSON() });
    const live = path.match(/^\/v1\/branches\/([^/]+)\/live$/);
    if (live && req.method() === "GET") {
      const given = opts.live?.[live[1]];
      return json(typeof given === "function" ? given() : (given ?? liveOf(live[1])));
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
