// Manage → Pricing (Story 8.23): what the owner sees of each branch's rates, plans, pricing rules
// and coupons, the Hub's field rules for editing them, and the per-branch commands an edit
// becomes. Ids are per branch, so an edit for "All branches" is matched by name and built once
// for each branch with that branch's own ids.

import { formatMoney, t } from "@arena/ui";
import type { CatalogueRow, OwnerBranch, SummaryCatalogue } from "@arena/ui/generated";
import type { MessageKey } from "@arena/ui/messages";
import type { CommandDraft, Excluded, Variant } from "./commands.svelte";
import { live, moneyLocale } from "./today.svelte";
import { ulid } from "./ulid";

// --- What each branch holds ---

/** A branch with the catalogue of its newest summary (null until it has sent one). */
export type BranchData = {
  branch: OwnerBranch;
  cat: SummaryCatalogue | null;
  /** When the Hub built that summary (Hub clock, ms). */
  asOf: number | null;
  currency: string;
  exponent: number;
};

export const ALL = "all";

/** Trimmed, case-insensitive: how items of different branches are told to be the same. */
export const norm = (s: string) => s.trim().toLowerCase();

/** The branches' catalogues from the shared `live` answers. */
export function dataOf(branches: OwnerBranch[]): BranchData[] {
  return branches.map((branch) => {
    const l = live[branch.id]?.data;
    const currency = l?.currency ?? "INR";
    return {
      branch,
      cat: l?.summary?.catalogue ?? null,
      asOf: l?.as_of ?? null,
      currency,
      exponent: moneyLocale(currency).exponent,
    };
  });
}

/** The strictest number of decimals among the branches: a typed amount must fit them all. */
export const exponentOf = (data: BranchData[]) =>
  data.length ? Math.min(...data.map((d) => d.exponent)) : 2;

export const moneyText = (minor: number, currency: string) =>
  formatMoney(minor, moneyLocale(currency));

/** An item of the café, with the branch rows that carry its name. */
export type Entry = { data: BranchData; row: CatalogueRow };
export type Listed = { key: string; name: string; entries: Entry[] };

/** Rows of every branch grouped by name, in the order they first appear. */
export function listed(data: BranchData[], pick: (c: SummaryCatalogue) => CatalogueRow[]) {
  const map = new Map<string, Listed>();
  for (const d of data) {
    for (const row of d.cat ? pick(d.cat) : []) {
      const key = norm(row.name);
      const item = map.get(key) ?? { key, name: row.name, entries: [] };
      item.entries.push({ data: d, row });
      map.set(key, item);
    }
  }
  return [...map.values()];
}

/** An older summary lacks `fields`: such an item is listed but cannot be edited. */
export const sourceOf = (item: Listed) => item.entries.find((e) => e.row.fields);

export const fieldsOf = (row: CatalogueRow) => (row.fields ?? {}) as Record<string, unknown>;
const num = (v: unknown) => (typeof v === "number" ? v : undefined);
const str = (v: unknown) => (typeof v === "string" ? v : undefined);

/** One group's rate at each branch that has the group. */
export type RateLine = {
  key: string;
  name: string;
  entries: { data: BranchData; group: CatalogueRow; rate: CatalogueRow | null }[];
};

export function rateLines(data: BranchData[]): RateLine[] {
  const map = new Map<string, RateLine>();
  for (const d of data) {
    for (const group of d.cat?.groups ?? []) {
      const key = norm(group.name);
      const line = map.get(key) ?? { key, name: group.name, entries: [] };
      const rate =
        d.cat?.rates.find(
          (r) => r.active !== false && (fieldsOf(r).group_id ?? null) === group.id,
        ) ??
        d.cat?.rates.find((r) => r.active !== false && norm(r.name) === key) ??
        null;
      line.entries.push({ data: d, group, rate });
      map.set(key, line);
    }
  }
  return [...map.values()];
}

/** A rate can be set when none exists yet, or when one of its rows came with its fields. */
export const rateEditable = (line: RateLine) =>
  line.entries.every((e) => !e.rate) || line.entries.some((e) => e.rate?.fields);

export const ratePerHour = (row: CatalogueRow) => {
  const minor = num(fieldsOf(row).price_per_hour_minor) ?? Number(row.detail);
  return Number.isFinite(minor) ? minor : null;
};

