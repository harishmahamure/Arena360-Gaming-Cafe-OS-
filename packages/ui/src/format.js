// The shared money and date formatters. Money follows the branch locale (`HubSnapshot.locale`);
// pair formatted amounts with the `.tabular-nums` class so columns of figures line up.

import currencies from "./generated/currencies.json" with { type: "json" };
import timezones from "./generated/timezones.json" with { type: "json" };

/** @typedef {import("./generated/index").SnapshotLocale} SnapshotLocale */
/**
 * What `formatMoney` needs from the locale.
 * @typedef {Pick<SnapshotLocale, "exponent" | "symbol" | "symbol_position" | "number_format">} MoneyLocale
 */

/**
 * The currency table generated from `arena_proto::currency::CURRENCIES`.
 * @type {ReadonlyArray<{ code: string; exponent: number; symbol: string }>}
 */
export const CURRENCIES = currencies;

/**
 * The IANA zone names the Hub accepts for `branch.tz` (generated from its tz database), sorted.
 * @type {ReadonlyArray<string>}
 */
export const TIME_ZONES = timezones;

/**
 * The currency with ISO code `code`, if the table has it.
 * @param {string} code
 */
export function currencyOf(code) {
  return CURRENCIES.find((c) => c.code === code);
}

/** U+2212 MINUS SIGN, which lines up with the digits (a hyphen does not). */
export const MINUS = "−";

/**
 * Groups a string of digits: Indian style (the last three digits, then pairs: 1,23,45,678) or
 * internationally (threes: 12,345,678).
 * @param {string} digits
 * @param {"indian" | "international"} style
 */
export function groupDigits(digits, style) {
  if (digits.length <= 3) return digits;
  const head = digits.slice(0, -3);
  const tail = digits.slice(-3);
  const size = style === "indian" ? 2 : 3;
  const groups = [];
  for (let end = head.length; end > 0; end -= size) {
    groups.unshift(head.slice(Math.max(0, end - size), end));
  }
  return [...groups, tail].join(",");
}

/**
 * Formats an amount held in integer minor units, with exact integer arithmetic (no float maths):
 * 12345678 INR (exponent 2, Indian grouping) is `₹1,23,456.78`; with the symbol after it is
 * `123,456.78 €`; a zero exponent has no decimals; a negative amount starts with `−`.
 * @param {number | bigint} minor an integer
 * @param {MoneyLocale} locale
 * @returns {string}
 */
export function formatMoney(minor, locale) {
  if (typeof minor === "number" && !Number.isSafeInteger(minor)) {
    throw new RangeError(`formatMoney: ${minor} is not a safe integer of minor units`); // i18n-ignore
  }
  const value = BigInt(minor);
  const negative = value < 0n;
  const exponent = Math.max(0, Math.trunc(locale.exponent));
  const digits = (negative ? -value : value).toString().padStart(exponent + 1, "0");
  const whole = digits.slice(0, digits.length - exponent);
  const fraction = digits.slice(digits.length - exponent);
  const number = groupDigits(whole, locale.number_format) + (exponent > 0 ? "." + fraction : "");
  const amount =
    locale.symbol_position === "after" ? `${number} ${locale.symbol}` : locale.symbol + number; // i18n-ignore
  return negative ? MINUS + amount : amount;
}

/**
 * Whether `tz` is an IANA zone this runtime knows.
 * @param {string} tz
 */
export function isTimeZone(tz) {
  try {
    new Intl.DateTimeFormat(undefined, { timeZone: tz });
    return true;
  } catch {
    return false;
  }
}

/**
 * A date and time in the branch time zone (`Intl`, the runtime's language). An unknown zone
 * falls back to the runtime's own.
 * @param {Date | number} ts a Date or UTC ms
 * @param {string | null | undefined} tz IANA zone
 * @param {Intl.DateTimeFormatOptions} [options] defaults to a medium date and a short time
 */
