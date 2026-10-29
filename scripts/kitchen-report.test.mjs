import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  budgets,
  compatRows,
  evaluate,
  load,
  main,
  renderCompat,
  tally,
} from "./kitchen-report.mjs";

const soak = (label) => ({
  source: "soak",
  label,
  orders: 500,
  tickets: 500,
  served: 500,
  advances: 1500,
  kots: 500,
  duplicate_kots: 0,
  lost: 0,
});
const browser = (passed, failed = 0) => ({
  stats: { expected: passed, unexpected: failed, flaky: 0 },
});

const good = {
  hub: { kot_ms: 700, board_ms: 40, hud_ms: 3200, tile_ms: 30 },
  unplugged: { alert: true, board: true },
  burst: {
    rate: 5,
    secs: 60,
    kiosks: 20,
    displays: 2,
    orders: 300,
    kots: 300,
    kot_once_each: true,
    kot_p95_ms: 900,
    boards_match: true,
  },
  soakUp: soak("wan-up"),
  soakDown: soak("wan-down"),
  browsers: { "android-chrome": browser(24), "ipad-safari": browser(24) },
};

const failing = (results, env = {}) =>
  evaluate(results, budgets(env))
    .filter((r) => !r.ok)
    .map((r) => r.what);

test("a run inside every budget passes", () => {
  assert.deepEqual(failing(good), []);
});

test("each Journey 4 leg has its own budget", () => {
  assert.deepEqual(failing({ ...good, hub: { ...good.hub, kot_ms: 3001 } }), [
    "KOT prints after commit",
  ]);
  assert.deepEqual(failing({ ...good, hub: { ...good.hub, board_ms: 2001 } }), [
    "Ticket appears on the board",
  ]);
  assert.deepEqual(failing({ ...good, hub: { ...good.hub, hud_ms: 5001 } }), [
    "HUD ready copy after the Ready tap",
  ]);
  assert.deepEqual(failing({ ...good, hub: { ...good.hub, hud_ms: 5000 } }), []);
  assert.deepEqual(failing({ ...good, hub: { ...good.hub, tile_ms: 2001 } }), [
    "Tile shows READY after the Ready tap",
  ]);
});

test("an unplugged printer needs the alert and the board's ticket", () => {
  assert.deepEqual(failing({ ...good, unplugged: { alert: false, board: true } }), [
    "Printer unplugged: Printer offline alert",
  ]);
  assert.deepEqual(failing({ ...good, unplugged: { alert: true, board: false } }), [
    "Printer unplugged: board gets the ticket",
  ]);
});

test("the burst must be 5 a second for a minute across 20 kiosks and 2 displays", () => {
  assert.deepEqual(failing({ ...good, burst: { ...good.burst, secs: 3 } }), ["Burst shape"]);
  assert.deepEqual(failing({ ...good, burst: { ...good.burst, kiosks: 5 } }), ["Burst shape"]);
  assert.deepEqual(failing({ ...good, burst: { ...good.burst, displays: 1 } }), ["Burst shape"]);
});

test("every KOT prints once, the boards match and p95 stays within 3 s", () => {
  assert.deepEqual(failing({ ...good, burst: { ...good.burst, kots: 301 } }), [
    "Burst: every KOT prints once",
  ]);
  assert.deepEqual(failing({ ...good, burst: { ...good.burst, kot_once_each: false } }), [
    "Burst: every KOT prints once",
  ]);
  assert.deepEqual(failing({ ...good, burst: { ...good.burst, boards_match: false } }), [
    "Burst: both boards match the Hub",
  ]);
  assert.deepEqual(failing({ ...good, burst: { ...good.burst, kot_p95_ms: 3001 } }), [
    "Burst: KOT latency, 95th percentile",
  ]);
});

test("the soak needs 500 orders and loses nothing", () => {
  const short = { ...soak("wan-down"), orders: 100, served: 100, kots: 100, advances: 300 };
  const bad = failing({ ...good, soakDown: short });
  assert.ok(bad.includes("Soak WAN down: orders"));
  const lost = { ...soak("wan-down"), served: 499, lost: 1 };
  assert.ok(
    failing({ ...good, soakDown: lost }).includes("Soak WAN down: ticket, advance or KOT lost"),
  );
  const twice = { ...soak("wan-down"), kots: 501, duplicate_kots: 1 };
  assert.ok(
    failing({ ...good, soakDown: twice }).includes("Soak WAN down: every KOT printed once"),
  );
});