// --- Parsing, as the Hub does ---

/** Major units typed by the owner (`300`, `300.50`) as minor units, or `null`. */
export function parsePrice(text: string, exponent: number): number | null {
  const m = /^(\d{1,12})(?:\.(\d*))?$/.exec(text.trim());
  if (!m || (m[2] ?? "").length > exponent) return null;
  return Number(m[1]) * 10 ** exponent + Number((m[2] ?? "").padEnd(exponent, "0") || 0);
}

export const priceText = (minor: number, exponent: number) =>
  (minor / 10 ** exponent).toFixed(exponent);

/** Minutes from local midnight as `HH:MM`, the value of a time input. */
export function clock(minutes: number): string {
  const h = Math.floor(minutes / 60);
  return `${String(h).padStart(2, "0")}:${String(minutes % 60).padStart(2, "0")}`; // i18n-ignore (a time of day)
}

/** The minutes from local midnight of an `HH:MM` time input, or `null`. */
export function parseClock(text: string): number | null {
  const m = /^([01]\d|2[0-3]):([0-5]\d)$/.exec(text.trim());
  return m ? Number(m[1]) * 60 + Number(m[2]) : null;
}

/** A whole number from 1 to `max` (digits only), or `null`. */
export function parseWhole(text: string, max: number): number | null {
  if (!/^\d{1,7}$/.test(text.trim())) return null;
  const n = Number(text.trim());
  return n >= 1 && n <= max ? n : null;
}

/** A percentage such as `-20` or `12.5` as basis points, or `null`. */
export function parsePercent(text: string): number | null {
  const m = /^([+-])?(\d{1,4})(?:\.(\d{1,2}))?$/.exec(text.trim());
  if (!m) return null;
  const bp = Number(m[2]) * 100 + Number((m[3] ?? "").padEnd(2, "0") || 0);
  return m[1] === "-" ? -bp : bp;
}

/** Basis points as the text of an input, e.g. `-20` or `12.5`. */
export function percentInput(bp: number): string {
  const value = bp / 100;
  return Number.isInteger(value) ? String(value) : String(Number(value.toFixed(2)));
}

/** A coupon percentage (`10`, `12.5`) as basis points from 1 to 10 000, or `null`. */
export function parseCouponPercent(text: string): number | null {
  const m = /^(\d{1,3})(?:\.(\d{1,2}))?$/.exec(text.trim());
  if (!m) return null;
  const bp = Number(m[1]) * 100 + Number((m[2] ?? "").padEnd(2, "0") || 0);
  return bp >= 1 && bp <= 10_000 ? bp : null;
}

const DATE = /^\d{4}-\d{2}-\d{2}$/;
const DAY_MS = 86_400_000;
const dayNumber = (date: string) => Date.parse(date + "T00:00:00Z") / DAY_MS;
const validDate = (s: string) => DATE.test(s) && !Number.isNaN(dayNumber(s));

/** The Hub's code rule: trimmed, upper case, 3 to 24 of `A-Z 0-9 - _`. */
export const normaliseCode = (text: string) => text.trim().toUpperCase();
export const codeValid = (code: string) => /^[A-Z0-9_-]{3,24}$/.test(code);

const MAX_NAME = 80;
const MAX_HOURS = 1666;
const MAX_DAYS = 3650;

type Errors = Record<string, MessageKey>;

// --- Rates ---

export type RateForm = { group: string; price: string };

export function validateRate(f: RateForm, exponent: number) {
  const minor = parsePrice(f.price, exponent);
  const errors: Errors = minor === null || minor < 1 ? { price: "hub.groups.rate_invalid" } : {};
  return { errors, ok: Object.keys(errors).length === 0 };
}

export function rateFormOf(line: RateLine): RateForm {
  const source = line.entries.find((e) => e.rate?.fields) ?? line.entries.find((e) => e.rate);
  const minor = source?.rate ? ratePerHour(source.rate) : null;
  return {
    group: line.name,
    price: minor === null || !source ? "" : priceText(minor, source.data.exponent),
  };
}

// --- Plans ---

export type PlanKind = "pack" | "block" | "pass";
export type PlanForm = {
  kind: PlanKind;
  name: string;
  price: string;
  hours: string;
  days: string;
  start: string;
  end: string;
  /** Group names: ids differ per branch. */
  groups: string[];
  active: boolean;
};

