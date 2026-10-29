#!/usr/bin/env node
// Story 7.11: merges the kitchen-flow results of one release run into a report, publishes the
// Kitchen display's browser results to the compatibility list, and fails the release when a budget
// is missed or a measurement is missing.
//
//   node scripts/kitchen-report.mjs [--dir target/kitchen] [--out target/kitchen/report.md]
//                                   [--compat docs/compat/kitchen-display.md] [--date YYYY-MM-DD]
//
// `--dir` holds what the jobs of release-bench.yml wrote (a file absent from disk is a failure,
// never a pass):
//   hub.json            Journey 4 over the LAN (arena-api lan.rs): kot_ms, board_ms, hud_ms, tile_ms
//   unplugged.json      the unplugged-printer run: alert, board
//   burst.json          5 orders/s across 20 kiosks and 2 displays: orders, kots, kot_once_each,
//                       kot_p95_ms, boards_match, rate, secs, kiosks, displays
//   soak-wan-up.json    the 500-order soak with a route out, and
//   soak-wan-down.json  the same soak with no route out: orders, tickets, served, advances, kots,
//                       duplicate_kots, lost
//   browser-<name>.json one Playwright JSON report per browser (apps/hub `test:kitchen`), `<name>`
//                       being `android-chrome` or `ipad-safari`
// Budgets are the story's numbers; KITCHEN_KOT_MS, KITCHEN_BOARD_MS, KITCHEN_HUD_MS,
// KITCHEN_TILE_MS and KITCHEN_P95_MS override them for a trial, and KITCHEN_SOAK_ORDERS lowers the
// order count the report accepts (500 otherwise).

import {
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  realpathSync,
  writeFileSync,
} from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

export const COMPAT = "docs/compat/kitchen-display.md";
/** The browsers the Kitchen display must pass on, in the order they are listed. */
export const BROWSERS = [
  ["android-chrome", "Chrome on Android"],
  ["ipad-safari", "Safari on iPad"],
];
const COMPAT_HEAD = "| Browser | Passed | Failed | Last run |";

const num = (v, fallback) => (v !== undefined && Number.isFinite(Number(v)) ? Number(v) : fallback);
const isNum = (v) => typeof v === "number" && Number.isFinite(v);

/** The story's budgets, with the environment's overrides. */
export function budgets(env = process.env) {
  return {
    kotMs: num(env.KITCHEN_KOT_MS, 3000),
    boardMs: num(env.KITCHEN_BOARD_MS, 2000),
    hudMs: num(env.KITCHEN_HUD_MS, 5000),
    tileMs: num(env.KITCHEN_TILE_MS, 2000),
    p95Ms: num(env.KITCHEN_P95_MS, 3000),
    soakOrders: num(env.KITCHEN_SOAK_ORDERS, 500),
    burstRate: 5,
    burstSecs: num(env.KITCHEN_BURST_SECS, 60),
    burstKiosks: 20,
    burstDisplays: 2,
  };
}

/** Reads every result file of `dir`; a file that is absent or not JSON is left out. */
export function load(dir) {
  const read = (name) => {
    const file = join(dir, name);
    if (!existsSync(file)) return undefined;
    try {
      return JSON.parse(readFileSync(file, "utf8"));
    } catch {
      return undefined;
    }
  };
  return {
    hub: read("hub.json"),
    unplugged: read("unplugged.json"),
    burst: read("burst.json"),
    soakUp: read("soak-wan-up.json"),
    soakDown: read("soak-wan-down.json"),
    browsers: Object.fromEntries(BROWSERS.map(([key]) => [key, read(`browser-${key}.json`)])),
  };
}

/** Passed and failed test counts of a Playwright JSON report, or undefined when it has none. */
export function tally(report) {
  const s = report?.stats;
  if (!s || !isNum(s.expected) || !isNum(s.unexpected)) return undefined;
  return { passed: s.expected, failed: s.unexpected + (s.flaky ?? 0) };
}

