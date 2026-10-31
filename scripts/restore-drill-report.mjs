#!/usr/bin/env node
// Story 9.27: merges the results of the restore drill and the privacy proof of one release run into
// a report and fails the release when a result is missing or a check is missed.
//
//   node scripts/restore-drill-report.mjs [--dir target/restore-drill] [--out <dir>/report.md]
//                                         [--date YYYY-MM-DD]
//
// `--dir` holds what the `restore-drill` job of release-bench.yml wrote (a file absent from disk is
// a failure, never a pass):
//   restore.json       arena-replay `--lib drill`: the seeded branch (days, changes, fork) restored
//                      from an export and from the cloud backup: restores[{from, epoch_above_source,
//                      rows_equal, erased_players, erased_without_key, erased_pii_empty,
//                      living_readable, orphaned}]
//   late-erasure.json  a player erased after the export, restored on a paid Hub with the cloud
//                      reachable: erasures_reapplied, keys_left, offline_erasures_reapplied
//   promote.json       a promote after an erasure: erased_players, erased_without_key,
//                      erasures_reapplied
//   privacy.json       the scan of every artefact: artefacts, kinds[], seeded_values, findings,
//                      log_bytes, log_player_ids
//   export-bench.json  arena-store `export_one_gigabyte_benchmark`: database_mib, took_ms, file_mib
// Budgets are the story's numbers; RESTORE_EXPORT_MS and RESTORE_DATABASE_MIB override them for a
// trial.