export const blankPlan = (): PlanForm => ({
  kind: "pack",
  name: "",
  price: "",
  hours: "",
  days: "",
  start: "",
  end: "",
  groups: [],
  active: true,
});

function parseHours(text: string): number | null {
  if (!/^\d{1,4}(\.\d{1,2})?$/.test(text.trim())) return null;
  const minutes = Number(text.trim()) * 60;
  return Number.isInteger(minutes) && minutes >= 1 && minutes <= MAX_HOURS * 60 ? minutes : null;
}

export type PlanValues = {
  kind: PlanKind;
  name: string;
  price: string;
  minutes?: number;
  days?: number;
  start?: number;
  end?: number;
  groups: string[];
  active: boolean;
};

export function validatePlan(f: PlanForm, exponent: number, kinds: Map<string, string>) {
  const errors: Errors = {};
  const name = f.name.trim();
  if (name.length < 1 || [...name].length > MAX_NAME) errors.name = "hub.plans.invalid.name";
  const price = parsePrice(f.price, exponent);
  if (price === null || price < 1) errors.price = "hub.plans.invalid.price";
  const minutes = f.kind === "pack" ? parseHours(f.hours) : null;
  if (f.kind === "pack" && minutes === null) errors.hours = "hub.plans.invalid.hours";
  const days = f.kind === "block" ? null : parseWhole(f.days, MAX_DAYS);
  if (f.kind !== "block" && days === null) errors.days = "hub.plans.invalid.validity";
  const start = f.kind === "pack" ? null : parseClock(f.start);
  const end = f.kind === "pack" ? null : parseClock(f.end);
  if (f.kind !== "pack" && (start === null || end === null || start === end)) {
    errors.window = "hub.plans.invalid.window";
  }
  if (f.groups.length === 0) errors.groups = "hub.plans.invalid.groups";
  else {
    const consoles = f.groups.filter((g) => kinds.get(norm(g)) === "console").length;
    if (consoles !== 0 && consoles !== f.groups.length) errors.groups = "hub.plans.kind_mismatch";
  }
  const values: PlanValues = {
    kind: f.kind,
    name,
    price: f.price,
    groups: f.groups,
    active: f.active,
    ...(minutes === null ? {} : { minutes }),
    ...(days === null ? {} : { days }),
    ...(start === null ? {} : { start }),
    ...(end === null ? {} : { end }),
  };
  return { errors, values, ok: Object.keys(errors).length === 0 };
}

/** The names of the groups a branch row lists by id. */
const groupNames = (cat: SummaryCatalogue, ids: unknown) =>
  (Array.isArray(ids) ? ids : []).flatMap((id) => {
    const g = cat.groups.find((x) => x.id === id);
    return g ? [g.name] : [];
  });

export function planFormOf(item: Listed): PlanForm {
  const e = sourceOf(item);
  if (!e) return blankPlan();
  const f = fieldsOf(e.row);
  const minutes = num(f.minutes);
  const start = num(f.window_start_min);
  const end = num(f.window_end_min);
  return {
    kind: (str(f.plan_kind) as PlanKind) ?? "pack",
    name: e.row.name,
    price: priceText(num(f.price_minor) ?? 0, e.data.exponent),
    hours: minutes === undefined ? "" : String(minutes / 60),
    days: num(f.validity_days) === undefined ? "" : String(f.validity_days),
    start: start === undefined ? "" : clock(start),
    end: end === undefined ? "" : clock(end),
    groups: e.data.cat ? groupNames(e.data.cat, f.group_ids) : [],
    active: e.row.active ?? true,
  };
}

// --- Pricing rules ---

export type RuleKind = "weekly" | "date";
export type Adjust = "percent_bp" | "override_minor";
export type RuleForm = {
  kind: RuleKind;
  name: string;
  /** The group's name: ids differ per branch. */
  group: string;
  days: number[];
  start: string;
  end: string;
  from: string;
  to: string;
  adjust: Adjust;
  value: string;
  priority: string;
  active: boolean;
};

export const blankRule = (): RuleForm => ({
  kind: "weekly",
  name: "",
  group: "",
  days: [],
  start: "",
  end: "",
  from: "",
  to: "",
  adjust: "percent_bp",
  value: "",
  priority: "0",
  active: true,
});

