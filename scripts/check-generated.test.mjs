import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { formatReport, gitStatus, parsePorcelain } from "./check-generated.mjs";

test("fresh: empty porcelain output passes", () => {
  assert.deepEqual(parsePorcelain(""), []);
  assert.deepEqual(formatReport(parsePorcelain("")), []);
});

test("stale, new and deleted files are each listed", () => {
  const out = [
    " M packages/ui/src/generated/ErrorCode.ts",
    "?? packages/ui/src/generated/Session.ts",
    " D packages/ui/src/generated/Old.ts",
    "",
  ].join("\n");
  const entries = parsePorcelain(out);
  assert.deepEqual(
    entries.map((e) => [e.kind, e.path]),
    [
      ["stale", "packages/ui/src/generated/ErrorCode.ts"],
      ["new", "packages/ui/src/generated/Session.ts"],
      ["deleted", "packages/ui/src/generated/Old.ts"],
    ],
  );
  const report = formatReport(entries);
  assert.match(report[0], /out of date/);
  assert.ok(report.includes("  stale: packages/ui/src/generated/ErrorCode.ts"));
  assert.ok(report.includes("  new: packages/ui/src/generated/Session.ts"));
  assert.ok(report.includes("  deleted: packages/ui/src/generated/Old.ts"));
});

test("staged changes, renames and CRLF output are parsed", () => {
  const entries = parsePorcelain(
    "M  packages/ui/src/generated/A.ts\r\nR  packages/ui/src/generated/B.ts -> packages/ui/src/generated/C.ts\r\n",
  );
  assert.deepEqual(
    entries.map((e) => [e.kind, e.path]),
    [
      ["stale", "packages/ui/src/generated/A.ts"],
      ["renamed", "packages/ui/src/generated/C.ts"],
    ],
  );
});

test("gitStatus reports modified, new and deleted files in a real repo", () => {
  const root = mkdtempSync(join(tmpdir(), "generated-"));
  try {
    const git = (...args) =>
      execFileSync("git", ["-c", "user.name=t", "-c", "user.email=t@t", ...args], {
        cwd: root,
        stdio: "ignore",
      });
    const dir = join(root, "packages/ui/src/generated");
    mkdirSync(dir, { recursive: true });
    writeFileSync(join(dir, "ErrorCode.ts"), 'export type ErrorCode = "A";\n');
    writeFileSync(join(dir, "Old.ts"), "export type Old = never;\n");
    writeFileSync(join(root, "outside.txt"), "x\n");
    git("init", "-q");
    git("add", "-A");
    git("commit", "-qm", "fixture");

    writeFileSync(join(dir, "ErrorCode.ts"), 'export type ErrorCode = "A" | "B";\n');
    writeFileSync(join(dir, "Session.ts"), "export type Session = {};\n");
    rmSync(join(dir, "Old.ts"));
    writeFileSync(join(root, "outside.txt"), "changed\n");

    const report = formatReport(parsePorcelain(gitStatus(root)));
    assert.deepEqual(report.slice(1).sort(), [
      "  deleted: packages/ui/src/generated/Old.ts",
      "  new: packages/ui/src/generated/Session.ts",
      "  stale: packages/ui/src/generated/ErrorCode.ts",
    ]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
