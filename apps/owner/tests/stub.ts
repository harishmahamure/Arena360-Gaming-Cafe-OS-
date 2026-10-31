import type { Page } from "@playwright/test";
import type {
  BranchPlan,
  BranchSummary,
  InvoiceRow,
  OwnerBackup,
  OwnerBranch,
  OwnerCommandPreview,
  OwnerCommandView,
  OwnerAlertSettings,
  OwnerAlertView,
  OwnerLive,
  OwnerRange,
  SummaryCatalogue,
} from "@arena/ui/generated";
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

/** A catalogue with only the lists a test names. */
export const catalogueOf = (over: Partial<SummaryCatalogue> = {}): SummaryCatalogue => ({
  ...summaryOf("x").catalogue,
  ...over,
});

/** The `live` answer of a branch whose summary carries `catalogue`, heard from `ageMs` ago. */
export const liveWith = (id: string, catalogue: Partial<SummaryCatalogue>, ageMs = 120_000) =>
  liveOf(id, {}, { catalogue: catalogueOf(catalogue), as_of: Date.now() - ageMs });

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
    stock: { levels_as_of: null, levels: [], waste: [], expiring: [] },
    credit: { as_of: null, outstanding: 0, aged: { d0_7: 0, d8_14: 0, d15p: 0 }, players: [] },
    ...over,
  };
}

export type StubOptions = {
  /** Answers `POST /v1/wizard/install-hub/standby-code` (Story 9.18); unset: a code. Each body is recorded in `calls.bodies["standby"]`. */
  standby?: (body: { branch_id?: string }) => { status: number; body: unknown } | undefined;
  /** The `summary` answer per branch id; a function gets the request's query string. */
  range?: Record<string, OwnerRange | ((query: URLSearchParams) => OwnerRange)>;
  /** The `live` answer per branch id (a function is called on every request); default: online. */
  live?: Record<string, OwnerLive | (() => OwnerLive)>;
  /** The status the export route answers with (default 200, with a small CSV). */
  exportStatus?: number;
  /** The commands the cloud holds at the start (the stub adds what the app posts). */
  commands?: OwnerCommandView[];
  /** Branch ids whose Hub is offline in a preview (default: all online). */
  offline?: string[];
  /** The first N command posts are recorded but their response is lost (the connection drops). */
  lose?: number;
  /** The alerts the cloud holds, newest first (the stub reads the array on every call). */
  alerts?: OwnerAlertView[];
  /** The notification settings the cloud starts with. */
  alertSettings?: Partial<OwnerAlertSettings>;
  /** Answers the audit routes (`GET` search, `POST` build); unset: 404. Each call is recorded in `calls.bodies["audit"]`. */
  audit?: (call: { method: string; branch: string; query: URLSearchParams; body: unknown }) => {
    status: number;
    body: unknown;
  };
  /** Answers `GET /v1/live/{branch}` (Story 8.40); unset: 404. Each call is recorded in `calls.bodies["live"]`. */
  liveView?: (branch: string) => { status: number; body: unknown };
  /** The Account -> Subscription cards; default: one per branch from its tier (`planOf`). */
  plans?: BranchPlan[] | (() => BranchPlan[]);
  /** The invoices the cloud holds, newest first. Each `GET` is recorded in `calls.bodies["invoices"]`. */
  invoices?: InvoiceRow[];
  /** Answers the plan change routes (Story 10.9). Each body is recorded in `calls.bodies["change"]`; unset: 404. */
  change?: (
    step: "preview" | "confirm" | "withdraw",
    body: Record<string, unknown>,
  ) => { status: number; body: unknown };
  /** Answers the billing cycle routes (Story 10.10). Each body is recorded in `calls.bodies["cycle"]`; unset: 404. */
  cycle?: (
    step: "preview" | "confirm",
    body: Record<string, unknown>,
  ) => { status: number; body: unknown };
  /** Answers `POST .../subscription/payment-method` (Story 10.10); recorded in `calls.bodies["method"]`; unset: 404. */
  method?: (body: Record<string, unknown>) => { status: number; body: unknown };
  /** Answers `GET /v1/account/invoices/{id}/pdf`; unset: a signed link. */
  pdf?: (id: string) => { status: number; body: unknown };
  /** Answers a command post instead of the default (queue one row per branch that can take it). */
  post?: (body: Record<string, unknown>) => { status: number; body: unknown } | undefined;
};

