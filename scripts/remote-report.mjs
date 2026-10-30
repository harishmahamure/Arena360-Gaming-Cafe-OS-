#!/usr/bin/env node
// Story 8.42: merges the owner's remote-experience results of one release run into a report and
// fails the release when a budget is missed or a measurement is missing.
//
//   node scripts/remote-report.mjs [--dir target/remote] [--out target/remote/report.md]
//                                  [--date YYYY-MM-DD] [--strict]
//
// `--dir` holds what the jobs of release-bench.yml wrote (a file absent from disk is a failure,
// never a pass):
//   today.json         arena-cloud `--test remote_experience` (Today's data age on Basic and
//                      Premium over scripted opens): tiers[{tier, opens, cadence_s,
//                      within_cadence_pct, server_read_p95_ms}], budget_within_pct
//   first-render.json  the Playwright JSON report of apps/owner `reference-phone` (the
//                      `first-render-ms` annotation of its test)
//   commands.json      owner commands, post to applied: commands, p95_ms, max_ms
//   catch_up.json      the cloud's side of the catch-up after an outage: outage_hours, rows,
//                      elapsed_ms, budget_ms, gaps_found, duplicate_objects
//   cost.json          a month of R2 PUTs per branch: tiers[{tier, puts_month, cost_paise}],
//                      average_paise, budget_paise
//   audit.json         a busy month's first audit results: rows, first_results_ms, budget_ms
// Only a run on the rig (a Hub with 20 kiosks through a real eight-hour WAN outage, and the cloud
// stopped for an hour) can write
//   soak.json          outage_hours, kiosks, sessions, orders, catch_up_ms, gaps, duplicate_sends
//   cloud-down.json    hours, changes_beyond_offline_chip, caught_up
// Without them the report lists the two as "not run" and passes; `--strict` (the soak run of
// release-bench.yml) makes them required like the rest.
// Budgets are the story's numbers; REMOTE_FIRST_RENDER_MS, REMOTE_WITHIN_PCT, REMOTE_COMMAND_MS,
// REMOTE_CATCH_UP_MS, REMOTE_COST_PAISE and REMOTE_AUDIT_MS override them for a trial.