export function formatDateTime(ts, tz, options = { dateStyle: "medium", timeStyle: "short" }) {
  const timeZone = tz && isTimeZone(tz) ? tz : undefined;
  return new Intl.DateTimeFormat(undefined, { ...options, timeZone }).format(ts);
}

/**
 * The time of day in the branch time zone, e.g. `14:05` or `2:05 PM` (the header clock).
 * @param {Date | number} ts
 * @param {string | null | undefined} tz
 */
export function formatTime(ts, tz) {
  return formatDateTime(ts, tz, { hour: "numeric", minute: "2-digit" });
}

/**
 * A business day (`YYYY-MM-DD`, a calendar date of the branch) as a medium date. The date is
 * already a date in the branch zone, so it is shown as is, whatever zone this PC is in.
 * @param {string} day
 */
export function formatBusinessDay(day) {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(day);
  if (!m) return day;
  const utc = Date.UTC(Number(m[1]), Number(m[2]) - 1, Number(m[3]), 12);
  return new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeZone: "UTC" }).format(utc);
}

/**
 * The wall-clock date and time of the instant `utcMs` in `tz`.
 * @param {number} utcMs
 * @param {string} tz
 */
function wallClock(utcMs, tz) {
  const parts = new Intl.DateTimeFormat("en-US", {
    timeZone: tz,
    hourCycle: "h23",
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  }).formatToParts(utcMs);
  /** @param {string} type */
  const n = (type) => Number(parts.find((p) => p.type === type)?.value);
  const day = `${String(n("year")).padStart(4, "0")}-${String(n("month")).padStart(2, "0")}-${String(n("day")).padStart(2, "0")}`; // i18n-ignore (an ISO date)
  const asUtc = Date.UTC(n("year"), n("month") - 1, n("day"), n("hour"), n("minute"), n("second"));
  return {
    day,
    offset: asUtc - Math.floor(utcMs / 1000) * 1000,
    midnight: asUtc % 86_400_000 === 0,
  };
}

/**
 * The UTC ms of the first instant of calendar date `day` (`YYYY-MM-DD`) in the zone `tz`:
 * its 00:00, or, when a clock change skips midnight that day, the moment the clocks jump
 * forward. An unknown zone counts as UTC. `null` when `day` is not a date. Used to turn a
 * date picked in the branch time zone into a range bound.
 * @param {string} day
 * @param {string | null | undefined} tz IANA zone
 * @returns {number | null}
 */
export function zonedDayStart(day, tz) {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(day);
  if (!m) return null;
  const local = Date.UTC(Number(m[1]), Number(m[2]) - 1, Number(m[3]));
  if (Number.isNaN(local)) return null;
  if (!tz || !isTimeZone(tz)) return local;
  const HALF_DAY = 43_200_000;
  // Midnight under the offset in force half a day before, and half a day after.
  const candidates = [
    local - wallClock(local - HALF_DAY, tz).offset,
    local - wallClock(local + HALF_DAY, tz).offset,
  ].sort((a, b) => a - b);
  for (const c of candidates) {
    const w = wallClock(c, tz);
    if (w.day === day && w.midnight) return c;
  }
  // Midnight does not exist that day: find the first instant whose date is `day`.
  let lo = candidates[0] - HALF_DAY;
  let hi = candidates[1] + HALF_DAY;
  while (hi - lo > 1) {
    const mid = lo + Math.floor((hi - lo) / 2);
    if (wallClock(mid, tz).day >= day) hi = mid;
    else lo = mid;
  }
  return hi;
}

/**
 * The calendar date after `day` (`YYYY-MM-DD`), e.g. for an inclusive "to" date.
 * @param {string} day
 * @returns {string | null}
 */
export function nextDay(day) {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(day);
  if (!m) return null;
  const d = new Date(Date.UTC(Number(m[1]), Number(m[2]) - 1, Number(m[3]) + 1));
  return d.toISOString().slice(0, 10);
}
