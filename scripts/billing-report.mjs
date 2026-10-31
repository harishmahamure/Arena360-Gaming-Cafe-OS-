#!/usr/bin/env node
// Story 10.20: merges the billing and Hub-move proofs of one release run into a report and fails
// the release when a proof failed or its result is missing.
//
//   node scripts/billing-report.mjs [--dir target/billing] [--out target/billing/report.md]
//                                   [--date YYYY-MM-DD] [--strict]
//
// `--dir` holds what the tests wrote when `ARENA_PROOF_OUT` was set (a file is written only when
// its test reached the end, so a file absent from disk is a failure, never a pass):
//   billing-walk.json    arena-cloud `--test billing_events each_band_walks`: bands[], steps[]
//   billing-cancel.json  `each_band_cancels...`: bands[]
//   billing-refund.json  `each_band_refunds...`: bands[]
//   downgrade.json       arena-core `plan_change_proof`: sessions, sessions_ended, tickets_open,
//                        devices, over_cap, ads_before_exp, ads_at_exp
//   move-lan.json        arena-api `--test handover a_free_tablet...`: history_days, sessions,
//                        kiosks, last_seq, checksums_equal, sessions_continue, follow_ms,
//                        budget_ms, successor_pushed
//   move-file.json       the same café moved by file (no successor is pushed)
// Only a run against Razorpay test mode on staging can write
//   staging.json         subscriptions[{plan, method, charged, refunded}], upi_offered_for_premium_yearly
// Without it the report lists the nightly staging run as "not run" and passes; `--strict` (the
// nightly job) makes it required like the rest.
// Budgets are the story's numbers; BILLING_FOLLOW_MS overrides the 30 s kiosk budget for a trial.

