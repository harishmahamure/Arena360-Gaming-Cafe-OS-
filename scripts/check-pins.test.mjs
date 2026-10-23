import { test } from "node:test";
import assert from "node:assert/strict";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  checkManifestDeps,
  checkRepo,
  comparePins,
  parsePinTable,
  readCargoWorkspaceDeps,
  readNvmrc,
  readPackageJson,
  readPnpmCatalog,
  readRustToolchain,
} from "./check-pins.mjs";

const README = `# Repo

## Pins

| Item | Version | Source |
| --- | --- | --- |
| rust | 1.95.0 | \`rust-toolchain.toml\` |
| vite | 8.3.5 | \`pnpm-workspace.yaml\` |
| tauri | =2.12.5 | \`Cargo.toml\` |

After the table.
`;

const sources = {
  "rust-toolchain.toml": { rust: "1.95.0" },
  "pnpm-workspace.yaml": { vite: "8.3.5" },
  "Cargo.toml": { tauri: "=2.12.5" },
};

test("parses the pin table and strips backticks", () => {
  assert.deepEqual(parsePinTable(README), [
    { item: "rust", version: "1.95.0", source: "rust-toolchain.toml" },
    { item: "vite", version: "8.3.5", source: "pnpm-workspace.yaml" },
    { item: "tauri", version: "=2.12.5", source: "Cargo.toml" },
  ]);
});

test("in-sync table has no errors", () => {
  assert.deepEqual(comparePins(parsePinTable(README), sources), []);
});

test("silently bumped pin names item, README value and manifest value", () => {
  const bumped = { ...sources, "pnpm-workspace.yaml": { vite: "8.4.0" } };
  const errors = comparePins(parsePinTable(README), bumped);
  assert.equal(errors.length, 1);
  assert.match(errors[0], /vite/);
  assert.match(errors[0], /README 8\.3\.5/);
  assert.match(errors[0], /8\.4\.0/);
});

test("deleted row reports missing pin row", () => {
  const readme = README.replace(/^\| vite .*\n/m, "");
  assert.deepEqual(comparePins(parsePinTable(readme), sources), [
    "missing pin row: vite (pnpm-workspace.yaml 8.3.5)",
  ]);
});

test("row without a manifest entry reports extra pin row", () => {
  const readme = README.replace(
    "| tauri |",
    "| svelte | 5.57.2 | `pnpm-workspace.yaml` |\n| tauri |",
  );
  const errors = comparePins(parsePinTable(readme), sources);
  assert.equal(errors.length, 1);
  assert.match(errors[0], /^extra pin row: svelte/);
});

test("duplicate row is reported", () => {
  const readme = README.replace("| tauri |", "| vite | 8.3.5 | `pnpm-workspace.yaml` |\n| tauri |");
  assert.deepEqual(comparePins(parsePinTable(readme), sources), [
    "duplicate pin row: vite (pnpm-workspace.yaml)",
  ]);
});

test("non-exact catalog and Cargo pins are rejected", () => {
  const ranged = {
    ...sources,
    "pnpm-workspace.yaml": { vite: "^8.3.5" },
    "Cargo.toml": { tauri: "2.12.5" },
  };
  const table = [
    { item: "rust", version: "1.95.0", source: "rust-toolchain.toml" },
    { item: "vite", version: "^8.3.5", source: "pnpm-workspace.yaml" },
    { item: "tauri", version: "2.12.5", source: "Cargo.toml" },
  ];
  assert.deepEqual(comparePins(table, ranged), [
    "non-exact pin: vite (pnpm-workspace.yaml ^8.3.5)",
    "non-exact pin: tauri (Cargo.toml 2.12.5)",
  ]);
});

test("malformed rows fail with the offending line", () => {
  const short = README.replace("| vite | 8.3.5 | `pnpm-workspace.yaml` |", "| vite | 8.3.5 |");
  assert.throws(() => parsePinTable(short), { message: "malformed pin row: | vite | 8.3.5 |" });
  const empty = README.replace("| vite |", "|  |");
  assert.throws(() => parsePinTable(empty), /^Error: malformed pin row: /);
});

test("missing table header fails", () => {
  const readme = README.replace("| Item | Version | Source |\n", "");
  assert.throws(() => parsePinTable(readme), { message: "pin table not found in README.md" });
});

test("CRLF line endings are accepted", () => {
  assert.equal(parsePinTable(README.replace(/\n/g, "\r\n")).length, 3);
});

test("manifest readers", () => {
  assert.deepEqual(readRustToolchain('[toolchain]\nchannel = "1.95.0"\n'), { rust: "1.95.0" });
  assert.deepEqual(readNvmrc("22.23.4\n"), { node: "22.23.4" });
  assert.deepEqual(
    readPackageJson(
      JSON.stringify({ packageManager: "pnpm@12.15.0+sha512.abc", engines: { node: ">=22 <23" } }),
    ),
    { pnpm: "12.15.0", "node engines": ">=22 <23" },
  );
  assert.deepEqual(
    readPnpmCatalog(
      'packages:\n  - "apps/*"\ncatalog:\n  svelte: 5.57.2\n  "@sveltejs/kit": 3.1.0\n  # note\nother:\n  x: 1\n',
    ),
    { svelte: "5.57.2", "@sveltejs/kit": "3.1.0" },
  );
  assert.deepEqual(
    readCargoWorkspaceDeps(
      '[workspace]\nmembers = ["crates/*"]\n[workspace.dependencies]\narena-proto = { path = "crates/arena-proto" }\ntauri = "=2.12.5"\nfoo = { version = "=1.0.0", features = ["x"] }\n[profile.release]\nlto = true\n',
    ),
    { tauri: "=2.12.5", foo: "=1.0.0" },
  );
});

test("manifest deps must use catalog: and workspace:*", () => {
  const catalog = { svelte: "5.57.2", vite: "8.3.5" };
  const manifests = [
    {
      file: "apps/a/package.json",
      pkg: {
        dependencies: { "@arena/ui": "workspace:*" },
        devDependencies: { svelte: "catalog:", prettier: "3.9.10" },
      },
    },
    {
      file: "apps/b/package.json",
      pkg: {
        dependencies: { "@arena/ui": "^0.0.0" },
        devDependencies: { vite: "8.3.5" },
        peerDependencies: { svelte: "^5.0.0" },
      },
    },
  ];
  assert.deepEqual(checkManifestDeps(manifests, catalog), [
    "non-workspace dependency: @arena/ui in apps/b/package.json (^0.0.0)",
    "non-catalog dependency: vite in apps/b/package.json (8.3.5)",
    "non-catalog dependency: svelte in apps/b/package.json (^5.0.0)",
  ]);
});

test("the real repo passes", () => {
  const root = join(dirname(fileURLToPath(import.meta.url)), "..");
  assert.deepEqual(checkRepo(root), []);
});

test("pinned root devDependencies are read and must be exact", () => {
  const pkg = readPackageJson(
    JSON.stringify({ devDependencies: { yaml: "2.9.1", prettier: "3.9.10" } }),
  );
  assert.deepEqual(pkg, { yaml: "2.9.1" });
  const ranged = { ...sources, "package.json": { yaml: "^2.9.1" } };
  const table = [
    { item: "rust", version: "1.95.0", source: "rust-toolchain.toml" },
    { item: "yaml", version: "^2.9.1", source: "package.json" },
  ];
  assert.ok(comparePins(table, ranged).includes("non-exact pin: yaml (package.json ^2.9.1)"));
});
