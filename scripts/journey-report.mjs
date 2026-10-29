#!/usr/bin/env node
// Story 6.38: merges the player-journey timings of one release run into a report and fails the
// release when a budget is missed or a measurement is missing.
//
//   node scripts/journey-report.mjs [--dir target/journey] [--out target/journey/report.md]
//
// `--dir` holds what the jobs of release-bench.yml wrote (a file absent from disk is a failure,
// never a pass):
//   hub.json           the Hub's legs over the LAN (arena-api lan.rs): login_ms, approve_ms, extend_ms, order_ms
//   ui.json            the kiosk UI's legs (apps/kiosk journey.spec.ts): login_ms, shell_ms, extend_ms, returned_to_attract
//   idle-attract.json  scripts/bench/kiosk-idle.ps1 -Phase attract: {phase, mb}
//   idle-shell.json    scripts/bench/kiosk-idle.ps1 -Phase shell:  {phase, mb}
//   hub-down.json      the real-clock Hub outage (arena-kiosk-svc): down_secs, locked_late_ms, locked_early
//   residue*.json      scripts/launcher-residue.mjs, one per Windows version: {os, sessions, residue}
// Budgets are the story's numbers; JOURNEY_LOGIN_MS, JOURNEY_APPROVE_MS, JOURNEY_EXTEND_MS,
// JOURNEY_ORDER_MS and JOURNEY_IDLE_MB override them for a trial run, and JOURNEY_DOWN_SECS
// lowers the outage length the report accepts (30 minutes otherwise).

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

const num = (v, fallback) => (v !== undefined && Number.isFinite(Number(v)) ? Number(v) : fallback);
const isNum = (v) => typeof v === "number" && Number.isFinite(v);

/** The story's budgets, with the environment's overrides. */
export function budgets(env = process.env) {
  return {
    loginMs: num(env.JOURNEY_LOGIN_MS, 3000),
    approveMs: num(env.JOURNEY_APPROVE_MS, 2000),
    extendMs: num(env.JOURNEY_EXTEND_MS, 2000),
    orderMs: num(env.JOURNEY_ORDER_MS, 3000),
    idleMb: num(env.JOURNEY_IDLE_MB, 80),
    downSecs: num(env.JOURNEY_DOWN_SECS, 30 * 60),
    lockLateMs: 1000,
    residueSessions: 20,
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
  const residue = existsSync(dir)
    ? readdirSync(dir)
        .filter((f) => /^residue.*\.json$/.test(f))
        .sort()
        .map(read)
        .filter(Boolean)
    : [];
  return {
    hub: read("hub.json"),
    ui: read("ui.json"),
    attract: read("idle-attract.json"),
    shell: read("idle-shell.json"),
    down: read("hub-down.json"),
    residue,
  };
}

/** The report's rows: {what, value, budget, ok}; a measurement that is absent is a failed row. */
export function evaluate(results, b = budgets()) {
  const rows = [];
  const row = (what, value, budget, ok) => rows.push({ what, value, budget, ok });
  const missing = (what, why) => rows.push({ what, value: "missing", budget: why, ok: false });

  const { hub, ui } = results;
  const both = (what, budget, hubLeg, uiLeg, within) => {
    if (!isNum(hubLeg) || !isNum(uiLeg)) return missing(what, "needs hub.json and ui.json");
    const total = hubLeg + uiLeg;
    return row(what, `${total} ms`, `${budget} ms`, within(total));
  };
  both("Login", b.loginMs, hub?.login_ms, ui?.login_ms, (t) => t < b.loginMs);
  both(
    "Shell opens after approval",
    b.approveMs,
    hub?.approve_ms,
    ui?.shell_ms,
    (t) => t < b.approveMs,
  );
  both(
    "Extend reaches the floor",
    b.extendMs,
    hub?.extend_ms,
    ui?.extend_ms,
    (t) => t <= b.extendMs,
  );
  if (isNum(hub?.order_ms)) {
    row(
      "Order on the Hub tile",
      `${hub.order_ms} ms`,
      `${b.orderMs} ms`,
      hub.order_ms <= b.orderMs,
    );
  } else {
    missing("Order on the Hub tile", "needs hub.json");
  }
  if (ui) {
    const back = ui.returned_to_attract === true;
    row("PC returns to attract after clean-up", back ? "yes" : "no", "yes", back);
  } else {
    missing("PC returns to attract after clean-up", "needs ui.json");
  }

  for (const [what, r] of [
    ["Kiosk memory, idle on attract", results.attract],
    ["Kiosk memory, idle in the shell", results.shell],
  ]) {
    if (isNum(r?.mb)) row(what, `${r.mb} MB`, `< ${b.idleMb} MB`, r.mb < b.idleMb);
    else missing(what, "needs the idle job");
  }

  const d = results.down;
  if (isNum(d?.down_secs)) {
    row("Hub outage length", `${d.down_secs} s`, `>= ${b.downSecs} s`, d.down_secs >= b.downSecs);
    row(
      "Locked before zero during the outage",
      d.locked_early ? "yes" : "no",
      "no",
      d.locked_early === false,
    );
    const late = isNum(d.locked_late_ms) && d.locked_late_ms <= b.lockLateMs;
    row("Lock lands after zero by", `${d.locked_late_ms} ms`, `<= ${b.lockLateMs} ms`, late);
  } else {
    missing("Hub outage", "needs hub-down.json");
  }

  if (!results.residue.length) {
    missing("Launcher logout residue", "needs a residue*.json per Windows version");
  }
  for (const r of results.residue) {
    const os = r.os ?? "Windows";
    row(
      `${os}: sessions run`,
      `${r.sessions}`,
      `>= ${b.residueSessions}`,
      isNum(r.sessions) && r.sessions >= b.residueSessions,
    );
    const left = Array.isArray(r.residue) ? r.residue.length : null;
    row(
      `${os}: residue after a session`,
      left === null ? "unreadable" : `${left}`,
      "0",
      left === 0,
    );
  }
  return rows;
}

/** The release report as markdown. */
export function render(rows, date = new Date().toISOString().slice(0, 10)) {
  return [
    `# Player journey on the reference rig (${date})`,
    "",
    "| Measure | Result | Budget | |",
    "| --- | --- | --- | --- |",
    ...rows.map((r) => `| ${r.what} | ${r.value} | ${r.budget} | ${r.ok ? "pass" : "FAIL"} |`),
    "",
  ].join("\n");
}

export function main(argv, env = process.env) {
  const flag = (name, fallback) => {
    const i = argv.indexOf(name);
    return i >= 0 && argv[i + 1] !== undefined ? argv[i + 1] : fallback;
  };
  const dir = flag("--dir", "target/journey");
  const out = flag("--out", join(dir, "report.md"));
  const rows = evaluate(load(dir), budgets(env));
  mkdirSync(dirname(out), { recursive: true });
  writeFileSync(out, render(rows));
  const bad = rows.filter((r) => !r.ok);
  for (const r of bad) console.error(`journey failed: ${r.what}: ${r.value} (${r.budget})`);
  console.log(`journey report: ${rows.length - bad.length}/${rows.length} pass, written to ${out}`);
  return bad.length ? 1 : 0;
}

if (process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exit(main(process.argv.slice(2)));
}
