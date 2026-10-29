import type {
  OwnerBranch,
  OwnerLive,
  OwnerRange,
  OwnerRangeShift,
  OwnerRangeStock,
  OwnerRangeTotals,
} from "@arena/ui/generated";
import { download, load } from "./api";

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

/** The days of rollups that stock levels and expiring lots are read from (the newest one wins). */
export const LEVELS_DAYS = 7;
/** Lots expiring within this many days of today are listed. */
export const EXPIRY_DAYS = 7;

/** One branch's stock as of its newest rollup in the last week. */
export type LevelsWindow = {
  branchId: string;
  stock: OwnerRangeStock | null;
  asOf: string;
  fresh: boolean;
};

/** Fetches the last week's stock levels and expiring lots of each branch. */
export async function loadLevels(
  branches: OwnerBranch[],
  today: string,
): Promise<LevelsWindow[] | "signed-out"> {
  const from = fromDays(toDays(today) - (LEVELS_DAYS - 1));
  const parts = await Promise.all(
    branches.map(async (b) => ({
      id: b.id,
      res: await load<OwnerRange>(`${base(b.id)}/summary?from=${from}&to=${today}&group_by=day`), // i18n-ignore
    })),
  );
  if (parts.some((p) => p.res === "signed-out")) return "signed-out";
  return parts.map(({ id, res }) => {
    const r = res === "signed-out" ? null : res;
    return { branchId: id, stock: r?.data.stock ?? null, asOf: r?.asOf ?? "", fresh: !!r?.live };
  });
}

/** Products match across branches by name and category, ignoring case and spacing. */
const productKey = (name: string, category?: string | null) =>
  `${name.trim().toLowerCase()}\u0000${(category ?? "").trim().toLowerCase()}`; // i18n-ignore

export type StockCell = { qty: number; low: number | null };
export type StockProduct = {
  key: string;
  name: string;
  category: string;
  /** By branch id; a branch without the product has no cell. */
  cells: Record<string, StockCell>;
  total: number;
};
export type WasteCell = { qty: number; value: number };
export type WasteLine = {
  key: string;
  name: string;
  category: string;
  reason: string;
  cells: Record<string, WasteCell>;
  qty: number;
  value: number;
};
export type LowLine = { branch: string; name: string; qty: number; low: number };
export type ExpiringLine = {
  branch: string;
  name: string;
  lot: string;
  qty: number;
  expiresOn: string;
  expired: boolean;
};
/** A low-stock attention item from a branch's live summary. */
export type LiveLow = { branch: string; product: string; qty: string; asOf: number };

export type StockBranch = {
  id: string;
  name: string;
  /** The business day of the rollup the levels come from; null when the week has none. */
  levelsAsOf: string | null;
  tracked: boolean;
};

/** The stock report of the branches that share one currency. */
export type StockView = {
  currency: string;
  branches: StockBranch[];
  products: StockProduct[];
  low: LowLine[];
  waste: WasteLine[];
  expiring: ExpiringLine[];
  live: LiveLow[];
};

const byName = <T extends { name: string; category?: string }>(a: T, b: T) =>
  a.name.localeCompare(b.name) || (a.category ?? "").localeCompare(b.category ?? "");

/**
 * Groups the branches' stock by currency, so waste value is never mixed. Levels and expiring lots
 * come from each branch's newest rollup of the last week, waste from the picked range, and the
 * live low-stock items from the branch's current summary.
 */