import { existsSync, mkdirSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const num = (v, fallback) =>
  v !== undefined && v !== "" && Number.isFinite(Number(v)) ? Number(v) : fallback;
const isNum = (v) => typeof v === "number" && Number.isFinite(v);

/** The artefacts the privacy scan must have covered. */
export const REQUIRED_KINDS = [
  "audit cache",
  "batch",
  "database",
  "export",
  "GST archive",
  "key backup",
  "snapshot",
];

/** The story's budgets, with the environment's overrides. */
export function budgets(env = process.env) {
  return {
    exportMs: num(env.RESTORE_EXPORT_MS, 10_000),
    databaseMib: num(env.RESTORE_DATABASE_MIB, 1024),
    days: 30,
    erased: 2,
    kinds: REQUIRED_KINDS,
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
    restore: read("restore.json"),
    lateErasure: read("late-erasure.json"),
    promote: read("promote.json"),
    privacy: read("privacy.json"),
    exportBench: read("export-bench.json"),
  };
}

const yes = (v) => v === true;

/**
 * The report's rows: {what, value, budget, ok}. A result that is absent is a failed row.
 */
export function evaluate(results, b = budgets()) {
  const rows = [];
  const row = (what, value, budget, ok) => rows.push({ what, value, budget, ok });
  const missing = (what, file) => row(what, "missing", `needs ${file}`, false);

  const r = results.restore;
  for (const [from, label] of [
    ["export", "Restore from a local export on a new PC"],
    ["cloud", "Restore from the cloud backup on a new PC"],
  ]) {
    const x = Array.isArray(r?.restores) ? r.restores.find((e) => e?.from === from) : undefined;
    if (!x) {
      missing(label, "restore.json");
      continue;
    }
    const erasedOk =
      isNum(x.erased_players) &&
      x.erased_players >= b.erased &&
      x.erased_without_key === x.erased_players &&
      x.erased_pii_empty === x.erased_players &&
      isNum(x.living_readable) &&
      x.living_readable > 0;
    row(
      label,
      `${yes(x.epoch_above_source) ? "new epoch" : "no new epoch"}, rows ${yes(x.rows_equal) ? "equal" : "DIFFER"}, ${x.erased_without_key}/${x.erased_players} erased without key`,
      "new epoch, rows equal, 2 erased players without key or PII",
      yes(x.epoch_above_source) && yes(x.rows_equal) && erasedOk,
    );
  }
  if (r) {
    row(
      "Drill branch: days of activity, fork",
      `${r.days} days, ${r.changes} changes, ${yes(r.fork) ? "fork" : "no fork"}`,
      `>= ${b.days} days and a fork`,
      isNum(r.days) && r.days >= b.days && isNum(r.changes) && r.changes > 0 && yes(r.fork),
    );
    const c = r.restores?.find((e) => e?.from === "cloud");
    if (c)
      row(
        "Cloud restore: orphaned rows kept out of state",
        `${c.orphaned} orphaned`,
        "> 0 (the fork was crossed)",
        isNum(c.orphaned) && c.orphaned > 0,
      );
  }

  const e = results.lateErasure;
  if (e)
    row(
      "Player erased after the export, restored with the cloud reachable",
      `${e.erasures_reapplied} erased again, ${e.keys_left} keys left; offline ${e.offline_erasures_reapplied} erased`,
      "1 erased again; offline 0",
      e.erasures_reapplied === 1 && e.offline_erasures_reapplied === 0,
    );
  else missing("Player erased after the export, restored with the cloud reachable", "late-erasure.json");

  const p = results.promote;
  if (p)
    row(
      "Promote after an erasure",
      `${p.erased_without_key}/${p.erased_players} erased without key, ${p.erasures_reapplied} re-applied`,
      "every erased player without key",
      isNum(p.erased_players) &&
        p.erased_players >= b.erased &&
        p.erased_without_key === p.erased_players &&
        p.erasures_reapplied >= p.erased_players,
    );
  else missing("Promote after an erasure", "promote.json");

  const s = results.privacy;
  if (s) {
    const kinds = Array.isArray(s.kinds) ? s.kinds : [];
    const absent = b.kinds.filter((k) => !kinds.includes(k));
    row(
      "Privacy scan covers every artefact",
      `${s.artefacts} files; ${absent.length ? `missing ${absent.join(", ")}` : "all kinds"}`,
      b.kinds.join(", "),
      absent.length === 0 && isNum(s.artefacts) && s.artefacts >= b.kinds.length,
    );
    row(
      "Seeded names, phones and emails found in plaintext",
      `${s.findings} of ${s.seeded_values} values`,
      "0",
      s.findings === 0 && isNum(s.seeded_values) && s.seeded_values > 0,
    );
    row(
      "Logs carry player ids only",
      `${s.log_player_ids} ids in ${s.log_bytes} bytes of log`,
      "ids present, no seeded value",
      isNum(s.log_bytes) && s.log_bytes > 0 && s.log_player_ids > 0 && s.findings === 0,
    );
  } else missing("Privacy scan", "privacy.json");

  const x = results.exportBench;
  if (isNum(x?.took_ms) && isNum(x?.database_mib))
    row(
      `Export of the ${x.database_mib} MiB reference database`,
      `${x.took_ms} ms`,
      `< ${b.exportMs} ms on >= ${b.databaseMib} MiB`,
      x.took_ms < b.exportMs && x.database_mib >= b.databaseMib,
    );
  else missing("Export of the 1 GB reference database", "export-bench.json");
  return rows;
}

/** The release report as markdown. */
export function render(rows, date = new Date().toISOString().slice(0, 10)) {
  const mark = (r) => (r.ok ? "pass" : "FAIL");
  return [
    `# Restore drill and privacy proof (${date})`,
    "",
    "| Check | Result | Budget | |",
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
  const dir = flag("--dir", "target/restore-drill");
  const out = flag("--out", join(dir, "report.md"));
  const date = flag("--date", new Date().toISOString().slice(0, 10));
  const rows = evaluate(load(dir), budgets(env));
  mkdirSync(dirname(out), { recursive: true });
  writeFileSync(out, render(rows, date));
  const bad = rows.filter((r) => !r.ok);
  for (const r of bad) console.error(`restore drill failed: ${r.what}: ${r.value} (${r.budget})`);
  console.log(`restore drill report: ${rows.length - bad.length}/${rows.length} pass, written to ${out}`);
  return bad.length ? 1 : 0;
}

if (process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exit(main(process.argv.slice(2)));
}