test("a WAN-down soak that differs from the WAN-up one fails", () => {
  const other = { ...soak("wan-down"), advances: 1499 };
  assert.ok(failing({ ...good, soakDown: other }).includes("Soak: WAN down differs from WAN up"));
  assert.deepEqual(failing({ ...good, soakDown: soak("wan-down") }), []);
});

test("both browsers must have run and passed", () => {
  assert.deepEqual(
    failing({ ...good, browsers: { ...good.browsers, "ipad-safari": browser(20, 1) } }),
    ["Kitchen display on Safari on iPad"],
  );
  assert.deepEqual(failing({ ...good, browsers: { ...good.browsers, "ipad-safari": undefined } }), [
    "Kitchen display on Safari on iPad",
  ]);
  assert.deepEqual(
    failing({ ...good, browsers: { ...good.browsers, "android-chrome": browser(0) } }),
    ["Kitchen display on Chrome on Android"],
  );
});

test("a missing result is a failure, never a pass", () => {
  const none = evaluate(
    {
      hub: undefined,
      unplugged: undefined,
      burst: undefined,
      soakUp: undefined,
      soakDown: undefined,
      browsers: {},
    },
    budgets({}),
  );
  assert.ok(none.length > 0 && none.every((r) => !r.ok));
});

test("budgets can be lowered for a trial run", () => {
  assert.deepEqual(failing(good, { KITCHEN_KOT_MS: "500" }), ["KOT prints after commit"]);
  const small = {
    ...good,
    soakUp: { ...soak("wan-up"), orders: 40, served: 40, kots: 40, advances: 120 },
  };
  assert.ok(failing({ ...small, soakDown: { ...small.soakUp, label: "wan-down" } }).length > 0);
  assert.deepEqual(
    failing(
      { ...small, soakDown: { ...small.soakUp, label: "wan-down" } },
      { KITCHEN_SOAK_ORDERS: "40" },
    ),
    [],
  );
});

test("tally reads a Playwright report", () => {
  assert.deepEqual(tally(browser(5, 2)), { passed: 5, failed: 2 });
  assert.equal(tally({}), undefined);
  assert.deepEqual(tally({ stats: { expected: 3, unexpected: 0, flaky: 1 } }), {
    passed: 3,
    failed: 1,
  });
});

test("the compatibility list keeps one row per browser and replaces a repeat", () => {
  const first = renderCompat("", good, "2026-10-01");
  assert.deepEqual(compatRows(first), [
    ["Chrome on Android", "24", "0", "2026-10-01"],
    ["Safari on iPad", "24", "0", "2026-10-01"],
  ]);
  const second = renderCompat(
    first,
    { ...good, browsers: { "android-chrome": undefined, "ipad-safari": browser(25, 1) } },
    "2026-11-01",
  );
  assert.deepEqual(compatRows(second), [
    ["Chrome on Android", "24", "0", "2026-10-01"],
    ["Safari on iPad", "25", "1", "2026-11-01"],
  ]);
});

test("main writes the report and the list and exits 1 on a miss", () => {
  const dir = mkdtempSync(join(tmpdir(), "kitchen-"));
  const put = (name, value) => writeFileSync(join(dir, name), JSON.stringify(value));
  put("hub.json", good.hub);
  put("unplugged.json", good.unplugged);
  put("burst.json", good.burst);
  put("soak-wan-up.json", good.soakUp);
  put("soak-wan-down.json", good.soakDown);
  put("browser-android-chrome.json", good.browsers["android-chrome"]);
  put("browser-ipad-safari.json", good.browsers["ipad-safari"]);
  const compat = join(dir, "kitchen-display.md");
  const args = ["--dir", dir, "--compat", compat, "--date", "2026-10-29"];
  const quiet = console.log;
  const loud = console.error;
  console.log = console.error = () => {};
  try {
    assert.equal(main(args, {}), 0);
    assert.match(readFileSync(join(dir, "report.md"), "utf8"), /Kitchen flow on the reference rig/);
    assert.match(readFileSync(compat, "utf8"), /Chrome on Android \| 24 \| 0 \| 2026-10-29/);
    put("hub.json", { ...good.hub, kot_ms: 9000 });
    assert.equal(main(args, {}), 1);
    assert.equal(load(dir).hub.kot_ms, 9000);
  } finally {
    console.log = quiet;
    console.error = loud;
  }
});
