// Branch surfaces (Hub, Kiosk, Kitchen) are dark + high contrast only: no theme control and no
// light theme. High contrast is set per device by a manager (Story 1.21), never by the player
// and never from the OS. This test fails on a `theme-toggle`, a `data-theme="light"` (or the
// same through `dataset.theme`) or a `ThemeToggle` import in their sources.

import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

/** The branch apps whose `src` is checked. */
export const BRANCH_APPS = ["hub", "kiosk", "kitchen"];
const EXTENSIONS = [".svelte", ".ts", ".js", ".html", ".css", ".json"];
const SKIP_DIRS = new Set(["node_modules", "build", ".svelte-kit"]);

/** What may not appear, with a name for the failure message. */
export const FORBIDDEN = [
  ["a theme-toggle", /theme-toggle/i],
  ["a light theme", /data-theme\s*=\s*\{?\s*["'`]light["'`]/i],
  ["a light theme", /dataset\.theme\s*=\s*["'`]light["'`]/i],
  ["a ThemeToggle import", /\bimport\b[^;]*\bThemeToggle\b/],
];

function* files(dir) {
  let entries;
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch {
    return;
  }
  for (const e of entries) {
    const path = join(dir, e.name);
    if (e.isDirectory()) {
      if (!SKIP_DIRS.has(e.name)) yield* files(path);
    } else if (EXTENSIONS.some((x) => e.name.endsWith(x))) {
      yield path;
    }
  }
}

/** Every forbidden match under `base`'s branch apps, as `file: what`. */
export function findThemeControls(base) {
  const found = [];
  for (const app of BRANCH_APPS) {
    for (const file of files(join(base, "apps", app, "src"))) {
      const text = readFileSync(file, "utf8");
      for (const [what, pattern] of FORBIDDEN) {
        if (pattern.test(text)) found.push(`${relative(base, file)}: ${what}`);
      }
    }
  }
  return found;
}

test("Hub, Kiosk and Kitchen have no theme control and no light theme", () => {
  assert.deepEqual(findThemeControls(root), []);
});

test("the check fails on a theme-toggle, a light theme or a ThemeToggle import", () => {
  const base = mkdtempSync(join(tmpdir(), "no-theme-toggle-"));
  try {
    const write = (app, name, text) => {
      const dir = join(base, "apps", app, "src");
      mkdirSync(dir, { recursive: true });
      writeFileSync(join(dir, name), text);
    };
    write("hub", "ok.svelte", '<html data-theme="hc"></html>');
    assert.deepEqual(findThemeControls(base), []);
    write("hub", "a.svelte", '<div data-component="theme-toggle"></div>');
    write("kiosk", "b.svelte", '<section data-theme="light"></section>');
    write("kitchen", "c.ts", 'import ThemeToggle from "./ThemeToggle.svelte";');
    write("kitchen", "d.ts", 'document.documentElement.dataset.theme = "light";');
    // Not a branch app: the owner PWA may have both.
    write("owner", "e.svelte", '<div data-component="theme-toggle" data-theme="light"></div>');
    assert.deepEqual(findThemeControls(base).sort(), [
      join("apps", "hub", "src", "a.svelte") + ": a theme-toggle",
      join("apps", "kiosk", "src", "b.svelte") + ": a light theme",
      join("apps", "kitchen", "src", "c.ts") + ": a ThemeToggle import",
      join("apps", "kitchen", "src", "d.ts") + ": a light theme",
    ]);
  } finally {
    rmSync(base, { recursive: true, force: true });
  }
});
