#!/usr/bin/env node
// Freshness gate: regenerates the protocol types and fails if the committed files differ.

import { execFileSync } from "node:child_process";
import { realpathSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { GENERATED_DIR, generate } from "./gen-types.mjs";

const KINDS = { "??": "new", D: "deleted", A: "new", M: "stale", R: "renamed" };

/**
 * Parses `git status --porcelain` (v1) output.
 * @returns {Array<{status: string, path: string, kind: string}>}
 */
export function parsePorcelain(text) {
  const entries = [];
  for (const line of text.split(/\r?\n/)) {
    if (line.length < 4) continue;
    const status = line.slice(0, 2);
    let path = line.slice(3);
    if (path.includes(" -> ")) path = path.split(" -> ").pop();
    path = path.replace(/^"(.*)"$/, "$1");
    const code = status === "??" ? "??" : status.replace(/ /g, "")[0];
    entries.push({ status, path, kind: KINDS[code] ?? "stale" });
  }
  return entries;
}

/** Human-readable failure report, or `[]` when the folder is fresh. */
export function formatReport(entries) {
  if (entries.length === 0) return [];
  return [
    `generated protocol types are out of date (run \`pnpm gen:types\` and commit):`,
    ...entries.map((e) => `  ${e.kind}: ${e.path}`),
  ];
}

export function gitStatus(root) {
  return execFileSync(
    "git",
    ["status", "--porcelain", "--untracked-files=all", "--", GENERATED_DIR],
    { cwd: root, encoding: "utf8" },
  );
}

async function main() {
  const root = join(dirname(fileURLToPath(import.meta.url)), "..");
  await generate(root);
  const report = formatReport(parsePorcelain(gitStatus(root)));
  if (report.length > 0) {
    for (const line of report) console.error(line);
    process.exit(1);
  }
  console.log("generated check: ok");
}

const isMain = (() => {
  try {
    return (
      Boolean(process.argv[1]) &&
      realpathSync(fileURLToPath(import.meta.url)) === realpathSync(process.argv[1])
    );
  } catch {
    return false;
  }
})();

if (isMain) {
  main().catch((err) => {
    console.error(err.message);
    process.exit(1);
  });
}
