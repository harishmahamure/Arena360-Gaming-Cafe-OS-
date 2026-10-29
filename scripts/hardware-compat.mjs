#!/usr/bin/env node
// Story 5.39: publishes the monthly hardware job's results to the compatibility list.
//
//   node scripts/hardware-compat.mjs <printers.json> [<scanner.json>] [--out docs/compat/hardware.md] [--date YYYY-MM-DD]
//
// `printers.json` is written by the `hardware_bench_*` test in crates/arena-tasks (one entry per
// listed printer: name, kind, paper and the state of its receipt, z_report and drawer jobs);
// `scanner.json` by tests/bench/scanner.spec.ts ({name, ok, took_ms}). The list keeps one row per
// printer (name, connection, paper) and one per scanner; a new run replaces the row it repeats and
// leaves the others. The exit code is 1 when any listed job or scan failed, so the job fails too.

import { readFileSync, writeFileSync, existsSync, mkdirSync, realpathSync } from "node:fs";
import { dirname } from "node:path";
import { fileURLToPath } from "node:url";

export const OUT = "docs/compat/hardware.md";
const JOBS = ["receipt", "z_report", "drawer", "kot"];
const PRINTER_HEAD =
  "| Printer | Connection | Paper | Receipt | Z-report | Drawer kick | KOT | Last run |";
const SCANNER_HEAD = "| Scanner | Adds an item | Last run |";

const mark = (ok) => (ok ? "pass" : "FAIL");

/** The rows of the table that starts with `head` in `md`, as arrays of cells. */
export function rowsOf(md, head) {
  const lines = md.split("\n");
  const at = lines.indexOf(head);
  if (at < 0) return [];
  const rows = [];
  for (const line of lines.slice(at + 2)) {
    if (!line.startsWith("|")) break;
    rows.push(
      line
        .split("|")
        .slice(1, -1)
        .map((c) => c.trim()),
    );
  }
  return rows;
}

const table = (head, cols, rows) =>
  [head, `|${cols.map(() => " --- |").join("")}`, ...rows.map((r) => `| ${r.join(" | ")} |`)].join(
    "\n",
  );

/** Merges a run into the list: a repeated printer or scanner replaces its row. */
export function render(existing, printers, scanner, date) {
  const printerRows = new Map(
    rowsOf(existing, PRINTER_HEAD).map((r) => [`${r[0]}|${r[1]}|${r[2]}`, r]),
  );
  for (const p of printers) {
    printerRows.set(`${p.name}|${p.kind}|${p.paper} mm`, [
      p.name,
      p.kind,
      `${p.paper} mm`,
      ...JOBS.map((j) => mark(p.jobs?.[j] === "done")),
      date,
    ]);
  }
  const scannerRows = new Map(rowsOf(existing, SCANNER_HEAD).map((r) => [r[0], r]));
  if (scanner) scannerRows.set(scanner.name, [scanner.name, mark(scanner.ok), date]);
  const sorted = (m) => [...m.values()].sort((a, b) => a[0].localeCompare(b[0]));
  return [
    "# Hardware compatibility",
    "",
    "Written by the monthly hardware job (`.github/workflows/hardware-bench.yml`, Story 5.39) from",
    "the test café's printers and scanner. A printer passes when its sale receipt, Z-report,",
    "drawer-kick and kitchen-ticket (KOT, within 3 s, Story 7.11) jobs all come back done at the",
    "paper width listed; whoever loads the paper checks the slips and the drawer by eye before the",
    "run is accepted.",
    "",
    "## ESC/POS printers",
    "",
    table(PRINTER_HEAD, [0, 1, 2, 3, 4, 5, 6, 7], sorted(printerRows)),
    "",
    "## Keyboard-wedge scanners",
    "",
    table(SCANNER_HEAD, [0, 1, 2], sorted(scannerRows)),
    "",
  ].join("\n");
}

/** The failures in a run, for the exit code and the job summary. */
export function failures(printers, scanner) {
  const out = [];
  for (const p of printers)
    for (const j of JOBS) if (p.jobs?.[j] !== "done") out.push(`${p.name} ${p.paper} mm: ${j}`);
  if (scanner && !scanner.ok) out.push(`${scanner.name}: scan`);
  return out;
}

export function main(argv) {
  const files = argv.filter((a, i) => !a.startsWith("--") && !argv[i - 1]?.startsWith("--"));
  const flag = (name, fallback) => {
    const i = argv.indexOf(name);
    return i >= 0 && argv[i + 1] !== undefined ? argv[i + 1] : fallback;
  };
  if (!files[0]) {
    console.error("usage: hardware-compat.mjs <printers.json> [<scanner.json>] [--out file]");
    return 2;
  }
  if (!existsSync(files[0])) {
    console.error(`hardware bench failed: no printer results at ${files[0]}`);
    return 1;
  }
  const printers = JSON.parse(readFileSync(files[0], "utf8"));
  const scanner =
    files[1] && existsSync(files[1]) ? JSON.parse(readFileSync(files[1], "utf8")) : null;
  const out = flag("--out", OUT);
  const date = flag("--date", new Date().toISOString().slice(0, 10));
  const existing = existsSync(out) ? readFileSync(out, "utf8") : "";
  mkdirSync(dirname(out), { recursive: true });
  writeFileSync(out, render(existing, printers, scanner, date));
  const bad = failures(printers, scanner);
  for (const b of bad) console.error(`hardware bench failed: ${b}`);
  return bad.length ? 1 : 0;
}

if (process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exit(main(process.argv.slice(2)));
}