/** A command row as the cloud lists it, `ageMs` old. */
export function commandOf(over: Partial<OwnerCommandView> = {}, ageMs = 120_000): OwnerCommandView {
  return {
    id: 1,
    branch_id: "b1",
    kind: "rate.upsert",
    status: "pending",
    payload: { group_id: "g1", price_per_hour_minor: 6000 },
    queued_at: Date.now() - ageMs,
    ...over,
  };
}

/** The backup block a paid Hub last sent (Story 9.17): uploaded 4 min ago, snapshot 3 h ago, 2 pending. */
export function backupOf(over: Partial<OwnerBackup> = {}): OwnerBackup {
  return {
    last_upload_at: Date.now() - 4 * 60_000,
    last_snapshot_at: Date.now() - 3 * 3_600_000,
    pending_batches: 2,
    ...over,
  };
}

/** An alert as the cloud lists it, `ageMs` old. */
export function alertOf(over: Partial<OwnerAlertView> = {}, ageMs = 120_000): OwnerAlertView {
  return {
    id: "al1",
    branch_id: "b1",
    branch_name: "Pixel Arena MG Road",
    kind: "low_stock",
    text: "Cola is running low: 3 left.",
    url: "/reports?tab=stock&branch=b1",
    created_at: Date.now() - ageMs,
    ...over,
  };
}

/** The Account -> Subscription card of a branch: Free when its tier is, else Basic monthly with GST. */
export function planOf(b: Branch, over: Partial<BranchPlan> = {}): BranchPlan {
  if (b.tier === "free")
    return {
      branch_id: b.id,
      branch_name: b.name,
      state: "free",
      tier: "free",
      add_ons: { devices_20: 0, tunnel: 0, owner_login: 0 },
      gst_percent: 0,
      prices: [],
      rev: 7,
      ...over,
    };
  return {
    branch_id: b.id,
    branch_name: b.name,
    state: "active",
    tier: b.tier,
    cycle: "monthly",
    band: "IN",
    currency: "INR",
    add_ons: { devices_20: 2, tunnel: 1, owner_login: 0 },
    current_period_end: Date.UTC(2026, 10, 30, 12),
    gst_percent: 18,
    prices: [
      { cycle: "monthly", amount_minor: 149_900, tax_minor: 26_982, total_minor: 176_882 },
      { cycle: "yearly", amount_minor: 1_499_000, tax_minor: 269_820, total_minor: 1_768_820 },
    ],
    payment_method: { method: "card", detail: "4242" },
    rev: 7,
    ...over,
  };
}

/** An invoice list line, issued on 30 Oct 2026. */
export function invoiceRowOf(over: Partial<InvoiceRow> = {}): InvoiceRow {
  return {
    id: "inv1",
    number: "A360/2026-27/000001",
    kind: "invoice",
    fy: "2026-27",
    branch_id: "b1",
    branch_name: "Pixel Arena MG Road",
    issued_at: Date.UTC(2026, 9, 30, 8),
    currency: "INR",
    total_minor: 176_882,
    pdf_ready: true,
    ...over,
  };
}

/** The CSV the stubbed export route answers with. */
export const EXPORT_CSV = "Report,Revenue\r\nRange,2026-10-26 to 2026-10-30\r\n";

