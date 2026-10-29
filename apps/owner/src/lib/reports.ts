import type {
  OwnerBranch,
  OwnerLive,
  OwnerRange,
  OwnerRangeShift,
  OwnerRangeTotals,
} from "@arena/ui/generated";
import { load } from "./api";

export type RangeKind = "today" | "week" | "month" | "custom";

/** The longest range one `summary` call covers (the cloud refuses more). */
export const MAX_DAYS = 400;
const DAY_MS = 86_400_000;

const toDays = (date: string) => Date.parse(date + "T00:00:00Z") / DAY_MS;
const fromDays = (days: number) => new Date(days * DAY_MS).toISOString().slice(0, 10);

/** The calendar date (`yyyy-mm-dd`) it is in `tz` at `now`. */
export function dateIn(tz: string, now = Date.now()): string {
  try {
    return new Intl.DateTimeFormat("en-CA", {
      timeZone: tz,
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
    }).format(now);
  } catch {
    return new Date(now).toISOString().slice(0, 10);
  }
}

/** `from`..`to` of a range kind, ending today; weeks start on Monday. */
export function rangeOf(kind: RangeKind, today: string, custom: { from: string; to: string }) {
  if (kind === "custom") return custom;
  if (kind === "today") return { from: today, to: today };
  if (kind === "month") return { from: today.slice(0, 8) + "01", to: today };
  // 1970-01-01 was a Thursday, so Monday is day 4 mod 7.
  const days = toDays(today);
  return { from: fromDays(days - ((days + 3) % 7)), to: today };
}

/** What is wrong with a range, if anything. */
export function rangeProblem(from: string, to: string): "order" | "long" | null {
  const a = toDays(from);
  const b = toDays(to);
  if (Number.isNaN(a) || Number.isNaN(b) || a > b) return "order";
  return b - a + 1 > MAX_DAYS ? "long" : null;
}

/** One branch's answers for a range. */
export type BranchReport = {
  branch: OwnerBranch;
  range: OwnerRange | null;
  live: OwnerLive | null;
  /** The oldest of the answers' times (a cached copy keeps the time it was saved). */
  asOf: string;
  /** Every answer came from the cloud just now. */
  fresh: boolean;
};

const base = (id: string) => `/branches/${encodeURIComponent(id)}`;

/** Fetches the range and the live figures of each branch; `"signed-out"` when a session ended. */
export async function loadReport(
  branches: OwnerBranch[],
  from: string,
  to: string,
): Promise<BranchReport[] | "signed-out"> {
  const query = `?from=${from}&to=${to}&group_by=day`;
  const parts = await Promise.all(
    branches.map(async (branch) => {
      const [range, live] = await Promise.all([
        load<OwnerRange>(`${base(branch.id)}/summary${query}`), // i18n-ignore
        load<OwnerLive>(`${base(branch.id)}/live`), // i18n-ignore
      ]);
      return { branch, range, live };
    }),
  );
  if (parts.some((p) => p.range === "signed-out" || p.live === "signed-out")) return "signed-out";
  return parts.map(({ branch, range, live }) => {
    const r = range === "signed-out" ? null : range;
    const l = live === "signed-out" ? null : live;
    const times = [r?.asOf, l?.asOf].filter((x): x is string => !!x).sort();
    return {
      branch,
      range: r?.data ?? null,
      live: l?.data ?? null,
      asOf: times[0] ?? "",
      fresh: !!r?.live && (l?.live ?? true),
    };
  });
}

/** Adds the numbers of `b` into a copy of `a`; both have the same shape. */
function plus<T>(a: T, b: T): T {
  if (typeof a === "number") return ((a as number) + (b as number)) as T;
  if (a && typeof a === "object") {
    return Object.fromEntries(
      Object.entries(a).map(([k, v]) => [k, plus(v, (b as Record<string, unknown>)[k])]),
    ) as T;
  }
  return a;
}

export const emptyTotals = (): OwnerRangeTotals => ({
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
});

export const sumTotals = (list: OwnerRangeTotals[]) => list.reduce(plus, emptyTotals());

/** A branch's live figure for the business day it is on, when that day is in the range. */
export type SoFar = { branch: string; day: string; minor: number; asOf: number };

export type DayRow = { day: string; minor: number; live: boolean; asOf?: number };

/** Days without an accepted rollup, per branch. */
export type Missing = { branch: string; dates: string[] };

/** The report of the branches that share one currency. */
export type CurrencyView = {
  currency: string;
  branches: { id: string; name: string; minor: number }[];
  totals: OwnerRangeTotals;
  days: DayRow[];
  soFar: SoFar[];
  missing: Missing[];
  /** Some day of the range has no figures yet. */
  partial: boolean;
};

/**
 * Groups branch reports by currency, so currencies are never mixed. A branch's current business
 * day comes from its live summary and is not a missing day: it is the "So far today" row, unless
 * its rollup already exists (the day is closed), when the rollup is the figure.
 */
export function byCurrency(reports: BranchReport[], from: string, to: string): CurrencyView[] {
  const views = new Map<string, CurrencyView>();
  for (const { branch, range, live } of reports) {
    if (!range) continue;
    const view = views.get(range.currency) ?? {
      currency: range.currency,
      branches: [],
      totals: emptyTotals(),
      days: [],
      soFar: [],
      missing: [],
      partial: false,
    };
    views.set(range.currency, view);
    view.totals = plus(view.totals, range.totals);
    view.branches.push({ id: branch.id, name: branch.name, minor: range.totals.revenue.total });

    const summary = live?.summary ?? null;
    const today =
      summary && summary.business_day >= from && summary.business_day <= to
        ? summary.business_day
        : null;
    const covered = !!today && range.missing_days.includes(today);
    if (summary && today && covered) {
      view.soFar.push({
        branch: branch.name,
        day: today,
        minor: summary.revenue_so_far_minor,
        asOf: summary.as_of,
      });
    }
    const missing = range.missing_days.filter((d) => !(covered && d === today));
    if (missing.length) {
      view.missing.push({ branch: branch.name, dates: missing });
      view.partial = true;
    }
    for (const row of range.rows) {
      if (row.totals.days === 0) continue;
      const have = view.days.find((d) => d.day === row.period && !d.live);
      if (have) have.minor += row.totals.revenue.total;
      else view.days.push({ day: row.period, minor: row.totals.revenue.total, live: false });
    }
  }
  const out = [...views.values()];
  for (const v of out) {
    for (const s of v.soFar) v.days.push({ day: s.day, minor: s.minor, live: true, asOf: s.asOf });
    v.days.sort((a, b) => a.day.localeCompare(b.day) || Number(a.live) - Number(b.live));
  }
  return out;
}

/** A shift of the cash reconciliation list with the branch it belongs to. */
export type ShiftRow = OwnerRangeShift & { branch: OwnerBranch; currency: string };

/** Every shift closed in the range, oldest closing first. */
export const shiftsOf = (reports: BranchReport[]): ShiftRow[] =>
  reports
    .flatMap(({ branch, range }) =>
      (range?.shifts ?? []).map((s) => ({ ...s, branch, currency: range?.currency ?? "INR" })),
    )
    .sort((a, b) => a.shift.closed_at - b.shift.closed_at);