/** The weekdays of a rule, Monday first, matching bit 0 of `days_mask`. */
export const DAYS: { bit: number; label: MessageKey }[] = [
  { bit: 0, label: "hub.rules.day.mon" },
  { bit: 1, label: "hub.rules.day.tue" },
  { bit: 2, label: "hub.rules.day.wed" },
  { bit: 3, label: "hub.rules.day.thu" },
  { bit: 4, label: "hub.rules.day.fri" },
  { bit: 5, label: "hub.rules.day.sat" },
  { bit: 6, label: "hub.rules.day.sun" },
];
export const maskOf = (bits: number[]) => bits.reduce((m, b) => m | (1 << b), 0);
export const bitsOf = (mask: number) => DAYS.map((d) => d.bit).filter((b) => (mask >> b) & 1);

export type RuleValues = {
  kind: RuleKind;
  name: string;
  group: string;
  mask?: number;
  start?: number;
  end?: number;
  from?: string;
  to?: string;
  adjust: Adjust;
  /** Basis points, or the typed rate (minor units differ per branch). */
  value: number | string;
  priority: number;
  active: boolean;
};

export function validateRule(f: RuleForm, exponent: number, groups: string[]) {
  const errors: Errors = {};
  const name = f.name.trim();
  if (name.length < 1 || [...name].length > MAX_NAME) errors.name = "hub.rules.invalid.name";
  if (!groups.some((g) => norm(g) === norm(f.group))) errors.group = "hub.rules.invalid.group";
  const weekly = f.kind === "weekly";
  if (weekly && f.days.length === 0) errors.days = "hub.rules.invalid.days";
  const start = weekly ? parseClock(f.start) : null;
  const end = weekly ? parseClock(f.end) : null;
  if (weekly && (start === null || end === null || start === end)) {
    errors.window = "hub.rules.invalid.window";
  }
  const from = f.from.trim();
  const to = f.to.trim() || from;
  if (!weekly) {
    if (!validDate(from) || !validDate(to) || to < from || dayNumber(to) - dayNumber(from) > 365) {
      errors.dates = "hub.rules.invalid.dates";
    }
  }
  const percent = f.adjust === "percent_bp";
  const value = percent ? parsePercent(f.value) : parsePrice(f.value, exponent);
  if (value === null || (percent ? value < -10_000 : value < 1)) {
    errors.adjust = "hub.rules.invalid.adjust";
  }
  const priority = f.priority.trim() === "0" ? 0 : parseWhole(f.priority, 1000);
  if (priority === null) errors.priority = "hub.rules.invalid.priority";
  const values: RuleValues = {
    kind: f.kind,
    name,
    group: f.group,
    adjust: f.adjust,
    value: percent ? (value ?? 0) : f.value,
    priority: priority ?? 0,
    active: f.active,
    ...(weekly ? { mask: maskOf(f.days), start: start ?? 0, end: end ?? 0 } : { from, to }),
  };
  return { errors, values, ok: Object.keys(errors).length === 0 };
}

export function ruleFormOf(item: Listed): RuleForm {
  const e = sourceOf(item);
  if (!e) return blankRule();
  const f = fieldsOf(e.row);
  const percent = f.adjust_kind === "percent_bp";
  const value = num(f.adjust_value) ?? 0;
  const start = num(f.start_min);
  const end = num(f.end_min);
  const from = str(f.date_from) ?? "";
  const to = str(f.date_to) ?? "";
  return {
    kind: (str(f.rule_kind) as RuleKind) ?? "weekly",
    name: e.row.name,
    group: e.data.cat?.groups.find((g) => g.id === f.group_id)?.name ?? "",
    days: bitsOf(num(f.days_mask) ?? 0),
    start: start === undefined ? "" : clock(start),
    end: end === undefined ? "" : clock(end),
    from,
    to: to && to !== from ? to : "",
    adjust: percent ? "percent_bp" : "override_minor",
    value: percent ? percentInput(value) : priceText(value, e.data.exponent),
    priority: String(num(f.priority) ?? 0),
    active: e.row.active ?? true,
  };
}

/** The days of a weekly rule in words, e.g. `Mon, Tue, Wed`. */
export const daysText = (mask: number) =>
  bitsOf(mask)
    .map((b) => t(DAYS[b].label))
    .join(", ");

