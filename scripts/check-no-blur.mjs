#!/usr/bin/env node
// Blur gate: branch surfaces (Hub, Kiosk, Kitchen) and the shared UI package never use
// `backdrop-filter` (no live blur or glass; media sits under a solid scrim). Exempt a line with a
// `blur-ignore` comment. Reads local files only (no network).

import { readFileSync, realpathSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { EXTENSIONS, listFiles } from "./check-tokens-used.mjs";

export const IGNORE_MARKER = "blur-ignore";
export const BRANCH_SURFACES = ["hub", "kiosk", "kitchen"];
const SCANNED = [...EXTENSIONS, ".html"];
const BACKDROP = /backdrop-filter/i;
const COMMENT = /^\s*(\/\/|\/\*|\*|<!--)/;

/** True for files in a branch surface or the shared UI package. */
export function inScope(file) {
  return (
    file.startsWith("packages/ui/src/") ||
    BRANCH_SURFACES.some((app) => file.startsWith(`apps/${app}/src/`))
  );
}

/**
 * `backdrop-filter` uses in one file (comment lines are skipped).
 * @returns {string[]} `file:line backdrop-filter`
 */
export function checkSource(file, text) {
  const out = [];
  text.split("\n").forEach((line, i) => {
    if (line.includes(IGNORE_MARKER) || COMMENT.test(line)) return;
    if (BACKDROP.test(line)) out.push(`${file}:${i + 1} backdrop-filter`);
  });
  return out;
}

export function checkRepo(root) {
  return listFiles(root, SCANNED)
    .filter(inScope)
    .flatMap((file) => checkSource(file, readFileSync(join(root, file), "utf8")));
}

function main() {
  const root = join(dirname(fileURLToPath(import.meta.url)), "..");
  const errors = checkRepo(root);
  if (errors.length > 0) {
    for (const e of errors) console.error(e);
    process.exit(1);
  }
  console.log("blur check: ok");
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
  main();
}
