#!/usr/bin/env node
// Remote-reference gate: branch surfaces (Hub, Kiosk, Kitchen) never load fonts, CDN assets or
// scripts at runtime. Scans their built output for remote references. Run after the build.
// Also checks each build bundles the base stylesheet (a local Sora woff2, CSS with --color-bg).
// Usage: node scripts/check-no-remote.mjs [app ...]   (default: hub kiosk kitchen)

import { existsSync, readdirSync, readFileSync, realpathSync } from "node:fs";
import { dirname, join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

export const APPS = ["hub", "kiosk", "kitchen"];
/** Built output folder per app (branch surfaces use `build/`). */
export const BUILD_DIRS = {
  owner: ".svelte-kit/cloudflare",
  partner: ".svelte-kit/cloudflare",
  admin: ".svelte-kit/cloudflare",
  onboarding: ".svelte-kit/cloudflare",
  marketing: "dist",
};
export const buildDir = (app) => `apps/${app}/${BUILD_DIRS[app] ?? "build"}`;
export const TEXT_EXTENSIONS = [
  ".html",
  ".css",
  ".js",
  ".mjs",
  ".json",
  ".svg",
  ".webmanifest",
  ".txt",
];

const URL_CHARS = String.raw`[^\s"'\`()<>]+`;
/** A remote URL, including protocol-relative `//host` and WebSocket schemes. */
const REMOTE = String.raw`((?:https?:|wss?:)?\/\/${URL_CHARS})`;
const rx = (source) => new RegExp(source, "gi");
const PATTERNS = [
  { re: rx(String.raw`url\(\s*["']?${REMOTE}`), what: "url()" },
  { re: rx(String.raw`@import\s+(?:url\(\s*)?["']?${REMOTE}`), what: "@import" },
  { re: rx(String.raw`<link\b[^>]*?\bhref\s*=\s*["']?${REMOTE}`), what: "<link>" },
  {
    re: rx(
      String.raw`<(script|img|iframe|source|video|audio|embed)\b[^>]*?\bsrc\s*=\s*["']?${REMOTE}`,
    ),
    what: "src",
  },
  { re: rx(String.raw`\bimport\(\s*["'\`]${REMOTE}`), what: "import()" },
  { re: rx(String.raw`\bnew\s+(?:Shared)?Worker\(\s*["'\`]${REMOTE}`), what: "Worker" },
];
const SRCSET = /\bsrcset\s*=\s*(["'])([^"']*)\1/gi;
const ANY_URL = new RegExp(
  String.raw`(?:https?:|wss?:)?\/\/([a-z0-9.-]+\.[a-z]{2,})(?:[:/?#]${URL_CHARS}|(?![\w.-]))`,
  "gi",
);
const BANNED_HOSTS = [/fonts\.googleapis/i, /fonts\.gstatic/i, /(^|[.-])cdn[\w-]*([.-]|$)/i];

/**
 * Remote references in one built file.
 * @returns {string[]} `file: <kind> <url>`
 */
export function checkSource(file, text) {
  const found = new Map();
  for (const { re, what } of PATTERNS) {
    for (const m of text.matchAll(re)) {
      const url = m[m.length - 1];
      found.set(url, what === "src" ? `<${m[1].toLowerCase()}>` : what);
    }
  }
  for (const m of text.matchAll(SRCSET)) {
    for (const candidate of m[2].split(",")) {
      const url = candidate.trim().split(/\s+/)[0];
      if (/^(?:https?:|wss?:)?\/\//i.test(url)) found.set(url, "srcset");
    }
  }
  for (const m of text.matchAll(ANY_URL)) {
    if (BANNED_HOSTS.some((h) => h.test(m[1])) && !found.has(m[0])) found.set(m[0], "remote host");
  }
  for (const m of text.matchAll(/fonts\.(?:googleapis|gstatic)\.com[^\s"'`()<>]*/gi)) {
    if (![...found.keys()].some((u) => u.includes(m[0]))) found.set(m[0], "remote host");
  }
  return [...found].map(([url, what]) => `${file}: ${what} ${url}`);
}

/** Every file under `dir` (repo-relative paths). */
export function listAllFiles(root, dir) {
  const files = [];
  const walk = (d) => {
    for (const entry of readdirSync(d, { withFileTypes: true })) {
      const path = join(d, entry.name);
      if (entry.isDirectory()) walk(path);
      else files.push(relative(root, path).split(sep).join("/"));
    }
  };
  walk(join(root, dir));
  return files.sort();
}

export function listFiles(root, dir) {
  return listAllFiles(root, dir).filter((f) => TEXT_EXTENSIONS.some((e) => f.endsWith(e)));
}

/**
 * The bundle carries the base stylesheet: a local Sora woff2 and CSS declaring `--color-bg`.
 * @returns {string[]} error messages
 */
export function checkBundled(dir, files, read) {
  const errors = [];
  if (!files.some((f) => /\/sora-[^/]*\.woff2$/.test(f))) {
    errors.push(`${dir}: no bundled sora-*.woff2 asset (is @arena/ui/base.css imported?)`);
  }
  const styled = files
    .filter((f) => f.endsWith(".css") || f.endsWith(".html"))
    .some((f) => /--color-bg\s*:/.test(read(f)));
  if (!styled) errors.push(`${dir}: no CSS declares --color-bg (is @arena/ui/base.css imported?)`);
  return errors;
}

export function checkRepo(root, apps = APPS) {
  const errors = [];
  for (const app of apps) {
    const dir = buildDir(app);
    if (!existsSync(join(root, dir))) {
      errors.push(`${dir}: missing build output (run \`pnpm --filter @arena/${app} build\` first)`);
      continue;
    }
    const all = listAllFiles(root, dir);
    if (all.length === 0) {
      errors.push(`${dir}: build output is empty`);
      continue;
    }
    const read = (file) => readFileSync(join(root, file), "utf8");
    for (const file of all.filter((f) => TEXT_EXTENSIONS.some((e) => f.endsWith(e)))) {
      errors.push(...checkSource(file, read(file)));
    }
    errors.push(...checkBundled(dir, all, read));
  }
  return errors;
}

function main() {
  const root = join(dirname(fileURLToPath(import.meta.url)), "..");
  const apps = process.argv.slice(2);
  const errors = checkRepo(root, apps.length > 0 ? apps : APPS);
  if (errors.length > 0) {
    for (const e of errors) console.error(e);
    process.exit(1);
  }
  console.log("no-remote check: ok");
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