/** When a rule applies, in words; empty for a row that came without its fields. */
export function scheduleText(row: CatalogueRow): string {
  const f = fieldsOf(row);
  if (f.rule_kind === "weekly") {
    return t("hub.rules.schedule.weekly", {
      days: daysText(num(f.days_mask) ?? 0),
      window: t("hub.plans.window", {
        start: clock(num(f.start_min) ?? 0),
        end: clock(num(f.end_min) ?? 0),
      }),
    });
  }
  if (f.rule_kind === "date") {
    const from = str(f.date_from) ?? "";
    const to = str(f.date_to) ?? from;
    return from === to
      ? t("hub.rules.schedule.day", { date: from })
      : t("hub.rules.schedule.range", { from, to });
  }
  return "";
}

/** What a rule does to the rate, e.g. `−20%` or `₹150.00 per hour`. */
export function adjustText(row: CatalogueRow, currency: string): string {
  const f = fieldsOf(row);
  const value = num(f.adjust_value);
  if (value === undefined) return "";
  if (f.adjust_kind === "percent_bp") {
    const v = Math.abs(value) / 100;
    const digits = Number.isInteger(v) ? String(v) : v.toFixed(2).replace(/0$/, "");
    return `${value < 0 ? "−" : "+"}${digits}%`; // i18n-ignore (a signed number)
  }
  return t("hub.rules.rate_per_hour", { rate: moneyText(value, currency) });
}

export type RuleStatus = "active" | "scheduled" | "ended" | "off";

const WEEKDAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/** The date, weekday (Monday 0) and minute of the day it is in `tz` at `now`. */
export function zonedNow(tz: string, now: number) {
  const at = (zone: string) =>
    new Intl.DateTimeFormat("en-US", {
      timeZone: zone,
      weekday: "short",
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
      hourCycle: "h23",
    }).formatToParts(now);
  let parts: Intl.DateTimeFormatPart[];
  try {
    parts = at(tz);
  } catch {
    parts = at("UTC");
  }
  const get = (type: string) => parts.find((p) => p.type === type)?.value ?? "";
  return {
    date: `${get("year")}-${get("month")}-${get("day")}`, // i18n-ignore (a date)
    dow: Math.max(0, WEEKDAYS.indexOf(get("weekday"))),
    minute: (Number(get("hour")) % 24) * 60 + Number(get("minute")),
  };
}

/**
 * `active` now (weekly: today's weekday and the minute in the window, in the branch zone; a
 * window past midnight belongs to the day it starts on; date: today within the dates),
 * `scheduled`, `ended`, or `off` when inactive. Null for a row that came without its fields.
 */
export function ruleStatus(row: CatalogueRow, tz: string, now: number): RuleStatus | null {
  if (row.active === false) return "off";
  const f = fieldsOf(row);
  const at = zonedNow(tz, now);
  if (f.rule_kind === "weekly") {
    const mask = num(f.days_mask) ?? 0;
    const start = num(f.start_min) ?? 0;
    const end = num(f.end_min) ?? 0;
    const today = (mask >> at.dow) & 1;
    const yesterday = (mask >> ((at.dow + 6) % 7)) & 1;
    const inside =
      start < end
        ? !!today && at.minute >= start && at.minute < end
        : start > end && ((!!today && at.minute >= start) || (!!yesterday && at.minute < end));
    return inside ? "active" : "scheduled";
  }
  if (f.rule_kind === "date") {
    const from = str(f.date_from) ?? "";
    const to = str(f.date_to) ?? from;
    return at.date < from ? "scheduled" : at.date > to ? "ended" : "active";
  }
  return null;
}

// --- Plans in words ---

export const timeText = (minutes: number) => {
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  return m ? t("hub.plans.time_hours_minutes", { h, m }) : t("hub.plans.time_hours", { h });
};

/** What a plan gives, in words: its time or window and its validity. */
export function termsText(row: CatalogueRow): string {
  const f = fieldsOf(row);
  const days = num(f.validity_days) ?? 0;
  if (f.plan_kind === "pack") {
    return t("hub.plans.terms.pack", { time: timeText(num(f.minutes) ?? 0), days });
  }
  if (f.plan_kind !== "block" && f.plan_kind !== "pass") return "";
  const window = t("hub.plans.window", {
    start: clock(num(f.window_start_min) ?? 0),
    end: clock(num(f.window_end_min) ?? 0),
  });
  return f.plan_kind === "block"
    ? t("hub.plans.terms.block", { window })
    : t("hub.plans.terms.pass", { window, days });
}