/** The soak outcome without what only names the run. */
const outcome = (s) => {
  const { label: _label, source: _source, ...rest } = s;
  return JSON.stringify(rest, Object.keys(rest).sort());
};

/** The report's rows: {what, value, budget, ok}; a measurement that is absent is a failed row. */
export function evaluate(results, b = budgets()) {
  const rows = [];
  const row = (what, value, budget, ok) => rows.push({ what, value, budget, ok });
  const missing = (what, why) => rows.push({ what, value: "missing", budget: why, ok: false });

  const { hub } = results;
  for (const [what, key, budget] of [
    ["KOT prints after commit", "kot_ms", b.kotMs],
    ["Ticket appears on the board", "board_ms", b.boardMs],
    ["HUD ready copy after the Ready tap", "hud_ms", b.hudMs],
    ["Tile shows READY after the Ready tap", "tile_ms", b.tileMs],
  ]) {
    if (isNum(hub?.[key])) row(what, `${hub[key]} ms`, `<= ${budget} ms`, hub[key] <= budget);
    else missing(what, "needs hub.json");
  }

  const u = results.unplugged;
  if (u) {
    row(
      "Printer unplugged: Printer offline alert",
      u.alert ? "yes" : "no",
      "yes",
      u.alert === true,
    );
    row(
      "Printer unplugged: board gets the ticket",
      u.board ? "yes" : "no",
      "yes",
      u.board === true,
    );
  } else {
    missing("Printer unplugged", "needs unplugged.json");
  }

  const t = results.burst;
  if (t) {
    const shape =
      t.rate >= b.burstRate &&
      t.secs >= b.burstSecs &&
      t.kiosks >= b.burstKiosks &&
      t.displays >= b.burstDisplays;
    row(
      "Burst shape",
      `${t.rate}/s for ${t.secs} s, ${t.kiosks} kiosks, ${t.displays} displays`,
      `>= ${b.burstRate}/s for ${b.burstSecs} s, ${b.burstKiosks} kiosks, ${b.burstDisplays} displays`,
      shape,
    );
    row(
      "Burst: every KOT prints once",
      `${t.kots} of ${t.orders}`,
      "once each",
      t.kot_once_each === true && t.kots === t.orders,
    );
    row(
      "Burst: both boards match the Hub",
      t.boards_match ? "yes" : "no",
      "yes",
      t.boards_match === true,
    );
    if (isNum(t.kot_p95_ms)) {
      row(
        "Burst: KOT latency, 95th percentile",
        `${t.kot_p95_ms} ms`,
        `<= ${b.p95Ms} ms`,
        t.kot_p95_ms <= b.p95Ms,
      );
    } else {
      missing("Burst: KOT latency, 95th percentile", "needs burst.json");
    }
  } else {
    missing("Burst", "needs burst.json");
  }

  const { soakUp, soakDown } = results;
  for (const [label, s] of [
    ["WAN up", soakUp],
    ["WAN down", soakDown],
  ]) {
    if (!s) {
      missing(`Soak ${label}`, `needs soak-wan-${label === "WAN up" ? "up" : "down"}.json`);
      continue;
    }
    row(
      `Soak ${label}: orders`,
      `${s.orders}`,
      `>= ${b.soakOrders}`,
      isNum(s.orders) && s.orders >= b.soakOrders,
    );
    const lost = s.lost === 0 && s.served === s.orders && s.advances === s.orders * 3;
    row(`Soak ${label}: ticket, advance or KOT lost`, lost ? "none" : "lost", "none", lost);
    const once = s.kots === s.orders && s.duplicate_kots === 0;
    row(
      `Soak ${label}: every KOT printed once`,
      `${s.kots} (${s.duplicate_kots} twice)`,
      "once each",
      once,
    );
  }
  if (soakUp && soakDown) {
    const same = outcome(soakUp) === outcome(soakDown);
    row("Soak: WAN down differs from WAN up", same ? "no" : "yes", "no", same);
  }

  for (const [key, name] of BROWSERS) {
    const r = tally(results.browsers[key]);
    if (!r) {
      missing(`Kitchen display on ${name}`, `needs browser-${key}.json`);
      continue;
    }
    row(
      `Kitchen display on ${name}`,
      `${r.passed} passed, ${r.failed} failed`,
      "0 failed",
      r.failed === 0 && r.passed > 0,
    );
  }
  return rows;
}