export async function stubCloud(
  page: Page,
  branches: Branch[] | (() => Branch[]) = PAID,
  opts: StubOptions = {},
) {
  const list = () => (typeof branches === "function" ? branches() : branches);
  const calls: string[] = [];
  const bodies: Record<string, unknown[]> = {};
  const exports: string[] = [];
  const commands: OwnerCommandView[] = [...(opts.commands ?? [])];
  let nextId = Math.max(0, ...commands.map((c) => c.id));
  let lost = opts.lose ?? 0;
  const alerts = opts.alerts ?? [];
  const settings: OwnerAlertSettings = {
    vapid_public_key:
      "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4",
    whatsapp: true,
    subscriptions: 0,
    ...opts.alertSettings,
  };
  const byKey = new Map<string, OwnerCommandView[]>();
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
    if (path === "/v1/account/subscription" && req.method() === "GET") {
      const given = typeof opts.plans === "function" ? opts.plans() : opts.plans;
      const all = given ?? list().map((b) => planOf(b));
      const only = new URL(req.url()).searchParams.get("branch");
      return json({ branches: all.filter((p) => !only || p.branch_id === only) });
    }
    if (path === "/v1/account/invoices" && req.method() === "GET") {
      const q = new URL(req.url()).searchParams;
      (bodies["invoices"] ??= []).push(q.toString());
      const all = opts.invoices ?? [];
      const rows = all.filter(
        (i) =>
          (!q.get("branch") || i.branch_id === q.get("branch")) &&
          (!q.get("fy") || i.fy === q.get("fy")),
      );
      return json({ invoices: rows, years: [...new Set(all.map((i) => i.fy))].sort().reverse() });
    }
    const change = path.match(
      /^\/v1\/branches\/([^/]+)\/subscription\/change(\/preview|\/withdraw)?$/,
    );
    if (change && req.method() === "POST") {
      const step =
        change[2] === "/preview" ? "preview" : change[2] === "/withdraw" ? "withdraw" : "confirm";
      const body = (req.postDataJSON() ?? {}) as Record<string, unknown>;
      (bodies["change"] ??= []).push({ step, branch: change[1], ...body });
      const answer = opts.change?.(step, body) ?? { status: 404, body: { code: "NOT_FOUND" } };
      return answer.status === 204
        ? route.fulfill({ status: 204 })
        : json(answer.body, answer.status);
    }
    const cycle = path.match(/^\/v1\/branches\/([^/]+)\/subscription\/cycle(\/preview)?$/);
    if (cycle && req.method() === "POST") {
      const step = cycle[2] ? "preview" : "confirm";
      const body = (req.postDataJSON() ?? {}) as Record<string, unknown>;
      (bodies["cycle"] ??= []).push({ step, branch: cycle[1], ...body });
      const answer = opts.cycle?.(step, body) ?? { status: 404, body: { code: "NOT_FOUND" } };
      return json(answer.body, answer.status);
    }
    const method = path.match(/^\/v1\/branches\/([^/]+)\/subscription\/payment-method$/);
    if (method && req.method() === "POST") {
      const body = (req.postDataJSON() ?? {}) as Record<string, unknown>;
      (bodies["method"] ??= []).push({ branch: method[1], ...body });
      const answer = opts.method?.(body) ?? { status: 404, body: { code: "NOT_FOUND" } };
      return json(answer.body, answer.status);
    }
    const pdf = path.match(/^\/v1\/account\/invoices\/([^/]+)\/pdf$/);
    if (pdf && req.method() === "GET") {
      (bodies["pdf"] ??= []).push(pdf[1]);
      const answer = opts.pdf?.(pdf[1]) ?? {
        status: 200,
        body: { url: "https://files.example.test/billing/invoices/x.pdf?sig=1", expires_in_s: 600 },
      };
      return json(answer.body, answer.status);
    }
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
    const audit = path.match(/^\/v1\/branches\/([^/]+)\/audit(\/build)?$/);
    if (audit && opts.audit) {
      const call = {
        method: req.method(),
        branch: audit[1],
        query: new URL(req.url()).searchParams,
        body: req.postDataJSON() ?? null,
      };
      (bodies["audit"] ??= []).push({ ...call, build: !!audit[2], query: call.query.toString() });
      const answer = opts.audit(call);
      return json(answer.body, answer.status);
    }
    const lv = path.match(/^\/v1\/live\/([^/]+)$/);
    if (lv && req.method() === "GET") {
      (bodies["live"] ??= []).push(lv[1]);
      const answer = opts.liveView?.(lv[1]) ?? {
        status: 404,
        body: { code: "NOT_FOUND", message: "" },
      };
      return json(answer.body, answer.status);
    }
    const exp = path.match(/^\/v1\/branches\/([^/]+)\/export$/);
    if (exp && req.method() === "GET") {
      exports.push(path + new URL(req.url()).search);
      if ((opts.exportStatus ?? 200) !== 200)
        return json({ code: "UNAVAILABLE", message: "" }, opts.exportStatus);
      return route.fulfill({
        status: 200,
        contentType: "text/csv; charset=utf-8",
        body: EXPORT_CSV,
      });
    }
    const age = path.match(/^\/v1\/branches\/([^/]+)\/data-age$/);
    if (age && req.method() === "POST") {
      (bodies[age[1]] ??= []).push(req.postDataJSON());
      return route.fulfill({ status: 204 });
    }
    if (path === "/v1/commands/preview" && req.method() === "POST") {
      const body = req.postDataJSON();
      const picked =
        body.branch_id === "all" ? list() : list().filter((b) => b.id === body.branch_id);
      const preview: OwnerCommandPreview = {
        targets: picked.map((b) => ({
          branch_id: b.id,
          name: b.name,
          online: !opts.offline?.includes(b.id),
          offered: b.tier !== "free",
          ...(b.tier === "free" ? { error: { code: "FEATURE_NOT_ENTITLED", message: "" } } : {}),
        })),
      };
      return json(preview);
    }
    if (path === "/v1/commands" && req.method() === "GET") {
      const branch = new URL(req.url()).searchParams.get("branch");
      const rows = commands.filter((c) => !branch || c.branch_id === branch);
      return json({ commands: rows.sort((x, y) => y.id - x.id) });
    }
    if (path === "/v1/commands" && req.method() === "POST") {
      const body = req.postDataJSON();
      (bodies["POST /v1/commands"] ??= []).push(body);
      const answer = opts.post?.(body);
      if (answer) return json(answer.body, answer.status);
      if (byKey.has(body.idempotency_key)) {
        // The same key returns the rows it already made: the cloud is idempotent.
        return json({ commands: byKey.get(body.idempotency_key), skipped: [] });
      }
      const all = body.branch_id === "all";
      const made = list()
        .filter((b) => b.tier !== "free" && (all || b.id === body.branch_id))
        .map((b): OwnerCommandView => ({
          id: ++nextId,
          branch_id: b.id,
          ...(all ? { group_id: body.idempotency_key } : {}),
          kind: body.kind,
          status: "pending",
          payload: {
            ...body.payload,
            ...(body.base_rev === undefined ? {} : { base_rev: body.base_rev }),
          },
          queued_at: Date.now(),
        }));
      commands.push(...made);
      byKey.set(body.idempotency_key, made);
      if (lost > 0) {
        lost--;
        return route.abort("connectionreset");
      }
      return json({ commands: made, skipped: [] });
    }
    if (path === "/v1/alerts" && req.method() === "GET") {
      const q = new URL(req.url()).searchParams;
      const from = Number(q.get("before") ?? 0);
      const limit = Number(q.get("limit") ?? 30);
      const page = alerts.slice(from, from + limit);
      const more = from + limit < alerts.length;
      return json({ alerts: page, ...(more ? { next_before: String(from + limit) } : {}) });
    }
    if (path === "/v1/alerts/settings" && req.method() === "GET") return json(settings);
    if (path === "/v1/alerts/settings" && req.method() === "PATCH") {
      const body = req.postDataJSON();
      (bodies["PATCH /v1/alerts/settings"] ??= []).push(body);
      settings.whatsapp = body.whatsapp;
      return json(settings);
    }
    if (path === "/v1/alerts/push" && req.method() === "POST") {
      (bodies["POST /v1/alerts/push"] ??= []).push(req.postDataJSON());
      settings.subscriptions++;
      return route.fulfill({ status: 204 });
    }
    if (path === "/v1/alerts/push/remove" && req.method() === "POST") {
      (bodies["POST /v1/alerts/push/remove"] ??= []).push(req.postDataJSON());
      settings.subscriptions = Math.max(0, settings.subscriptions - 1);
      return route.fulfill({ status: 204 });
    }
    if (path === "/v1/meta/timezones") return json(["Asia/Dubai", "Asia/Kolkata"]);
    if (path === "/v1/wizard/install-hub/code")
      return json({ code: "ABCD2345", expires_at: Date.now() + 900_000, server_now: Date.now() });
    if (path === "/v1/wizard/install-hub/standby-code") {
      const body = req.postDataJSON();
      (bodies["standby"] ??= []).push(body);
      const answer = opts.standby?.(body);
      if (answer) return json(answer.body, answer.status);
      return json({ code: "WXYZ6789", expires_at: Date.now() + 900_000, server_now: Date.now() });
    }
    if (path === "/v1/signout") return json(null, 200);
    return json({ code: "NOT_FOUND", message: "" }, 404);
  });
  return Object.assign(calls, { bodies, exports, commands, alerts, settings });
}

export const cacheCount = (page: Page) =>
  page.evaluate(
    () =>
      new Promise<number>((ok) => {
        const req = indexedDB.open("arena-owner");
        req.onupgradeneeded = () => req.transaction!.abort();
        req.onerror = () => ok(-1);
        req.onsuccess = () => {
          const c = req.result.transaction("payloads").objectStore("payloads").count();
          c.onsuccess = () => ok(c.result);
        };
      }),
  );