import { existsSync, mkdirSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const num = (v, fallback) => (v !== undefined && Number.isFinite(Number(v)) ? Number(v) : fallback);
const isNum = (v) => typeof v === "number" && Number.isFinite(v);

export const BANDS = ["IN", "MY", "SG", "US"];
export const STEPS = [
  "create",
  "activate",
  "upgrade",
  "cycle",
  "approval",
  "grace",
  "recovery",
  "lapse",
];

/** The story's budgets, with the environment's overrides. */
export function budgets(env = process.env) {
  return {
    followMs: num(env.BILLING_FOLLOW_MS, 30_000),
    sessions: 10,
    tickets: 2,
    moveSessions: 5,
    moveKiosks: 20,
    moveDays: 30,
  };
}

const FILES = {
  walk: "billing-walk.json",
  cancel: "billing-cancel.json",
  refund: "billing-refund.json",
  downgrade: "downgrade.json",
  moveLan: "move-lan.json",
  moveFile: "move-file.json",
  staging: "staging.json",
};

/** Reads every result file of `dir`; a file that is absent or not JSON is left out. */
export function load(dir) {
  const out = {};
  for (const [key, name] of Object.entries(FILES)) {
    const file = join(dir, name);
    if (!existsSync(file)) continue;
    try {
      out[key] = JSON.parse(readFileSync(file, "utf8"));
    } catch {
      /* a damaged file counts as missing */
    }
  }
  return out;
}

const hasAll = (have, want) => Array.isArray(have) && want.every((x) => have.includes(x));

/**
 * The report's rows: {what, value, budget, ok}. A result that is absent is a failed row, except
 * the staging run, which only the nightly job can write and which is "not run" unless `strict`.
 */
export function evaluate(results, b = budgets(), strict = false) {
  const rows = [];
  const row = (what, value, budget, ok) => rows.push({ what, value, budget, ok });
  const missing = (what, file) =>
    rows.push({ what, value: "missing", budget: `needs ${file}`, ok: false });

  const bandRow = (what, r, file, extra = () => true) => {
    if (!r) return missing(what, file);
    row(
      what,
      `${(r.bands ?? []).join(", ") || "none"}`,
      `all of ${BANDS.join(", ")}`,
      hasAll(r.bands, BANDS) && extra(r),
    );
  };
  bandRow(
    "Billing walk: create, activate, upgrade, cycle switch, approval, grace, recovery, lapse",
    results.walk,
    FILES.walk,
    (r) => hasAll(r.steps, STEPS),
  );
  bandRow("Billing: cancel, period end, Free", results.cancel, FILES.cancel);
  bandRow("Billing: 7-day refund and credit note", results.refund, FILES.refund);

  const d = results.downgrade;
  if (d) {
    row(
      `Premium to Basic to Free: ${d.sessions} sessions, ${d.tickets_open} kitchen tickets`,
      `${d.sessions_ended} ended, ${d.over_cap} over cap`,
      `${b.sessions} sessions, ${b.tickets} tickets, 0 ended`,
      d.sessions >= b.sessions &&
        d.tickets_open >= b.tickets &&
        d.sessions_ended === 0 &&
        d.over_cap > 0,
    );
    row(
      "Ads on the Android Hub only at the paid licence's exp",
      `before exp: ${d.ads_before_exp ? "ads" : "none"}, at exp: ${d.ads_at_exp ? "ads" : "none"}`,
      "none, ads",
      d.ads_before_exp === false && d.ads_at_exp === true,
    );
  } else {
    missing("Premium to Basic to Free keeps every session and ticket", FILES.downgrade);
    missing("Ads on the Android Hub only at the paid licence's exp", FILES.downgrade);
  }

  const move = (what, m, file, pushed) => {
    if (!m) return missing(what, file);
    row(
      `${what}: checksums and sessions`,
      `final seq ${m.last_seq}`,
      "equal, every session continues",
      m.checksums_equal === true && m.sessions_continue === true && isNum(m.last_seq),
    );
    row(
      `${what}: ${m.sessions} sessions, ${m.history_days} days of history`,
      `${m.sessions} sessions, ${m.history_days} days`,
      `>= ${b.moveSessions} sessions, >= ${b.moveDays} days`,
      m.sessions >= b.moveSessions && m.history_days >= b.moveDays,
    );
    row(
      `${what}: ${m.kiosks} kiosks reach the new Hub with their old token`,
      `${m.follow_ms} ms`,
      `< ${b.followMs} ms of ${b.moveKiosks} kiosks`,
      isNum(m.follow_ms) &&
        m.follow_ms < b.followMs &&
        m.kiosks >= b.moveKiosks &&
        m.successor_pushed === pushed,
    );
  };
  move("Hub move over the LAN", results.moveLan, FILES.moveLan, true);
  move("Hub move by file", results.moveFile, FILES.moveFile, false);

  const s = results.staging;
  if (s) {
    const subs = s.subscriptions ?? [];
    row(
      "Staging (Razorpay test mode): Basic monthly UPI Autopay, Premium yearly card and eNACH",
      `${subs.length} created, charged and refunded`,
      "3, all charged and refunded",
      subs.length >= 3 && subs.every((x) => x.charged === true && x.refunded === true),
    );
    row(
      "Staging: UPI never offered for Premium yearly",
      s.upi_offered_for_premium_yearly ? "offered" : "not offered",
      "not offered",
      s.upi_offered_for_premium_yearly === false,
    );
  } else {
    rows.push({
      what: "Nightly run on Razorpay test mode (staging)",
      value: "not run",
      budget: `needs staging (${FILES.staging})`,
      ok: !strict,
      pending: true,
    });
  }
  return rows;
}

/** The release report as markdown. */
export function render(rows, date = new Date().toISOString().slice(0, 10)) {
  const mark = (r) => (r.pending ? "not run" : r.ok ? "pass" : "FAIL");
  return [
    `# Billing, tier changes and the Hub move, proven end to end (${date})`,
    "",
    "| Proof | Result | Budget | |",
    "| --- | --- | --- | --- |",
    ...rows.map((r) => `| ${r.what} | ${r.value} | ${r.budget} | ${mark(r)} |`),
    "",
  ].join("\n");
}

export function main(argv, env = process.env) {
  const flag = (name, fallback) => {
    const i = argv.indexOf(name);
    return i >= 0 && argv[i + 1] !== undefined ? argv[i + 1] : fallback;
  };
  const dir = flag("--dir", "target/billing");
  const out = flag("--out", join(dir, "report.md"));
  const date = flag("--date", new Date().toISOString().slice(0, 10));
  const rows = evaluate(load(dir), budgets(env), argv.includes("--strict"));
  mkdirSync(dirname(out), { recursive: true });
  writeFileSync(out, render(rows, date));
  const bad = rows.filter((r) => !r.ok);
  for (const r of bad) console.error(`billing failed: ${r.what}: ${r.value} (${r.budget})`);
  console.log(`billing report: ${rows.length - bad.length}/${rows.length} pass, written to ${out}`);
  return bad.length ? 1 : 0;
}

if (process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exit(main(process.argv.slice(2)));
}
