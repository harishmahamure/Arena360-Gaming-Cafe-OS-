import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { budgets, evaluate, firstRenderMs, load, main } from "./remote-report.mjs";

const playwright = (ms) => ({
  suites: [
    {
      suites: [
        {
          specs: [
            { tests: [{ annotations: [{ type: "first-render-ms", description: String(ms) }] }] },
          ],
        },
      ],
    },
  ],
});

const good = {
  today: {
    budget_within_pct: 99,
    tiers: [
      { tier: "basic", opens: 1000, cadence_s: 900, within_cadence_pct: 99.9 },
      { tier: "premium", opens: 1000, cadence_s: 300, within_cadence_pct: 99.5 },
    ],
  },
  firstRender: playwright(1400),
  commands: { commands: 200, p95_ms: 56_500, max_ms: 59_500 },
  catchUp: { outage_hours: 8, elapsed_ms: 35_000, gaps_found: 0, duplicate_objects: 0 },
  cost: {
    average_paise: 651,
    tiers: [
      { tier: "basic", puts_month: 17_340, cost_paise: 651 },
      { tier: "premium", puts_month: 17_340, cost_paise: 651 },
    ],
  },
  audit: { rows: 30_000, first_results_ms: 2300 },
};
const rig = {
  soak: {
    outage_hours: 8,
    kiosks: 20,
    sessions: 1000,
    orders: 500,
    catch_up_ms: 420_000,
    gaps: 0,
    duplicate_sends: 0,
  },
  cloudDown: { hours: 1, changes_beyond_offline_chip: 0, caught_up: true },
};

const failed = (rows) => rows.filter((r) => !r.ok).map((r) => r.what);

test("every measure inside its budget passes, the rig's two are not run", () => {
  const rows = evaluate(good, budgets());
  assert.deepEqual(failed(rows), []);
  assert.equal(rows.filter((r) => r.pending).length, 2);
});

test("strict makes the rig's two required, and a full run passes", () => {
  assert.equal(failed(evaluate(good, budgets(), true)).length, 2);
  assert.deepEqual(failed(evaluate({ ...good, ...rig }, budgets(), true)), []);
});

test("a missed budget fails its row", () => {
  const slow = { ...good, firstRender: playwright(2100) };
  assert.deepEqual(failed(evaluate(slow, budgets())), ["Today first render, reference phone"]);
  const old = {
    ...good,
    today: {
      tiers: [
        { tier: "basic", opens: 1000, cadence_s: 900, within_cadence_pct: 99.9 },
        { tier: "premium", opens: 1000, cadence_s: 300, within_cadence_pct: 98.9 },
      ],
    },
  };
  assert.equal(failed(evaluate(old, budgets())).length, 1);
  const dear = { ...good, cost: { ...good.cost, average_paise: 1000 } };
  assert.deepEqual(failed(evaluate(dear, budgets())), ["R2 cost per branch per month, average"]);
  const gap = { ...good, catchUp: { ...good.catchUp, gaps_found: 1 } };
  assert.deepEqual(failed(evaluate(gap, budgets())), ["Catch-up: gaps and duplicates"]);
  const late = { ...good, audit: { rows: 1, first_results_ms: 10_000 } };
  assert.equal(failed(evaluate(late, budgets())).length, 1);
  const slowCmd = { ...good, commands: { commands: 200, p95_ms: 120_000 } };
  assert.equal(failed(evaluate(slowCmd, budgets())).length, 1);
});

test("a short run does not count: fewer opens, commands or outage hours than the story's", () => {
  const few = {
    ...good,
    today: { tiers: good.today.tiers.map((t) => ({ ...t, opens: 100 })) },
    commands: { commands: 20, p95_ms: 1000 },
    catchUp: { ...good.catchUp, outage_hours: 1 },
  };
  assert.equal(failed(evaluate(few, budgets())).length, 4);
});

test("a measurement that is absent is a failure, never a pass", () => {
  const rows = evaluate({}, budgets());
  assert.equal(failed(rows).length, rows.filter((r) => !r.pending).length);
  assert.ok(rows.some((r) => r.value === "missing"));
});

test("a rig result that misses its budget fails even when not strict", () => {
  const slow = { ...good, soak: { ...rig.soak, catch_up_ms: 700_000 } };
  assert.equal(failed(evaluate(slow, budgets())).length, 1);
  const lossy = { ...good, soak: { ...rig.soak, duplicate_sends: 1 } };
  assert.equal(failed(evaluate(lossy, budgets())).length, 1);
  const trading = { ...good, cloudDown: { ...rig.cloudDown, changes_beyond_offline_chip: 2 } };
  assert.equal(failed(evaluate(trading, budgets())).length, 1);
});

test("the first render is read from the Playwright annotation, the slowest of several", () => {
  assert.equal(firstRenderMs(playwright(1234)), 1234);
  assert.equal(firstRenderMs({ suites: [] }), undefined);
  assert.equal(firstRenderMs(undefined), undefined);
  const two = playwright(1000);
  two.suites[0].suites[0].specs[0].tests.push(
    playwright(1800).suites[0].suites[0].specs[0].tests[0],
  );
  assert.equal(firstRenderMs(two), 1800);
});

test("budgets are overridable for a trial", () => {
  const b = budgets({ REMOTE_FIRST_RENDER_MS: "3000", REMOTE_COST_PAISE: "500" });
  assert.equal(b.firstRenderMs, 3000);
  assert.equal(b.costPaise, 500);
  assert.equal(budgets({}).auditMs, 10_000);
});

test("main reads a folder, writes the report and exits 1 on a failure", () => {
  const dir = mkdtempSync(join(tmpdir(), "remote-"));
  const put = (name, v) => writeFileSync(join(dir, name), JSON.stringify(v));
  put("today.json", good.today);
  put("first-render.json", good.firstRender);
  put("commands.json", good.commands);
  put("catch_up.json", good.catchUp);
  put("cost.json", good.cost);
  put("audit.json", good.audit);
  assert.equal(Object.keys(load(dir)).filter((k) => load(dir)[k]).length, 6);
  const log = console.log;
  console.log = () => {};
  try {
    assert.equal(main(["--dir", dir, "--date", "2026-10-30"]), 0);
    const md = readFileSync(join(dir, "report.md"), "utf8");
    assert.match(md, /remote experience against its targets \(2026-10-30\)/);
    assert.match(md, /not run/);
    assert.equal(main(["--dir", dir, "--strict"]), 1);
    put("audit.json", { rows: 1, first_results_ms: 99_999 });
    const err = console.error;
    console.error = () => {};
    assert.equal(main(["--dir", dir]), 1);
    console.error = err;
  } finally {
    console.log = log;
  }
});
