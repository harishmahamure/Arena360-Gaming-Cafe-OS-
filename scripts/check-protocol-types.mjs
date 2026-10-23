#!/usr/bin/env node
// Protocol type lint: protocol types come only from `@arena/ui/generated`. Fails on hand-written
// declarations that reuse a protocol type name, and on imports of a protocol type from anywhere
// else. Reads local files only (no network).

import { existsSync, readdirSync, readFileSync, realpathSync } from "node:fs";
import { dirname, join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { GENERATED_DIR, parseBarrel } from "./gen-types.mjs";

export const REQUIRED_SPECIFIER = "@arena/ui/generated";
export const SCAN_ROOTS = ["apps", "packages"];
export const EXTENSIONS = [".ts", ".js", ".svelte", ".astro"];
/** Exact repo-relative folders that are not scanned. */
export const SKIP_PATHS = new Set([GENERATED_DIR]);
/** Folder names skipped wherever they appear. */
export const SKIP_DIRS = new Set([
  "node_modules",
  "build",
  "dist",
  ".svelte-kit",
  ".astro",
  ".wrangler",
  "target",
]);

const DECL =
  /^\s*(?:export\s+)?(?:default\s+)?(?:declare\s+)?(?:abstract\s+)?(?:const\s+)?(?:type|interface|enum|class)\s+([A-Za-z_$][\w$]*)/;
const IMPORT =
  /\b(?:import|export)\s+(?:type\s+)?((?:[\w$]+\s*,\s*)?\{[^}]*\}|\*\s*as\s+[\w$]+|[\w$]+)\s*from\s*(["'])([^"']+)\2/g;
const STAR = /\b(?:import|export)\s+(?:type\s+)?\*(?:\s*as\s+[\w$]+)?\s*from\s*(["'])([^"']+)\1/g;
const DYNAMIC_IMPORT = /\bimport\(\s*(["'])([^"']+)\1\s*\)\s*\.\s*([\w$]+)/g;

const lineAt = (text, index) => text.slice(0, index).split("\n").length;

/** Original (pre-`as`) names bound by an import clause such as `A, { type B as C }`. */
export function importedNames(clause) {
  const names = [];
  const braces = clause.match(/\{([^}]*)\}/);
  const head = clause
    .replace(/\{[^}]*\}/, "")
    .replace(/,/g, " ")
    .trim();
  if (head && !head.startsWith("*")) names.push(head);
  if (braces) {
    for (const part of braces[1].split(",")) {
      const name = part
        .trim()
        .replace(/^type\s+/, "")
        .split(/\s+as\s+/)[0]
        .trim();
      if (name) names.push(name);
    }
  }
  return names;
}

/** Whether `specifier` may supply protocol types to `file` (a repo-relative, `/`-separated path). */
export function isAllowedSpecifier(specifier, file) {
  if (specifier === REQUIRED_SPECIFIER) return true;
  return (
    file.startsWith("packages/ui/") &&
    /^\.{1,2}(?:\/\.\.)*\/generated(?:\/index(?:\.ts|\.js)?)?$/.test(specifier)
  );
}

/**
 * Lints one source file.
 * @param {string} file repo-relative path with `/` separators
 * @param {string} text file contents
 * @param {Set<string>} names protocol type names
 * @returns {Array<{file: string, line: number, message: string}>}
 */
export function checkSource(file, text, names) {
  const out = [];
  text.split(/\r?\n/).forEach((line, i) => {
    const m = line.match(DECL);
    if (m && names.has(m[1])) {
      out.push({ file, line: i + 1, message: `hand-written protocol type ${m[1]}` });
    }
  });
  const wrongImport = (index, name, specifier) =>
    out.push({
      file,
      line: lineAt(text, index),
      message: `protocol type ${name} must be imported from "${REQUIRED_SPECIFIER}" (found "${specifier}")`,
    });
  for (const m of text.matchAll(IMPORT)) {
    const specifier = m[3];
    if (isAllowedSpecifier(specifier, file)) continue;
    for (const name of importedNames(m[1])) {
      if (names.has(name)) wrongImport(m.index, name, specifier);
    }
  }
  for (const m of text.matchAll(STAR)) {
    const specifier = m[2];
    if (/generated/.test(specifier) && !isAllowedSpecifier(specifier, file)) {
      out.push({
        file,
        line: lineAt(text, m.index),
        message: `protocol types must be imported by name from "${REQUIRED_SPECIFIER}" (found star import/export of "${specifier}")`,
      });
    }
  }
  for (const m of text.matchAll(DYNAMIC_IMPORT)) {
    if (names.has(m[3]) && !isAllowedSpecifier(m[2], file)) wrongImport(m.index, m[3], m[2]);
  }
  return out.sort((a, b) => a.line - b.line);
}

export const formatViolation = (v) => `${v.file}:${v.line} ${v.message}`;

/** Repo-relative paths of every file to scan. */
export function listFiles(root, dirs = SCAN_ROOTS, exts = EXTENSIONS) {
  const files = [];
  const walk = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const path = join(dir, entry.name);
      const rel = relative(root, path).split(sep).join("/");
      if (entry.isDirectory()) {
        if (!SKIP_DIRS.has(entry.name) && !SKIP_PATHS.has(rel)) walk(path);
      } else if (exts.some((e) => entry.name.endsWith(e))) {
        files.push(rel);
      }
    }
  };
  for (const d of dirs) if (existsSync(join(root, d))) walk(join(root, d));
  return files.sort();
}

export function checkRepo(root) {
  const barrel = join(root, GENERATED_DIR, "index.ts");
  if (!existsSync(barrel)) return [`${GENERATED_DIR}/index.ts not found (run \`pnpm gen:types\`)`];
  const names = new Set(parseBarrel(readFileSync(barrel, "utf8")));
  return listFiles(root).flatMap((file) =>
    checkSource(file, readFileSync(join(root, file), "utf8"), names).map(formatViolation),
  );
}

function main() {
  const root = join(dirname(fileURLToPath(import.meta.url)), "..");
  const errors = checkRepo(root);
  if (errors.length > 0) {
    for (const e of errors) console.error(e);
    process.exit(1);
  }
  console.log("protocol type check: ok");
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