// --- Coupons ---

export type CouponKind = "percent" | "amount";
export type CouponForm = {
  code: string;
  kind: CouponKind;
  value: string;
  from: string;
  to: string;
  limit: string;
  minimum: string;
  groups: string[];
  plans: string[];
  products: string[];
  active: boolean;
};

export const blankCoupon = (): CouponForm => ({
  code: "",
  kind: "percent",
  value: "",
  from: "",
  to: "",
  limit: "",
  minimum: "",
  groups: [],
  plans: [],
  products: [],
  active: true,
});

export type CouponValues = {
  code: string;
  kind: CouponKind;
  /** Basis points, or the typed amount (minor units differ per branch). */
  value: number | string;
  from?: string;
  to?: string;
  limit?: number;
  minimum: string;
  groups: string[];
  plans: string[];
  products: string[];
  active: boolean;
};

export function validateCoupon(f: CouponForm, exponent: number) {
  const errors: Errors = {};
  const code = normaliseCode(f.code);
  if (!codeValid(code)) errors.code = "hub.coupons.invalid.code";
  const percent = f.kind === "percent" ? parseCouponPercent(f.value) : null;
  const amount = f.kind === "amount" ? parsePrice(f.value, exponent) : null;
  if (f.kind === "percent" && percent === null) errors.value = "hub.coupons.invalid.percent";
  if (f.kind === "amount" && (amount === null || amount < 1)) {
    errors.value = "hub.coupons.invalid.amount";
  }
  const from = f.from.trim();
  const to = f.to.trim();
  if ((from && !validDate(from)) || (to && !validDate(to)) || (from && to && to < from)) {
    errors.dates = "hub.coupons.invalid.dates";
  }
  const limit = f.limit.trim() === "" ? undefined : parseWhole(f.limit, 1_000_000_000);
  if (limit === null) errors.limit = "hub.coupons.invalid.limit";
  const minimum = f.minimum.trim() === "" ? 0 : parsePrice(f.minimum, exponent);
  if (minimum === null) errors.minimum = "hub.coupons.invalid.minimum";
  const values: CouponValues = {
    code,
    kind: f.kind,
    value: f.kind === "percent" ? (percent ?? 0) : f.value,
    minimum: f.minimum,
    groups: f.groups,
    plans: f.plans,
    products: f.products,
    active: f.active,
    ...(from ? { from } : {}),
    ...(to ? { to } : {}),
    ...(limit ? { limit } : {}),
  };
  return { errors, values, ok: Object.keys(errors).length === 0 };
}

const namesOf = (rows: CatalogueRow[], targets: { kind: string; id: string }[], kind: string) =>
  targets
    .filter((x) => x.kind === kind)
    .flatMap((x) => {
      const r = rows.find((row) => row.id === x.id);
      return r ? [r.name] : [];
    });

export function couponFormOf(item: Listed): CouponForm {
  const e = sourceOf(item);
  if (!e) return blankCoupon();
  const f = fieldsOf(e.row);
  const cat = e.data.cat;
  const targets = (Array.isArray(f.targets) ? f.targets : []) as { kind: string; id: string }[];
  const percent = num(f.percent_bp);
  const minimum = num(f.min_bill_minor) ?? 0;
  return {
    code: e.row.name,
    kind: percent === undefined ? "amount" : "percent",
    value:
      percent === undefined
        ? priceText(num(f.amount_minor) ?? 0, e.data.exponent)
        : percentInput(percent),
    from: str(f.valid_from) ?? "",
    to: str(f.valid_to) ?? "",
    limit: num(f.usage_limit) === undefined ? "" : String(f.usage_limit),
    minimum: minimum > 0 ? priceText(minimum, e.data.exponent) : "",
    groups: cat ? namesOf(cat.groups, targets, "group") : [],
    plans: cat ? namesOf(cat.plans, targets, "plan") : [],
    products: cat ? namesOf([...cat.products, ...cat.bundles], targets, "product") : [],
    active: e.row.active ?? true,
  };
}

