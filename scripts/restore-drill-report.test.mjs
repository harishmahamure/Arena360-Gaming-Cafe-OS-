import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { REQUIRED_KINDS, budgets, evaluate, load, main, render } from "./restore-drill-report.mjs";

const restored = (from, over = {}) => ({
  from,
  new_epoch: "2:01JNEWPC",
  epoch_above_source: true,
  rows_equal: true,
  erased_players: 2,
  erased_without_key: 2,
  erased_pii_empty: 2,
  living_readable: 4,
  orphaned: from === "cloud" ? 2 : 0,
  ...over,
});

const good = () => ({
  restore: {
    days: 30,
    changes: 140,
    fork: true,
    restores: [restored("export"), restored("cloud")],
  },
  lateErasure: {
    erasures_reapplied: 1,
    keys_left: 3,
    offline_erasures_reapplied: 0,
    offline_keys_left: 4,
  },
  promote: { erased_players: 2, erased_without_key: 2, erasures_reapplied: 2 },
  privacy: {
    artefacts: 16,
    kinds: [...REQUIRED_KINDS],
    seeded_values: 24,
    findings: 0,
    log_bytes: 900,
    log_player_ids: 6,
  },
  exportBench: { database_mib: 1024, took_ms: 4200, file_mib: 1024 },
});

const failed = (rows) => rows.filter((r) => !r.ok).map((r) => r.what);

test("every check inside its budget passes", () => {
  assert.deepEqual(failed(evaluate(good(), budgets())), []);
});

test("a result file that is missing fails its checks", () => {
  for (const [key, file] of [
    ["restore", "restore.json"],
    ["lateErasure", "late-erasure.json"],
    ["promote", "promote.json"],
    ["privacy", "privacy.json"],
    ["exportBench", "export-bench.json"],
  ]) {
    const results = good();
    delete results[key];
    const rows = evaluate(results, budgets());
    assert.ok(failed(rows).length >= 1, `${key} missing must fail`);
    assert.ok(
      rows.some((r) => !r.ok && r.budget === `needs ${file}`),
      `${key} names ${file}`,
    );
  }
});

test("rows that differ, no new epoch or a key left on an erased player fail a restore", () => {
  const differ = good();
  differ.restore.restores[0] = restored("export", { rows_equal: false });
  assert.deepEqual(failed(evaluate(differ, budgets())), [
    "Restore from a local export on a new PC",
  ]);
  const same = good();
  same.restore.restores[1] = restored("cloud", { epoch_above_source: false });
  assert.deepEqual(failed(evaluate(same, budgets())), [
    "Restore from the cloud backup on a new PC",
  ]);
  const keyed = good();
  keyed.restore.restores[1] = restored("cloud", { erased_without_key: 1 });
  assert.deepEqual(failed(evaluate(keyed, budgets())), [
    "Restore from the cloud backup on a new PC",
  ]);
  const one = good();
  one.restore.restores[0] = restored("export", {
    erased_players: 1,
    erased_without_key: 1,
    erased_pii_empty: 1,
  });
  assert.equal(failed(evaluate(one, budgets())).length, 1, "two erased players are required");
});

test("a drill without 30 days, a fork or an orphaned row fails", () => {
  const short = good();
  short.restore.days = 7;
  assert.deepEqual(failed(evaluate(short, budgets())), ["Drill branch: days of activity, fork"]);
  const flat = good();
  flat.restore.restores[1] = restored("cloud", { orphaned: 0 });
  assert.deepEqual(failed(evaluate(flat, budgets())), [
    "Cloud restore: orphaned rows kept out of state",
  ]);
});

test("the late erasure must erase once online and nothing offline", () => {
  const none = good();
  none.lateErasure.erasures_reapplied = 0;
  assert.equal(failed(evaluate(none, budgets())).length, 1);
  const offline = good();
  offline.lateErasure.offline_erasures_reapplied = 1;
  assert.equal(failed(evaluate(offline, budgets())).length, 1);
});

test("a promote that leaves a key on an erased player fails", () => {
  const bad = good();
  bad.promote.erased_without_key = 1;
  assert.deepEqual(failed(evaluate(bad, budgets())), ["Promote after an erasure"]);
});

test("a finding, a missing artefact kind or an empty log fails the privacy proof", () => {
  const found = good();
  found.privacy.findings = 1;
  assert.ok(failed(evaluate(found, budgets())).includes("Seeded names, phones and emails found in plaintext"));
  const thin = good();
  thin.privacy.kinds = REQUIRED_KINDS.filter((k) => k !== "GST archive");
  assert.deepEqual(failed(evaluate(thin, budgets())), ["Privacy scan covers every artefact"]);
  const quiet = good();
  quiet.privacy.log_bytes = 0;
  assert.deepEqual(failed(evaluate(quiet, budgets())), ["Logs carry player ids only"]);
});

test("the export must take under 10 s on at least the 1 GB database", () => {
  const slow = good();
  slow.exportBench.took_ms = 10_000;
  assert.equal(failed(evaluate(slow, budgets())).length, 1);
  const small = good();
  small.exportBench.database_mib = 100;
  assert.equal(failed(evaluate(small, budgets())).length, 1);
  assert.deepEqual(failed(evaluate(slow, budgets({ RESTORE_EXPORT_MS: "20000" }))), []);
});

test("main reads a folder, writes the report and returns the exit code", () => {
  const dir = mkdtempSync(join(tmpdir(), "restore-drill-"));
  const write = (name, value) => writeFileSync(join(dir, name), JSON.stringify(value));
  const all = good();
  write("restore.json", all.restore);
  write("late-erasure.json", all.lateErasure);
  write("promote.json", all.promote);
  write("privacy.json", all.privacy);
  write("export-bench.json", all.exportBench);
  const quiet = () => {};
  const { log, error } = console;
  console.log = quiet;
  console.error = quiet;
  try {
    assert.equal(main(["--dir", dir, "--date", "2027-01-15"], {}), 0);
    assert.match(readFileSync(join(dir, "report.md"), "utf8"), /^# Restore drill and privacy proof \(2027-01-15\)/);
    assert.equal(Object.keys(load(dir)).length, 5);
    write("privacy.json", { ...all.privacy, findings: 2 });
    assert.equal(main(["--dir", dir], {}), 1);
    assert.equal(main(["--dir", join(dir, "nowhere")], {}), 1, "an empty folder fails every check");
  } finally {
    console.log = log;
    console.error = error;
  }
});

test("the report marks every row", () => {
  const text = render(evaluate(good(), budgets()), "2027-01-15");
  assert.equal(text.split("\n").filter((l) => l.endsWith("| pass |")).length, 10);
  assert.ok(!text.includes("FAIL"));
});
