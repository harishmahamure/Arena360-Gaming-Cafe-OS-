import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const IMPORT = /^\s*import\s+["']@arena\/ui\/base\.css["'];?\s*$/m;

/** The root entry of each app: SvelteKit's root layout, or the Astro layout. */
export function rootEntry(app) {
  return app === "marketing"
    ? `apps/${app}/src/layouts/Layout.astro`
    : `apps/${app}/src/routes/+layout.svelte`;
}

test("every app's root layout (or Astro page) imports @arena/ui/base.css", () => {
  const apps = readdirSync(join(root, "apps"), { withFileTypes: true })
    .filter((e) => e.isDirectory() && existsSync(join(root, "apps", e.name, "src")))
    .map((e) => e.name);
  assert.ok(apps.length >= 8, apps.join(", "));
  for (const app of apps) {
    const file = rootEntry(app);
    assert.ok(existsSync(join(root, file)), `missing ${file}`);
    assert.match(readFileSync(join(root, file), "utf8"), IMPORT, `${file} must import base.css`);
  }
});

test("SvelteKit root layouts render their children", () => {
  for (const app of readdirSync(join(root, "apps"))) {
    const file = join(root, rootEntry(app));
    if (app === "marketing" || !existsSync(file)) continue;
    assert.match(readFileSync(file, "utf8"), /\{@render children\(\)\}/, file);
  }
});
