import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { budgets, evaluate, load, main, render } from "./journey-report.mjs";

const good = {
  hub: { login_ms: 400, approve_ms: 300, extend_ms: 500, order_ms: 900 },
  ui: { login_ms: 900, shell_ms: 700, extend_ms: 400, returned_to_attract: true },
  attract: { phase: "attract", mb: 61.2 },
  shell: { phase: "shell", mb: 74 },
  down: { down_secs: 1800, locked_late_ms: 120, locked_early: false },
  residue: [
    { os: "Windows 10", sessions: 20, residue: [] },
    { os: "Windows 11", sessions: 20, residue: [] },
  ],
};

const failing = (results, env = {}) =>
  evaluate(results, budgets(env))
    .filter((r) => !r.ok)
    .map((r) => r.what);

test("a run inside every budget passes", () => {
  assert.deepEqual(failing(good), []);
});

test("login adds the Hub's leg to the kiosk's and must stay under 3 s", () => {
  assert.deepEqual(failing({ ...good, hub: { ...good.hub, login_ms: 2100 } }), ["Login"]);
  assert.deepEqual(failing({ ...good, hub: { ...good.hub, login_ms: 2099 } }), []);
});

test("the shell opens under 2 s after approval and Extend reaches the floor within 2 s", () => {
  assert.deepEqual(failing({ ...good, ui: { ...good.ui, shell_ms: 1700 } }), [
    "Shell opens after approval",
  ]);
  assert.deepEqual(failing({ ...good, ui: { ...good.ui, extend_ms: 1500 } }), []);
  assert.deepEqual(failing({ ...good, ui: { ...good.ui, extend_ms: 1501 } }), [
    "Extend reaches the floor",
  ]);
});

test("an order must be on the tile within 3 s", () => {
  assert.deepEqual(failing({ ...good, hub: { ...good.hub, order_ms: 3001 } }), [
    "Order on the Hub tile",
  ]);
});

test("the kiosk must stay under 80 MB idle on attract and in the shell", () => {
  assert.deepEqual(failing({ ...good, shell: { phase: "shell", mb: 80 } }), [
    "Kiosk memory, idle in the shell",
  ]);
  assert.deepEqual(failing({ ...good, attract: { phase: "attract", mb: 95 } }), [
    "Kiosk memory, idle on attract",
  ]);
});

test("the Hub outage must last 30 minutes, never lock early and lock within 1 s of zero", () => {
  assert.deepEqual(failing({ ...good, down: { ...good.down, down_secs: 600 } }), [
    "Hub outage length",
  ]);
  assert.deepEqual(failing({ ...good, down: { ...good.down, locked_early: true } }), [
    "Locked before zero during the outage",
  ]);
  assert.deepEqual(failing({ ...good, down: { ...good.down, locked_late_ms: 1001 } }), [
    "Lock lands after zero by",
  ]);
  assert.deepEqual(
    failing({ ...good, down: { ...good.down, down_secs: 5 } }, { JOURNEY_DOWN_SECS: "5" }),
    [],
  );
});

test("any residue, or fewer than 20 sessions, fails that Windows version", () => {
  const dirty = { os: "Windows 11", sessions: 20, residue: ["steam: loginusers.vdf"] };
  assert.deepEqual(failing({ ...good, residue: [good.residue[0], dirty] }), [
    "Windows 11: residue after a session",
  ]);
  assert.deepEqual(
    failing({ ...good, residue: [{ os: "Windows 10", sessions: 19, residue: [] }] }),
    ["Windows 10: sessions run"],
  );
});

test("a missing measurement is a failure, never a pass", () => {
  assert.ok(failing({ ...good, hub: undefined }).includes("Login"));
  assert.ok(failing({ ...good, ui: undefined }).includes("PC returns to attract after clean-up"));
  assert.deepEqual(failing({ ...good, down: undefined }), ["Hub outage"]);
  assert.deepEqual(failing({ ...good, residue: [] }), ["Launcher logout residue"]);
  assert.deepEqual(failing({ ...good, attract: undefined }), ["Kiosk memory, idle on attract"]);
});

test("a kiosk that did not return to attract fails", () => {
  assert.deepEqual(failing({ ...good, ui: { ...good.ui, returned_to_attract: false } }), [
    "PC returns to attract after clean-up",
  ]);
});

test("the report lists every measure with its budget", () => {
  const md = render(evaluate(good, budgets()), "2026-11-01");
  assert.match(md, /# Player journey on the reference rig \(2026-11-01\)/);
  assert.match(md, /\| Login \| 1300 ms \| 3000 ms \| pass \|/);
  assert.match(md, /\| Windows 11: residue after a session \| 0 \| 0 \| pass \|/);
});

test("main reads a directory, writes the report and exits 1 on a miss", () => {
  const dir = mkdtempSync(join(tmpdir(), "journey-"));
  const put = (name, v) => writeFileSync(join(dir, name), JSON.stringify(v));
  put("hub.json", good.hub);
  put("ui.json", good.ui);
  put("idle-attract.json", good.attract);
  put("idle-shell.json", good.shell);
  put("hub-down.json", good.down);
  put("residue-win10.json", good.residue[0]);
  put("residue-win11.json", good.residue[1]);
  assert.equal(load(dir).residue.length, 2);
  assert.equal(main(["--dir", dir]), 0);
  assert.match(readFileSync(join(dir, "report.md"), "utf8"), /pass/);
  put("idle-shell.json", { phase: "shell", mb: 120 });
  assert.equal(main(["--dir", dir]), 1);
  assert.match(readFileSync(join(dir, "report.md"), "utf8"), /FAIL/);
});

test("an empty directory fails with every measure missing", () => {
  const dir = mkdtempSync(join(tmpdir(), "journey-"));
  assert.equal(main(["--dir", dir]), 1);
});
