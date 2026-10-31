import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { BANDS, STEPS, budgets, evaluate, load, main } from "./billing-report.mjs";

const move = (pushed) => ({
  history_days: 30,
  sessions: 5,
  kiosks: 20,
  last_seq: 412,
  checksums_equal: true,
  sessions_continue: true,
  follow_ms: 900,
  budget_ms: 30_000,
  successor_pushed: pushed,
});
const good = {
  walk: { bands: BANDS, steps: STEPS },
  cancel: { bands: BANDS },
  refund: { bands: BANDS },
  downgrade: {
    sessions: 10,
    sessions_ended: 0,
    tickets_open: 2,
    devices: 24,
    over_cap: 4,
    ads_before_exp: false,
    ads_at_exp: true,
  },
  moveLan: move(true),
  moveFile: move(false),
};
const staging = {
  subscriptions: [
    { plan: "basic-monthly", method: "upi", charged: true, refunded: true },
    { plan: "premium-yearly", method: "card", charged: true, refunded: true },
    { plan: "premium-yearly", method: "emandate", charged: true, refunded: true },
  ],
  upi_offered_for_premium_yearly: false,
};

const failing = (rows) => rows.filter((r) => !r.ok).map((r) => r.what);

test("a full set of results passes, and the staging run is only 'not run'", () => {
  const rows = evaluate(good);
  assert.deepEqual(failing(rows), []);
  assert.ok(rows.some((r) => r.pending));
});

test("--strict makes the staging run required", () => {
  assert.equal(failing(evaluate(good, budgets(), true)).length, 1);
  assert.deepEqual(failing(evaluate({ ...good, staging }, budgets(), true)), []);
});

test("every missing result is a failure", () => {
  for (const key of Object.keys(good)) {
    const { [key]: _gone, ...rest } = good;
    assert.ok(failing(evaluate(rest)).length > 0, key);
  }
});

test("a band or a step left out fails the walk", () => {
  assert.equal(failing(evaluate({ ...good, walk: { bands: ["IN"], steps: STEPS } })).length, 1);
  assert.equal(failing(evaluate({ ...good, walk: { bands: BANDS, steps: ["create"] } })).length, 1);
  assert.equal(failing(evaluate({ ...good, cancel: { bands: ["IN", "MY"] } })).length, 1);
});

test("an ended session, no over-cap device or early ads fail the downgrade", () => {
  const bad = (patch) => failing(evaluate({ ...good, downgrade: { ...good.downgrade, ...patch } }));
  assert.equal(bad({ sessions_ended: 1 }).length, 1);
  assert.equal(bad({ over_cap: 0 }).length, 1);
  assert.equal(bad({ ads_before_exp: true }).length, 1);
  assert.equal(bad({ ads_at_exp: false }).length, 1);
  assert.equal(bad({ sessions: 9 }).length, 1);
});

test("the move fails on unequal checksums, a slow follow or too few kiosks", () => {
  const bad = (patch) => failing(evaluate({ ...good, moveLan: { ...move(true), ...patch } }));
  assert.equal(bad({ checksums_equal: false }).length, 1);
  assert.equal(bad({ sessions_continue: false }).length, 1);
  assert.equal(bad({ follow_ms: 30_000 }).length, 1);
  assert.equal(bad({ kiosks: 19 }).length, 1);
  assert.equal(bad({ successor_pushed: false }).length, 1);
  assert.equal(bad({ sessions: 4 }).length, 1);
});

test("a move by file pushes no successor", () => {
  const rows = evaluate({ ...good, moveFile: move(true) });
  assert.equal(failing(rows).length, 1);
});

test("a staging run that offered UPI for Premium yearly, or left a refund undone, fails", () => {
  const bad = (patch) => failing(evaluate({ ...good, staging: { ...staging, ...patch } }));
  assert.equal(bad({ upi_offered_for_premium_yearly: true }).length, 1);
  assert.equal(
    bad({ subscriptions: staging.subscriptions.map((s) => ({ ...s, refunded: false })) }).length,
    1,
  );
});

test("main reads the folder, writes the report and returns the exit code", () => {
  const dir = mkdtempSync(join(tmpdir(), "billing-"));
  const quiet = console.error;
  console.error = () => {};
  try {
    assert.equal(main(["--dir", dir, "--date", "2026-10-31"]), 1, "nothing was written");
    const names = {
      walk: "billing-walk",
      cancel: "billing-cancel",
      refund: "billing-refund",
      downgrade: "downgrade",
      moveLan: "move-lan",
      moveFile: "move-file",
    };
    for (const [k, name] of Object.entries(names))
      writeFileSync(join(dir, `${name}.json`), JSON.stringify(good[k]));
    assert.deepEqual(Object.keys(load(dir)).sort(), Object.keys(names).sort());
    assert.equal(main(["--dir", dir, "--date", "2026-10-31"]), 0);
    assert.match(readFileSync(join(dir, "report.md"), "utf8"), /2026-10-31/);
    assert.equal(main(["--dir", dir, "--strict"]), 1);
    writeFileSync(join(dir, "billing-walk.json"), "not json");
    assert.equal(main(["--dir", dir]), 1, "a damaged file is missing");
  } finally {
    console.error = quiet;
  }
});