/** The release report as markdown. */
export function render(rows, date = new Date().toISOString().slice(0, 10)) {
  return [
    `# Kitchen flow on the reference rig (${date})`,
    "",
    "| Measure | Result | Budget | |",
    "| --- | --- | --- | --- |",
    ...rows.map((r) => `| ${r.what} | ${r.value} | ${r.budget} | ${r.ok ? "pass" : "FAIL"} |`),
    "",
  ].join("\n");
}

/** The rows of the browser table in `md`, as arrays of cells. */
export function compatRows(md) {
  const lines = md.split("\n");
  const at = lines.indexOf(COMPAT_HEAD);
  if (at < 0) return [];
  const rows = [];
  for (const line of lines.slice(at + 2)) {
    if (!line.startsWith("|")) break;
    rows.push(
      line
        .split("|")
        .slice(1, -1)
        .map((c) => c.trim()),
    );
  }
  return rows;
}

/** The compatibility list with this run's browsers merged in: a repeated browser replaces its row. */
export function renderCompat(existing, results, date) {
  const rows = new Map(compatRows(existing).map((r) => [r[0], r]));
  for (const [key, name] of BROWSERS) {
    const r = tally(results.browsers[key]);
    if (r) rows.set(name, [name, `${r.passed}`, `${r.failed}`, date]);
  }
  const order = BROWSERS.map(([, name]) => name);
  const sorted = [...rows.values()].sort((a, b) => order.indexOf(a[0]) - order.indexOf(b[0]));
  return [
    "# Kitchen display compatibility",
    "",
    "Written by the release bench (`.github/workflows/release-bench.yml`, Story 7.11). The Kitchen",
    "display's suite (board, chime, mute, Undo and LAN loss) runs on Chrome on Android and on Safari",
    "on iPad; a browser passes when no test of its run failed.",
    "",
    COMPAT_HEAD,
    "| --- | --- | --- | --- |",
    ...sorted.map((r) => `| ${r.join(" | ")} |`),
    "",
  ].join("\n");
}

export function main(argv, env = process.env) {
  const flag = (name, fallback) => {
    const i = argv.indexOf(name);
    return i >= 0 && argv[i + 1] !== undefined ? argv[i + 1] : fallback;
  };
  const dir = flag("--dir", "target/kitchen");
  const out = flag("--out", join(dir, "report.md"));
  const compat = flag("--compat", COMPAT);
  const date = flag("--date", new Date().toISOString().slice(0, 10));
  const results = load(dir);
  const rows = evaluate(results, budgets(env));
  mkdirSync(dirname(out), { recursive: true });
  writeFileSync(out, render(rows, date));
  if (BROWSERS.some(([key]) => tally(results.browsers[key]))) {
    const existing = existsSync(compat) ? readFileSync(compat, "utf8") : "";
    mkdirSync(dirname(compat), { recursive: true });
    writeFileSync(compat, renderCompat(existing, results, date));
  }
  const bad = rows.filter((r) => !r.ok);
  for (const r of bad) console.error(`kitchen failed: ${r.what}: ${r.value} (${r.budget})`);
  console.log(`kitchen report: ${rows.length - bad.length}/${rows.length} pass, written to ${out}`);
  return bad.length ? 1 : 0;
}

if (process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exit(main(process.argv.slice(2)));
}