export function stockViews(
  reports: BranchReport[],
  windows: LevelsWindow[],
  today: string,
): StockView[] {
  const views = new Map<string, StockView>();
  const horizon = fromDays(toDays(today) + EXPIRY_DAYS);
  for (const { branch, range, live } of reports) {
    if (!range) continue;
    const view = views.get(range.currency) ?? {
      currency: range.currency,
      branches: [],
      products: [],
      low: [],
      waste: [],
      expiring: [],
      live: [],
    };
    views.set(range.currency, view);

    const stock = windows.find((w) => w.branchId === branch.id)?.stock ?? null;
    const levels = stock?.levels ?? [];
    view.branches.push({
      id: branch.id,
      name: branch.name,
      levelsAsOf: stock?.levels_as_of ?? null,
      tracked: levels.length > 0,
    });
    for (const l of levels) {
      const key = productKey(l.name, l.category);
      let p = view.products.find((x) => x.key === key);
      if (!p) {
        p = { key, name: l.name, category: l.category ?? "", cells: {}, total: 0 };
        view.products.push(p);
      }
      // Two products of one branch sharing a name and category add up into one cell.
      const cell = p.cells[branch.id];
      p.cells[branch.id] = {
        qty: (cell?.qty ?? 0) + l.qty,
        low: cell?.low ?? l.low_qty ?? null,
      };
      p.total += l.qty;
      if (l.low_qty != null && l.qty <= l.low_qty) {
        view.low.push({ branch: branch.name, name: l.name, qty: l.qty, low: l.low_qty });
      }
    }
    for (const e of stock?.expiring ?? []) {
      if (e.expires_on > horizon) continue;
      view.expiring.push({
        branch: branch.name,
        name: e.name,
        lot: e.lot,
        qty: e.qty,
        expiresOn: e.expires_on,
        expired: e.expires_on < today,
      });
    }
    for (const w of range.stock.waste) {
      const key = productKey(w.name, w.category) + "\u0000" + w.reason; // i18n-ignore
      let line = view.waste.find((x) => x.key === key);
      if (!line) {
        line = {
          key,
          name: w.name,
          category: w.category ?? "",
          reason: w.reason,
          cells: {},
          qty: 0,
          value: 0,
        };
        view.waste.push(line);
      }
      const cell = (line.cells[branch.id] ??= { qty: 0, value: 0 });
      cell.qty += w.qty;
      cell.value += w.value;
      line.qty += w.qty;
      line.value += w.value;
    }
    for (const a of live?.summary?.attention ?? []) {
      if (a.kind !== "low_stock") continue;
      view.live.push({
        branch: branch.name,
        product: a.text_params.product ?? "",
        qty: a.text_params.qty ?? "",
        asOf: live?.summary?.as_of ?? 0,
      });
    }
  }
  const out = [...views.values()];
  for (const v of out) {
    v.products.sort(byName);
    v.low.sort((a, b) => a.name.localeCompare(b.name) || a.branch.localeCompare(b.branch));
    v.waste.sort((a, b) => b.value - a.value || byName(a, b) || a.reason.localeCompare(b.reason));
    v.expiring.sort(
      (a, b) => a.expiresOn.localeCompare(b.expiresOn) || a.name.localeCompare(b.name),
    );
  }
  return out;
}

/** The reports that export to CSV (Story 8.17). */
export type ExportKind = "revenue" | "cash" | "stock";
export const EXPORTS = ["revenue", "cash", "stock"] as const satisfies readonly ExportKind[];

/** The export path of `kind`: one branch by id, several through `all` narrowed to the pick. */
export function exportPath(kind: ExportKind, picked: OwnerBranch[], from: string, to: string) {
  const query = `from=${from}&to=${to}&report=${kind}`; // i18n-ignore
  if (picked.length === 1) return `${base(picked[0].id)}/export?${query}`; // i18n-ignore
  const ids = picked.map((b) => encodeURIComponent(b.id)).join(",");
  return `/branches/all/export?${query}&branches=${ids}`; // i18n-ignore
}

/** Downloads the CSV of a report; `"failed"` when the cloud could not be reached or refused. */
export async function downloadCsv(
  kind: ExportKind,
  picked: OwnerBranch[],
  from: string,
  to: string,
): Promise<"ok" | "failed" | "signed-out"> {
  if (!picked.length) return "failed";
  const file = await download(exportPath(kind, picked, from, to));
  if (file === "signed-out") return "signed-out";
  if (!file) return "failed";
  const href = URL.createObjectURL(file);
  const link = document.createElement("a");
  link.href = href;
  link.download = `arena360-${kind}-${from}-to-${to}.csv`; // i18n-ignore
  document.body.append(link);
  link.click();
  link.remove();
  setTimeout(() => URL.revokeObjectURL(href), 10_000);
  return "ok";
}
