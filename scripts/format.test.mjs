// Unit tests for the shared money and date formatters (packages/ui/src/format.js).

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  CURRENCIES,
  MINUS,
  TIME_ZONES,
  currencyOf,
  formatBusinessDay,
  formatDateTime,
  formatMoney,
  formatTime,
  groupDigits,
  isTimeZone,
  nextDay,
  zonedDayStart,
} from "../packages/ui/src/format.js";

/** A locale for `code` from the generated table. */
function locale(code, symbol_position = "before", number_format = "international") {
  const c = currencyOf(code);
  assert.ok(c, code);
  return { exponent: c.exponent, symbol: c.symbol, symbol_position, number_format };
}

test("the generated table has the required currencies", () => {
  for (const [code, exponent, symbol] of [
    ["INR", 2, "₹"],
    ["USD", 2, "$"],
    ["EUR", 2, "€"],
    ["GBP", 2, "£"],
    ["AED", 2, "د.إ"],
    ["SGD", 2, "S$"],
    ["JPY", 0, "¥"],
    ["KWD", 3, "KD"],
    ["BHD", 3, "BD"],
  ]) {
    assert.deepEqual(currencyOf(code), { code, exponent, symbol });
  }
  assert.equal(currencyOf("XYZ"), undefined);
  assert.ok(CURRENCIES.length >= 9);
});

test("Indian format", () => {
  assert.equal(formatMoney(12345678, locale("INR", "before", "indian")), "₹1,23,456.78");
  assert.equal(formatMoney(123456789012n, locale("INR", "before", "indian")), "₹1,23,45,67,890.12");
});

test("international format", () => {
  assert.equal(formatMoney(12345678, locale("USD")), "$123,456.78");
  assert.equal(formatMoney(100000000, locale("USD")), "$1,000,000.00");
});

test("symbol after, with a space", () => {
  assert.equal(formatMoney(12345678, locale("EUR", "after")), "123,456.78 €");
});

test("zero exponent has no decimals", () => {
  assert.equal(formatMoney(1234567, locale("JPY")), "¥1,234,567");
  assert.equal(formatMoney(0, locale("JPY")), "¥0");
});

test("three decimals", () => {
  assert.equal(formatMoney(1234567, locale("KWD")), "KD1,234.567");
  assert.equal(formatMoney(5, locale("BHD")), "BD0.005");
});

test("negative amounts start with U+2212", () => {
  assert.equal(formatMoney(-5000, locale("INR", "before", "indian")), "−₹50.00");
  assert.equal(MINUS, "−");
  assert.equal(formatMoney(-12345678, locale("EUR", "after")), "−123,456.78 €");
  assert.equal(formatMoney(-1, locale("USD")), "−$0.01");
});

test("small amounts are zero-padded", () => {
  assert.equal(formatMoney(7, locale("USD")), "$0.07");
  assert.equal(formatMoney(0, locale("INR", "before", "indian")), "₹0.00");
});

test("exact beyond float precision", () => {
  // 2^63 - 1 minor units: a float would round the last digits.
  assert.equal(formatMoney(9223372036854775807n, locale("USD")), "$92,233,720,368,547,758.07");
  assert.throws(() => formatMoney(1.5, locale("USD")), RangeError);
  assert.throws(() => formatMoney(2 ** 60, locale("USD")), RangeError);
});

test("digit grouping", () => {
  assert.equal(groupDigits("1", "indian"), "1");
  assert.equal(groupDigits("123", "indian"), "123");
  assert.equal(groupDigits("1234", "indian"), "1,234");
  assert.equal(groupDigits("123456", "indian"), "1,23,456");
  assert.equal(groupDigits("1234567", "indian"), "12,34,567");
  assert.equal(groupDigits("1234567", "international"), "1,234,567");
});

test("dates and times use the branch time zone", () => {
  // 2027-01-15 08:30 UTC is 14:00 in Kolkata and 12:30 in Dubai.
  const ts = Date.UTC(2027, 0, 15, 8, 30);
  const opts = { hour: "2-digit", minute: "2-digit", hourCycle: "h23" };
  assert.match(formatDateTime(ts, "Asia/Kolkata", opts), /14:00/);
  assert.match(formatDateTime(ts, "Asia/Dubai", opts), /12:30/);
  assert.ok(formatTime(ts, "Asia/Kolkata").length > 0);
  // An unknown zone falls back to the runtime's.
  assert.ok(formatDateTime(ts, "Mars/Base").length > 0);
  assert.equal(isTimeZone("Asia/Kolkata"), true);
  assert.equal(isTimeZone("Mars/Base"), false);
});

test("a business day is shown as its own calendar date", () => {
  assert.match(formatBusinessDay("2027-01-16"), /16/);
  assert.match(formatBusinessDay("2027-01-16"), /2027/);
  assert.equal(formatBusinessDay("not a date"), "not a date");
});

test("the generated time zone list has UTC and the branch zones", () => {
  for (const tz of ["UTC", "Asia/Kolkata", "Asia/Dubai", "Europe/London"]) {
    assert.ok(TIME_ZONES.includes(tz), tz);
  }
  assert.deepEqual([...TIME_ZONES].sort(), [...TIME_ZONES]);
});

test("a date in the branch zone starts at its local midnight", () => {
  // Kolkata is UTC+5:30 all year.
  assert.equal(zonedDayStart("2026-10-23", "Asia/Kolkata"), Date.UTC(2026, 9, 22, 18, 30));
  assert.equal(zonedDayStart("2026-10-23", "UTC"), Date.UTC(2026, 9, 23));
  // New York on the day DST ends (midnight is still EDT, UTC−4).
  assert.equal(zonedDayStart("2026-11-01", "America/New_York"), Date.UTC(2026, 10, 1, 4));
  assert.equal(zonedDayStart("2026-11-02", "America/New_York"), Date.UTC(2026, 10, 2, 5));
  assert.equal(zonedDayStart("2026-10-23", "Not/AZone"), Date.UTC(2026, 9, 23));
  assert.equal(zonedDayStart("23/10/2026", "UTC"), null);
});

test("a day whose midnight is skipped starts when the clocks jump", () => {
  // Cuba springs forward at 00:00 on 2026-03-08 (UTC−5 → UTC−4): the day starts at 01:00.
  assert.equal(zonedDayStart("2026-03-08", "America/Havana"), Date.UTC(2026, 2, 8, 5));
  assert.equal(zonedDayStart("2026-03-09", "America/Havana"), Date.UTC(2026, 2, 9, 4));
  assert.equal(zonedDayStart("2026-03-07", "America/Havana"), Date.UTC(2026, 2, 7, 5));
});

test("nextDay rolls months and years", () => {
  assert.equal(nextDay("2026-10-23"), "2026-10-24");
  assert.equal(nextDay("2026-12-31"), "2027-01-01");
  assert.equal(nextDay("2028-02-28"), "2028-02-29");
  assert.equal(nextDay("nope"), null);
});
