#!/usr/bin/env node
// String lint: user-visible text comes from the message catalogue
// (packages/ui/src/messages/en.json). Fails on hard-coded strings and on unknown `t()` keys.
// `.astro` files are checked for unknown keys only. Exempt a line with `// i18n-ignore`, or put `<!-- i18n-ignore -->` on the line before.
// Reads local files only (no network).

import { existsSync, readdirSync, readFileSync, realpathSync } from "node:fs";
import { dirname, join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { parse } from "svelte/compiler";

export const CATALOGUE = "packages/ui/src/messages/en.json";
export const EXTENSIONS = [".svelte", ".ts", ".js", ".astro"];
/** Exact repo-relative folders that are not scanned. */
export const SKIP_PATHS = new Set([
  "packages/ui/src/generated",
  "packages/ui/src/messages",
  "packages/ui/src/tokens",
]);
/** Folder names skipped wherever they appear. */
export const SKIP_DIRS = new Set(["node_modules", "build", "dist", ".svelte-kit"]);
export const TEXT_ATTRIBUTES = new Set(["title", "alt", "placeholder", "aria-label", "label"]);

const HAS_LETTER = /\p{L}/u;
const collapse = (s) => s.replace(/\s+/g, " ").trim();

/** Line numbers (1-based) exempted by an ignore marker. */
export function ignoredLines(text) {
  const lines = text.split(/\r?\n/);
  const ignored = new Set();
  lines.forEach((line, i) => {
    if (line.includes("// i18n-ignore") || line.includes("<!-- i18n-ignore -->")) {
      ignored.add(i + 1);
    }
    if (line.includes("<!-- i18n-ignore -->")) ignored.add(i + 2);
  });
  return ignored;
}

/** Whether a string literal reads like user-visible prose. */
export function looksLikeProse(value) {
  const s = value.trim();
  return /^\p{L}/u.test(s) && /\s/.test(s) && s.split(/\s+/).length >= 2;
}

const REGEX_PRECEDERS = new Set([..."(,=:[!&|?{};+-*%<>~^"]);
const REGEX_KEYWORDS = /(?:^|[^\w$])(?:return|typeof|case|do|else|in|of|void|yield|await)$/;

/**
 * Finds string and template literals in JS/TS source (comments and regex literals skipped).
 * Template literals are reported by their static text; `${}` parts are scanned recursively.
 * @returns {Array<{value: string, index: number, end: number}>}
 */
export function scanStringLiterals(src, offset = 0) {
  const out = [];
  let i = 0;
  let prev = ""; // last significant char
  let prevWord = "";
  const n = src.length;
  while (i < n) {
    const c = src[i];
    if (c === "/" && src[i + 1] === "/") {
      while (i < n && src[i] !== "\n") i++;
      continue;
    }
    if (c === "/" && src[i + 1] === "*") {
      const end = src.indexOf("*/", i + 2);
      i = end === -1 ? n : end + 2;
      continue;
    }
    if (c === '"' || c === "'") {
      const start = i;
      let value = "";
      i++;
      while (i < n && src[i] !== c && src[i] !== "\n") {
        if (src[i] === "\\") {
          value += src[i + 1] ?? "";
          i += 2;
        } else value += src[i++];
      }
      i++;
      out.push({ value, index: start + offset, end: i + offset });
      prev = c;
      continue;
    }
    if (c === "`") {
      const start = i;
      let value = "";
      i++;
      while (i < n && src[i] !== "`") {
        if (src[i] === "\\") {
          value += src[i + 1] ?? "";
          i += 2;
        } else if (src[i] === "$" && src[i + 1] === "{") {
          let depth = 1;
          const exprStart = i + 2;
          i += 2;
          while (i < n && depth > 0) {
            if (src[i] === "{") depth++;
            else if (src[i] === "}") depth--;
            if (depth > 0) i++;
          }
          out.push(...scanStringLiterals(src.slice(exprStart, i), offset + exprStart));
          value += " x ";
          i++;
        } else value += src[i++];
      }
      i++;
      out.push({ value, index: start + offset, end: i + offset });
      prev = "`";
      continue;
    }
    if (c === "/") {
      const isRegex =
        prev === "" || REGEX_PRECEDERS.has(prev) || REGEX_KEYWORDS.test(prevWord.trimEnd());
      if (isRegex) {
        i++;
        let inClass = false;
        while (i < n && src[i] !== "\n") {
          if (src[i] === "\\") i += 2;
          else if (src[i] === "[") ((inClass = true), i++);
          else if (src[i] === "]") ((inClass = false), i++);
          else if (src[i] === "/" && !inClass) break;
          else i++;
        }
        i++;
        while (i < n && /[a-z]/i.test(src[i])) i++;
        prev = "/";
        prevWord = "";
        continue;
      }
    }
    if (!/\s/.test(c)) {
      prev = c;
      prevWord = /[\w$]/.test(c) ? prevWord + c : "";
    } else if (prevWord) {
      prevWord += " ";
      prevWord = prevWord.slice(-20);
    }
    i++;
  }
  return out;
}

const isModuleSpecifier = (src, index, offset) => {
  const before = src.slice(Math.max(0, index - offset - 30), index - offset);
  return /(?:\bfrom|\bimport|\bimport\s*\(|\brequire\s*\()\s*$/.test(before);
};

const lineOf = (text, index) => text.slice(0, index).split("\n").length;

/** Hard-coded prose string literals in JS/TS source. `text` is the whole file for line numbers. */
function scriptViolations(file, text, src, offset) {
  const out = [];
  for (const lit of scanStringLiterals(src, offset)) {
    if (!looksLikeProse(lit.value)) continue;
    if (isModuleSpecifier(src, lit.index, offset)) continue;
    out.push({
      file,
      line: lineOf(text, lit.index),
      message: `hard-coded string "${collapse(lit.value)}"`,
    });
  }
  return out;
}

const KEY_SHAPE = /^[a-z][a-z0-9_]*(\.[a-z0-9_]+)+$/;

function svelteViolations(file, text, keys) {
  const out = [];
  const ast = parse(text, { modern: true });
  const textAt = (index, value) =>
    out.push({
      file,
      line: lineOf(text, index),
      message: `hard-coded string "${collapse(value)}"`,
    });
  // `owner` is the element whose attribute list is being walked.
  const walk = (node, owner) => {
    if (!node || typeof node !== "object") return;
    if (Array.isArray(node)) return node.forEach((n) => walk(n, owner));
    if (node.type === "Text" && HAS_LETTER.test(node.data)) {
      textAt(node.start + node.raw.search(/\S/), node.data);
      return;
    }
    if (node.type === "Attribute") {
      if (TEXT_ATTRIBUTES.has(node.name.toLowerCase()) && Array.isArray(node.value)) {
        for (const part of node.value) {
          if (part.type !== "Text" || !HAS_LETTER.test(part.data)) continue;
          // A component prop such as `<TextField label="gallery.field.name">` names a catalogue
          // key. On a native element the browser would render the raw key, so it still fails.
          const isComponent = /^[A-Z]/.test(owner?.name ?? "");
          if (isComponent && node.value.length === 1 && KEY_SHAPE.test(part.data)) {
            if (!keys.has(part.data)) {
              out.push({
                file,
                line: lineOf(text, part.start),
                message: `unknown message key ${part.data}`,
              });
            }
            continue;
          }
          textAt(part.start, part.data);
        }
      }
      // Other attributes' literal text (class names, ids, ...) is not prose; expressions are.
      const parts = Array.isArray(node.value) ? node.value : [node.value];
      walk(parts.filter((p) => p && p.type === "ExpressionTag"));
      return;
    }
    if (node.type === "ExpressionTag" || node.type === "MustacheTag") {
      out.push(...scriptViolations(file, text, text.slice(node.start, node.end), node.start));
      return;
    }
    for (const [key, value] of Object.entries(node)) {
      if (key === "parent" || key === "expression" || key === "metadata") continue;
      if (value && typeof value === "object") walk(value, key === "attributes" ? node : undefined);
    }
  };
  walk(ast.fragment);
  for (const script of [ast.instance, ast.module]) {
    if (!script) continue;
    const { start, end } = script.content;
    out.push(...scriptViolations(file, text, text.slice(start, end), start));
  }
  return out;
}

/** Literal `t("…")` / `t('…')` keys with their offsets. */
export function messageKeys(text) {
  return [...text.matchAll(/(?<![\w$.])t\(\s*(["'])([^"'\\\n]+)\1/g)].map((m) => ({
    key: m[2],
    index: m.index,
  }));
}

/**
 * Lints one source file.
 * @param {string} file repo-relative path with `/` separators
 * @param {string} text file contents
 * @param {Set<string>} keys catalogue keys
 * @returns {Array<{file: string, line: number, message: string}>}
 */
export function checkSource(file, text, keys) {
  let found;
  try {
    // `.astro` files are checked for unknown keys only.
    found = file.endsWith(".astro")
      ? []
      : file.endsWith(".svelte")
        ? svelteViolations(file, text, keys)
        : scriptViolations(file, text, text, 0);
  } catch (err) {
    return [
      {
        file,
        line: err.start?.line ?? (err.position ? lineOf(text, err.position[0]) : 1),
        message: `cannot parse: ${err.message}`,
      },
    ];
  }
  for (const { key, index } of messageKeys(text)) {
    if (!keys.has(key)) {
      found.push({ file, line: lineOf(text, index), message: `unknown message key ${key}` });
    }
  }
  const ignored = ignoredLines(text);
  return found.filter((v) => !ignored.has(v.line)).sort((a, b) => a.line - b.line);
}

export const formatViolation = (v) => `${v.file}:${v.line} ${v.message}`;

/** `apps/*\/src` and `packages/ui/src`, relative to root. */
export function scanRoots(root) {
  const roots = [];
  if (existsSync(join(root, "apps"))) {
    for (const entry of readdirSync(join(root, "apps"), { withFileTypes: true })) {
      if (entry.isDirectory() && existsSync(join(root, "apps", entry.name, "src"))) {
        roots.push(`apps/${entry.name}/src`);
      }
    }
  }
  if (existsSync(join(root, "packages/ui/src"))) roots.push("packages/ui/src");
  return roots.sort();
}

export function listFiles(root) {
  const files = [];
  const walk = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const path = join(dir, entry.name);
      const rel = relative(root, path).split(sep).join("/");
      if (entry.isDirectory()) {
        if (!SKIP_DIRS.has(entry.name) && !SKIP_PATHS.has(rel)) walk(path);
      } else if (EXTENSIONS.some((e) => entry.name.endsWith(e))) {
        files.push(rel);
      }
    }
  };
  for (const d of scanRoots(root)) walk(join(root, d));
  return files.sort();
}

export function checkRepo(root) {
  const keys = new Set(Object.keys(JSON.parse(readFileSync(join(root, CATALOGUE), "utf8"))));
  return listFiles(root).flatMap((file) =>
    checkSource(file, readFileSync(join(root, file), "utf8"), keys).map(formatViolation),
  );
}

function main() {
  const root = join(dirname(fileURLToPath(import.meta.url)), "..");
  const errors = checkRepo(root);
  if (errors.length > 0) {
    for (const e of errors) console.error(e);
    process.exit(1);
  }
  console.log("string check: ok");
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
