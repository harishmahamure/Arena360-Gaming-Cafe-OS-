#!/usr/bin/env node
// Manifest gate (Story 8.3, AD-4): the cloud finds upload gaps from the heartbeat and a bucket
// listing, never from a manifest. No source file may name an upload-manifest endpoint, an
// upload-manifest table or an ingest cursor. This script and its test spell the patterns out and
// are exempt; planning documents are not scanned. Reads local files only (no network).

import { readFileSync, readdirSync, realpathSync } from "node:fs";
import { dirname, join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

export const PATTERNS = [
  { name: "manifest-endpoint", re: /\/uploads\/manifest/i },
  { name: "manifest-table", re: /upload[_ -]?manifest/i },
  { name: "ingest-cursor", re: /ingest[_ -]?cursor/i },
];
export const SKIPPED_DIRS = new Set([
  ".git",
  ".svelte-kit",
  "_bmad",
  "_bmad-output",
  "build",
  "dist",
  "docs",
  "node_modules",
  "target",
]);
export const SCANNED = [
  ".rs",
  ".sql",
  ".ts",
  ".js",
  ".mjs",
  ".svelte",
  ".toml",
  ".yaml",
  ".yml",
  ".json",
];
const SELF = new Set(["scripts/check-no-manifest.mjs", "scripts/check-no-manifest.test.mjs"]);

/** Every scanned file under `root`, as `/`-separated paths relative to it. */
export function listSources(root, dir = root) {
  const out = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.isDirectory()) {
      if (!SKIPPED_DIRS.has(entry.name)) out.push(...listSources(root, join(dir, entry.name)));
    } else if (SCANNED.some((ext) => entry.name.endsWith(ext))) {
      out.push(relative(root, join(dir, entry.name)).split(sep).join("/"));
    }
  }
  return out;
}

/**
 * Manifest violations in one file.
 * @returns {string[]} `file:line` followed by the pattern name
 */
export function checkSource(file, text) {
  if (SELF.has(file)) return [];
  const out = [];
  text.split("\n").forEach((line, i) => {
    for (const { name, re } of PATTERNS) {
      if (re.test(line)) out.push(`${file}:${i + 1} ${name}`);
    }
  });
  return out;
}

export function checkRepo(root) {
  return listSources(root).flatMap((file) =>
    checkSource(file, readFileSync(join(root, file), "utf8")),
  );
}

function main() {
  const root = join(dirname(fileURLToPath(import.meta.url)), "..");
  const errors = checkRepo(root);
  if (errors.length > 0) {
    for (const e of errors) console.error(e);
    process.exit(1);
  }
  console.log("manifest check: ok");
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