/** The offer of a coupon in words, e.g. `10% off` or `₹50.00 off`. */
export function offText(row: CatalogueRow, currency: string): string {
  const f = fieldsOf(row);
  const percent = num(f.percent_bp);
  if (percent !== undefined) {
    return t("hub.coupons.off_percent", { percent: `${percentInput(percent)}%` }); // i18n-ignore (a number)
  }
  const amount = num(f.amount_minor);
  return amount === undefined
    ? ""
    : t("hub.coupons.off_amount", { amount: moneyText(amount, currency) });
}

/** The days a coupon is valid, in words. */
export function windowText(row: CatalogueRow): string {
  const f = fieldsOf(row);
  const from = str(f.valid_from);
  const to = str(f.valid_to);
  return from && to
    ? t("hub.coupons.window.both", { from, to })
    : from
      ? t("hub.coupons.window.from", { date: from })
      : to
        ? t("hub.coupons.window.to", { date: to })
        : t("hub.coupons.window.always");
}

// --- Choices the forms offer, by name over the branches in scope ---

export type Choice = { key: string; name: string; kind?: string };

function choices(data: BranchData[], pick: (c: SummaryCatalogue) => CatalogueRow[]): Choice[] {
  const map = new Map<string, Choice>();
  for (const d of data) {
    for (const r of d.cat ? pick(d.cat) : []) {
      const key = norm(r.name);
      if (!map.has(key)) map.set(key, { key, name: r.name, kind: r.detail });
    }
  }
  return [...map.values()];
}

export const groupChoices = (data: BranchData[]) => choices(data, (c) => c.groups);
export const planChoices = (data: BranchData[]) => choices(data, (c) => c.plans);
export const productChoices = (data: BranchData[]) =>
  choices(data, (c) => [...c.products, ...c.bundles]);

// --- From a form to the commands ---

/** Where a form's change goes: the scope, the branches in it and the item being edited. */
export type Ctx = {
  /** `all` or a branch id. */
  scope: string;
  data: BranchData[];
  /** The normalised name the item had when the edit started; null when adding. */
  origin: string | null;
};

type Made = { payload: Record<string, unknown> } | { reason: string };

type Spec = {
  kind: string;
  /** The name as typed, for the preview. */
  label: string;
  /** The rows of this kind at a branch. */
  list: (c: SummaryCatalogue) => CatalogueRow[];
  /** The normalised name a new item would have. */
  name: string;
  /** A rate has no id: the group is the key. */
  keyed?: boolean;
  /** The payload for a branch, or why the branch can't take it. */
  make: (d: BranchData, cat: SummaryCatalogue) => Made;
};

const idByName = (rows: CatalogueRow[], name: string) =>
  rows.find((r) => norm(r.name) === norm(name))?.id;

const missing = (name: string): Made => ({
  reason: t("owner.pricing.excluded.missing", { name }),
});

function resolve(ctx: Ctx, p: Spec): CommandDraft {
  const variants: Variant[] = [];
  const excluded: Excluded[] = [];
  const many = ctx.scope === ALL;
  for (const d of ctx.data) {
    const branch_id = d.branch.id;
    if (!d.cat) {
      excluded.push({ branch_id, reason: t("owner.pricing.excluded.nodata") });
      continue;
    }
    const made = p.make(d, d.cat);
    if ("reason" in made) {
      excluded.push({ branch_id, reason: made.reason });
      continue;
    }
    const existing = p.list(d.cat).find((r) => norm(r.name) === (ctx.origin ?? p.name));
    if (ctx.origin === null) {
      if (existing) {
        excluded.push({
          branch_id,
          reason: t("owner.pricing.excluded.exists", { name: existing.name }),
        });
      } else variants.push({ branch_id, payload: made.payload, key: ulid() });
    } else if (existing) {
      variants.push({
        branch_id,
        payload: p.keyed ? made.payload : { id: existing.id, ...made.payload },
        base_rev: existing.rev,
        key: ulid(),
      });
    } else {
      // Missing here by name: created here, once the owner says so when several branches are in scope.
      variants.push({
        branch_id,
        payload: made.payload,
        key: ulid(),
        ...(many ? { ask: true } : {}),
      });
    }
  }
  const { id: _id, ...payload } = variants[0]?.payload ?? (p.keyed ? {} : { name: p.label });
  return {
    kind: p.kind,
    payload,
    branch_id: ctx.scope,
    key: ulid(),
    variants,
    excluded,
    choices: {},
  };
}

