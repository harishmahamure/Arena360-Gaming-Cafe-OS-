#!/usr/bin/env node
// Tier-lock gate (Story 10.5): billing is on, so every `tier-lock` shows a live "Upgrade" link.
// Fails when the shared billing flag is off, when a source still names the "Available soon"
// placeholder key, when a `<TierLock>` is given the old `billing` switch, or when the component
// no longer renders its Upgrade link. This script and its test are exempt. Reads local files only.

import { existsSync, readFileSync, readdirSync, realpathSync } from "node:fs";
import { dirname, join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

export const FLAG_FILE = "packages/ui/src/billing.js";
export const COMPONENT = "packages/ui/src/components/TierLock.svelte";
export const SOON_KEY = "staff.tier.soon";
const SCANNED = [".svelte", ".ts", ".js"];
const SKIPPED = new Set([".svelte-kit", "build", "dist", "node_modules", "generated", "messages"]);
const SELF = new Set(["scripts/check-tier-lock.mjs", "scripts/check-tier-lock.test.mjs"]);

/** Source files under `apps/<app>/src` and `packages/ui/src`, as `/`-separated paths from `root`. */
export function listSources(root) {
  const out = [];
  const walk = (dir) => {
    if (!existsSync(dir)) return;
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const path = join(dir, entry.name);
      if (entry.isDirectory()) {
        if (!SKIPPED.has(entry.name)) walk(path);
      } else if (SCANNED.some((ext) => entry.name.endsWith(ext))) {
        out.push(relative(root, path).split(sep).join("/"));
      }
    }
  };
  const apps = join(root, "apps");
  if (existsSync(apps)) {
    for (const app of readdirSync(apps, { withFileTypes: true })) {
      if (app.isDirectory()) walk(join(apps, app.name, "src"));
    }
  }
  walk(join(root, "packages", "ui", "src"));
  return out;
}

/** Violations in one source file. */
export function checkSource(file, text) {
  if (SELF.has(file)) return [];
  const out = [];
  text.split("\n").forEach((line, i) => {
    if (line.includes(SOON_KEY)) out.push(`${file}:${i + 1} available-soon-placeholder`);
    if (/<TierLock\b[^>]*\sbilling\b/.test(line)) out.push(`${file}:${i + 1} billing-switch`);
  });
  return out;
}

export function checkFlag(text) {
  return /export\s+const\s+billingEnabled\s*=\s*true\s*;/.test(text)
    ? []
    : [`${FLAG_FILE} billingEnabled must be true`];
}

export function checkComponent(text) {
  return /<a\b[^>]*class="upgrade"/.test(text) ? [] : [`${COMPONENT} must render its Upgrade link`];
}

export function checkRepo(root) {
  const read = (file) =>
    existsSync(join(root, file)) ? readFileSync(join(root, file), "utf8") : "";
  return [
    ...checkFlag(read(FLAG_FILE)),
    ...checkComponent(read(COMPONENT)),
    ...listSources(root).flatMap((file) => checkSource(file, read(file))),
  ];
}

function main() {
  const root = join(dirname(fileURLToPath(import.meta.url)), "..");
  const errors = checkRepo(root);
  if (errors.length > 0) {
    for (const e of errors) console.error(e);
    process.exit(1);
  }
  console.log("tier-lock check: ok");
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
