import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { failures, main, render, rowsOf } from "./hardware-compat.mjs";

const done = { receipt: "done", z_report: "done", drawer: "done" };
const printer = (name, paper, jobs = done) => ({ name, kind: "usb", paper, jobs });

test("a run lists each printer with its three jobs and the date", () => {
  const md = render("", [printer("Epson TM-T20", "80")], null, "2026-11-01");
  assert.match(md, /\| Epson TM-T20 \| usb \| 80 mm \| pass \| pass \| pass \| 2026-11-01 \|/);
});

test("a repeated printer replaces its row and the others stay", () => {
  const first = render("", [printer("A", "58"), printer("B", "80")], null, "2026-11-01");
  const next = render(
    first,
    [printer("A", "58", { ...done, drawer: "failed" })],
    null,
    "2026-12-01",
  );
  const rows = rowsOf(
    next,
    "| Printer | Connection | Paper | Receipt | Z-report | Drawer kick | Last run |",
  );
  assert.equal(rows.length, 2);
  assert.deepEqual(rows[0].slice(3), ["pass", "pass", "FAIL", "2026-12-01"]);
  assert.equal(rows[1][6], "2026-11-01");
});

test("the same printer on two paper widths is two rows", () => {
  const md = render("", [printer("A", "58"), printer("A", "80")], null, "2026-11-01");
  assert.equal(
    rowsOf(md, "| Printer | Connection | Paper | Receipt | Z-report | Drawer kick | Last run |")
      .length,
    2,
  );
});

test("a scanner that added its item is listed as a pass", () => {
  const md = render("", [], { name: "Honeywell 1250g", ok: true, took_ms: 900 }, "2026-11-01");
  assert.match(md, /\| Honeywell 1250g \| pass \| 2026-11-01 \|/);
});

test("any job that is not done, or a failed scan, is a failure", () => {
  assert.deepEqual(failures([printer("A", "58")], { name: "S", ok: true }), []);
  assert.deepEqual(failures([printer("A", "58", { ...done, z_report: "failed" })], null), [
    "A 58 mm: z_report",
  ]);
  assert.deepEqual(failures([], { name: "S", ok: false }), ["S: scan"]);
});

test("the script exits 1 on a failed job and still lists the scanner, 0 on a clean run, 2 with no file", () => {
  const dir = mkdtempSync(join(tmpdir(), "hwc-"));
  const out = join(dir, "hardware.md");
  const good = join(dir, "good.json");
  const bad = join(dir, "bad.json");
  const scan = join(dir, "scanner.json");
  writeFileSync(good, JSON.stringify([printer("A", "58")]));
  writeFileSync(bad, JSON.stringify([printer("A", "58", { ...done, drawer: "failed" })]));
  writeFileSync(scan, JSON.stringify({ name: "S1", ok: true }));
  const quiet = console.error;
  console.error = () => {};
  try {
    assert.equal(main([good, scan, "--out", out]), 0);
    assert.match(readFileSync(out, "utf8"), /| S1 | pass |/);
    assert.equal(main([bad, scan, "--out", out]), 1);
    assert.equal(main([good, join(dir, "missing.json"), "--out", out]), 0);
    assert.equal(main([join(dir, "nope.json"), "--out", out]), 1);
    assert.equal(main([]), 2);
  } finally {
    console.error = quiet;
  }
});