const money = (text: string, d: BranchData) => parsePrice(text, d.exponent);
const badAmount = (): Made => ({ reason: t("owner.pricing.excluded.amount") });

export function proposeRate(ctx: Ctx, f: RateForm): CommandDraft {
  return resolve(ctx, {
    kind: "rate.upsert",
    label: f.group,
    list: (c) => c.rates.filter((r) => r.active !== false),
    name: norm(f.group),
    keyed: true,
    make: (d, cat) => {
      const group_id = idByName(cat.groups, f.group);
      if (group_id === undefined) return missing(f.group);
      const price = money(f.price, d);
      if (price === null || price < 1) return badAmount();
      return { payload: { group_id, price_per_hour_minor: price } };
    },
  });
}

export function proposePlan(ctx: Ctx, v: PlanValues): CommandDraft {
  return resolve(ctx, {
    kind: "plan.upsert",
    label: v.name,
    list: (c) => c.plans,
    name: norm(v.name),
    make: (d, cat) => {
      const group_ids: string[] = [];
      for (const g of v.groups) {
        const id = idByName(cat.groups, g);
        if (id === undefined) return missing(g);
        group_ids.push(id);
      }
      const price = money(v.price, d);
      if (price === null || price < 1) return badAmount();
      return {
        payload: {
          plan_kind: v.kind,
          name: v.name,
          price_minor: price,
          ...(v.minutes === undefined ? {} : { minutes: v.minutes }),
          ...(v.days === undefined ? {} : { validity_days: v.days }),
          ...(v.start === undefined ? {} : { window_start_min: v.start }),
          ...(v.end === undefined ? {} : { window_end_min: v.end }),
          group_ids,
          active: v.active,
        },
      };
    },
  });
}

export function proposeRule(ctx: Ctx, v: RuleValues): CommandDraft {
  return resolve(ctx, {
    kind: "rule.upsert",
    label: v.name,
    list: (c) => c.pricing_rules,
    name: norm(v.name),
    make: (d, cat) => {
      const group_id = idByName(cat.groups, v.group);
      if (group_id === undefined) return missing(v.group);
      const value = typeof v.value === "number" ? v.value : money(v.value, d);
      if (value === null || (v.adjust === "override_minor" && value < 1)) return badAmount();
      return {
        payload: {
          name: v.name,
          group_id,
          rule_kind: v.kind,
          ...(v.kind === "weekly"
            ? { days_mask: v.mask, start_min: v.start, end_min: v.end }
            : { date_from: v.from, date_to: v.to || v.from }),
          adjust_kind: v.adjust,
          adjust_value: value,
          priority: v.priority,
          active: v.active,
        },
      };
    },
  });
}

export function proposeCoupon(ctx: Ctx, v: CouponValues): CommandDraft {
  return resolve(ctx, {
    kind: "coupon.upsert",
    label: v.code,
    list: (c) => c.coupons,
    name: norm(v.code),
    make: (d, cat) => {
      const targets: { kind: string; id: string }[] = [];
      const sets: [string, string[], CatalogueRow[]][] = [
        ["group", v.groups, cat.groups],
        ["plan", v.plans, cat.plans],
        ["product", v.products, [...cat.products, ...cat.bundles]],
      ];
      for (const [kind, names, rows] of sets) {
        for (const name of names) {
          const id = idByName(rows, name);
          if (id === undefined) return missing(name);
          targets.push({ kind, id });
        }
      }
      const amount =
        v.kind === "amount" ? (typeof v.value === "number" ? v.value : money(v.value, d)) : null;
      if (v.kind === "amount" && (amount === null || amount < 1)) return badAmount();
      const minimum = v.minimum.trim() === "" ? 0 : money(v.minimum, d);
      if (minimum === null) return badAmount();
      return {
        payload: {
          code: v.code,
          ...(v.kind === "percent" ? { percent_bp: v.value } : { amount_minor: amount }),
          ...(v.from ? { valid_from: v.from } : {}),
          ...(v.to ? { valid_to: v.to } : {}),
          ...(v.limit ? { usage_limit: v.limit } : {}),
          min_bill_minor: minimum,
          targets,
          active: v.active,
        },
      };
    },
  });
}