import { existsSync, mkdirSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const num = (v, fallback) => (v !== undefined && Number.isFinite(Number(v)) ? Number(v) : fallback);
const isNum = (v) => typeof v === "number" && Number.isFinite(v);

/** The story's budgets, with the environment's overrides. */
export function budgets(env = process.env) {
  return {
    firstRenderMs: num(env.REMOTE_FIRST_RENDER_MS, 2000),
    withinPct: num(env.REMOTE_WITHIN_PCT, 99),
    opens: num(env.REMOTE_OPENS, 1000),
    commandMs: num(env.REMOTE_COMMAND_MS, 120_000),
    commands: num(env.REMOTE_COMMANDS, 200),
    catchUpMs: num(env.REMOTE_CATCH_UP_MS, 600_000),
    outageHours: num(env.REMOTE_OUTAGE_HOURS, 8),
    costPaise: num(env.REMOTE_COST_PAISE, 1000),
    auditMs: num(env.REMOTE_AUDIT_MS, 10_000),
    kiosks: 20,
    sessions: 1000,
    orders: 500,
    cloudDownHours: 1,
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
    today: read("today.json"),
    firstRender: read("first-render.json"),
    commands: read("commands.json"),
    catchUp: read("catch_up.json"),
    cost: read("cost.json"),
    audit: read("audit.json"),
    soak: read("soak.json"),
    cloudDown: read("cloud-down.json"),
  };
}

/** The `first-render-ms` annotation of a Playwright JSON report, or undefined. */
export function firstRenderMs(report) {
  const found = [];
  const walk = (suite) => {
    for (const spec of suite.specs ?? []) {
      for (const t of spec.tests ?? []) {
        for (const a of t.annotations ?? []) {
          if (a.type === "first-render-ms") found.push(Number(a.description));
        }
      }
    }
    for (const s of suite.suites ?? []) walk(s);
  };
  for (const s of report?.suites ?? []) walk(s);
  return found.length && found.every(Number.isFinite) ? Math.max(...found) : undefined;
}

const rupees = (paise) => `Rs ${(paise / 100).toFixed(2)}`;

/**
 * The report's rows: {what, value, budget, ok}. A measurement that is absent is a failed row,
 * except the two that only the rig can write, which are "not run" unless `strict`.
 */
export function evaluate(results, b = budgets(), strict = false) {
  const rows = [];
  const row = (what, value, budget, ok) => rows.push({ what, value, budget, ok });
  const missing = (what, why) => rows.push({ what, value: "missing", budget: why, ok: false });
  const pending = (what, why) =>
    rows.push({ what, value: "not run", budget: why, ok: !strict, pending: true });

  const ms = firstRenderMs(results.firstRender);
  if (isNum(ms))
    row(
      "Today first render, reference phone",
      `${ms} ms`,
      `< ${b.firstRenderMs} ms`,
      ms < b.firstRenderMs,
    );
  else missing("Today first render, reference phone", "needs first-render.json");

  const t = results.today;
  if (t && Array.isArray(t.tiers) && t.tiers.length) {
    for (const x of t.tiers) {
      const what = `Today data age within cadence, ${x.tier} (${x.cadence_s} s)`;
      row(
        what,
        `${x.within_cadence_pct}% of ${x.opens} opens`,
        `>= ${b.withinPct}% of ${b.opens}`,
        x.within_cadence_pct >= b.withinPct && x.opens >= b.opens,
      );
    }
    for (const tier of ["basic", "premium"]) {
      if (!t.tiers.some((x) => x.tier === tier))
        missing(`Today data age within cadence, ${tier}`, "needs today.json");
    }
  } else {
    missing("Today data age within cadence", "needs today.json");
  }

  const c = results.commands;
  if (isNum(c?.p95_ms))
    row(
      `Owner command post to applied, p95 of ${c.commands}`,
      `${c.p95_ms} ms`,
      `< ${b.commandMs} ms of ${b.commands}`,
      c.p95_ms < b.commandMs && c.commands >= b.commands,
    );
  else missing("Owner command post to applied", "needs commands.json");

  const u = results.catchUp;
  if (isNum(u?.elapsed_ms)) {
    row(
      `Catch-up after a ${u.outage_hours} h outage, cloud side`,
      `${u.elapsed_ms} ms`,
      `<= ${b.catchUpMs} ms after ${b.outageHours} h`,
      u.elapsed_ms <= b.catchUpMs && u.outage_hours >= b.outageHours,
    );
    row(
      "Catch-up: gaps and duplicates",
      `${u.gaps_found} gaps, ${u.duplicate_objects} duplicates`,
      "0, 0",
      u.gaps_found === 0 && u.duplicate_objects === 0,
    );
  } else missing("Catch-up after an outage, cloud side", "needs catch_up.json");

  const s = results.soak;
  if (s) {
    row(
      `Hub soak: ${s.kiosks} kiosks, ${s.sessions} sessions, ${s.orders} orders over ${s.outage_hours} h`,
      `${s.catch_up_ms} ms to catch up`,
      `<= ${b.catchUpMs} ms`,
      isNum(s.catch_up_ms) &&
        s.catch_up_ms <= b.catchUpMs &&
        s.kiosks >= b.kiosks &&
        s.sessions >= b.sessions &&
        s.orders >= b.orders &&
        s.outage_hours >= b.outageHours,
    );
    row(
      "Hub soak: gaps and duplicate sends",
      `${s.gaps} gaps, ${s.duplicate_sends} duplicates`,
      "0, 0",
      s.gaps === 0 && s.duplicate_sends === 0,
    );
  } else
    pending("Hub soak with 20 kiosks through an eight-hour outage", "needs the rig (soak.json)");

  const d = results.cloudDown;
  if (d)
    row(
      `Cloud stopped for ${d.hours} h`,
      `${d.changes_beyond_offline_chip} changes, ${d.caught_up ? "caught up" : "not caught up"}`,
      "0 changes, caught up",
      d.hours >= b.cloudDownHours && d.changes_beyond_offline_chip === 0 && d.caught_up === true,
    );
  else
    pending("Cloud stopped for an hour while the branch trades", "needs the rig (cloud-down.json)");

  const k = results.cost;
  if (isNum(k?.average_paise)) {
    for (const x of k.tiers ?? [])
      row(
        `R2 PUTs per branch per month, ${x.tier}`,
        `${x.puts_month} PUTs, ${rupees(x.cost_paise)}`,
        "published",
        true,
      );
    row(
      "R2 cost per branch per month, average",
      rupees(k.average_paise),
      `< ${rupees(b.costPaise)}`,
      k.average_paise < b.costPaise,
    );
  } else missing("R2 cost per branch per month", "needs cost.json");

  const a = results.audit;
  if (isNum(a?.first_results_ms))
    row(
      `Audit first results, busy month of ${a.rows} changes`,
      `${a.first_results_ms} ms`,
      `< ${b.auditMs} ms`,
      a.first_results_ms < b.auditMs,
    );
  else missing("Audit first results", "needs audit.json");
  return rows;
}

/** The release report as markdown. */
export function render(rows, date = new Date().toISOString().slice(0, 10)) {
  const mark = (r) => (r.pending ? "not run" : r.ok ? "pass" : "FAIL");
  return [
    `# The owner's remote experience against its targets (${date})`,
    "",
    "| Measure | Result | Budget | |",
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
  const dir = flag("--dir", "target/remote");
  const out = flag("--out", join(dir, "report.md"));
  const date = flag("--date", new Date().toISOString().slice(0, 10));
  const rows = evaluate(load(dir), budgets(env), argv.includes("--strict"));
  mkdirSync(dirname(out), { recursive: true });
  writeFileSync(out, render(rows, date));
  const bad = rows.filter((r) => !r.ok);
  for (const r of bad) console.error(`remote failed: ${r.what}: ${r.value} (${r.budget})`);
  console.log(`remote report: ${rows.length - bad.length}/${rows.length} pass, written to ${out}`);
  return bad.length ? 1 : 0;
}

if (process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exit(main(process.argv.slice(2)));
}
