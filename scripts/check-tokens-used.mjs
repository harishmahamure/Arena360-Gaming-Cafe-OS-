#!/usr/bin/env node
// Unknown-token gate: every `var(--name)` in app and UI sources must be declared in the
// generated tokens.css or in the same file. Reads local files only (no network).

import { readdirSync, readFileSync, realpathSync } from "node:fs";
import { dirname, join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { scanRoots } from "./check-strings.mjs";
import { TOKENS_DIR } from "./gen-tokens.mjs";

export const TOKENS_CSS = `${TOKENS_DIR}/tokens.css`;
export const EXTENSIONS = [".svelte", ".css", ".ts", ".js", ".astro"];
/** Exact repo-relative folders that are not scanned. */
export const SKIP_PATHS = new Set([TOKENS_DIR]);
/** Folder names skipped wherever they appear. */
export const SKIP_DIRS = new Set(["node_modules", "build", "dist", ".svelte-kit", ".astro"]);

// A name followed by `${` is built in a template literal (`var(--color-${x})`) and is skipped.
const USE = /var\(\s*(--[\w-]+)(?![\w-]|\$\{)/g;
const DECLARE = /(?:^|[\s{;"'`(])(--[\w-]+)\s*:|style:(--[\w-]+)|setProperty\(\s*["'`](--[\w-]+)/g;

const lineOf = (text, index) => text.slice(0, index).split("\n").length;

/** Custom property names declared in CSS-like text (`--x:`, Svelte `style:--x`, `setProperty`). */
export function declaredNames(text) {
  const names = new Set();
  for (const m of text.matchAll(DECLARE)) names.add(m[1] ?? m[2] ?? m[3]);
  return names;
}

/**
 * Unknown `var(--name)` uses in one file.
 * @param {string} file repo-relative path
 * @param {string} text file contents
 * @param {Set<string>} known names declared in tokens.css
 * @returns {string[]} `file:line unknown token --name`
 */
export function checkSource(file, text, known) {
  const local = declaredNames(text);
  const out = [];
  for (const m of text.matchAll(USE)) {
    if (known.has(m[1]) || local.has(m[1])) continue;
    out.push(`${file}:${lineOf(text, m.index)} unknown token ${m[1]}`);
  }
  return out;
}

/** Scanned source files under `apps/*\/src` and `packages/ui/src`, repo-relative. */
export function listFiles(root, extensions = EXTENSIONS) {
  const files = [];
  const walk = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const path = join(dir, entry.name);
      const rel = relative(root, path).split(sep).join("/");
      if (entry.isDirectory()) {
        if (!SKIP_DIRS.has(entry.name) && !SKIP_PATHS.has(rel)) walk(path);
      } else if (extensions.some((e) => entry.name.endsWith(e))) {
        files.push(rel);
      }
    }
  };
  for (const d of scanRoots(root)) walk(join(root, d));
  return files.sort();
}

export function checkRepo(root) {
  const known = declaredNames(readFileSync(join(root, TOKENS_CSS), "utf8"));
  return listFiles(root).flatMap((file) =>
    checkSource(file, readFileSync(join(root, file), "utf8"), known),
  );
}

function main() {
  const root = join(dirname(fileURLToPath(import.meta.url)), "..");
  const errors = checkRepo(root);
  if (errors.length > 0) {
    for (const e of errors) console.error(e);
    process.exit(1);
  }
  console.log("tokens used check: ok");
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
