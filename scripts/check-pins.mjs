#!/usr/bin/env node
// Pin table check: the README pin table must match the manifests exactly.
// Reads local files only (no network).

import { existsSync, readdirSync, readFileSync, realpathSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const lines = (text) => text.split(/\r?\n/);
const unquote = (s) => s.trim().replace(/^(['"])(.*)\1$/, "$2");

/**
 * Parses the `| Item | Version | Source |` table from README text.
 * @returns {Array<{item: string, version: string, source: string}>}
 */
export function parsePinTable(readme) {
  const all = lines(readme);
  const header = all.findIndex((l) =>
    /^\|\s*Item\s*\|\s*Version\s*\|\s*Source\s*\|\s*$/.test(l.trim()),
  );
  if (header === -1 || !/^\|[\s:|-]+\|$/.test((all[header + 1] ?? "").trim())) {
    throw new Error("pin table not found in README.md");
  }
  const rows = [];
  for (const raw of all.slice(header + 2)) {
    const line = raw.trim();
    if (!line.startsWith("|")) break;
    const cells = line
      .slice(1, line.endsWith("|") ? -1 : undefined)
      .split("|")
      .map((c) => c.trim().replace(/^`(.*)`$/, "$1"));
    const [item, version, source] = cells;
    if (cells.length !== 3 || !item) throw new Error(`malformed pin row: ${line}`);
    rows.push({ item, version, source });
  }
  return rows;
}

/** rust-toolchain.toml -> { rust: channel } */
export function readRustToolchain(text) {
  const m = text.match(/^\s*channel\s*=\s*"([^"]+)"/m);
  return m ? { rust: m[1] } : {};
}

/** .nvmrc -> { node: version } */
export function readNvmrc(text) {
  const v = text.trim();
  return v ? { node: v } : {};
}

/** Root devDependencies that carry a README pin row (exact versions). */
export const PINNED_DEV_DEPENDENCIES = ["yaml"];

/** package.json -> { pnpm, "node engines", ...PINNED_DEV_DEPENDENCIES } */
export function readPackageJson(text) {
  const pkg = JSON.parse(text);
  const out = {};
  for (const name of PINNED_DEV_DEPENDENCIES) {
    const spec = pkg.devDependencies?.[name];
    if (typeof spec === "string") out[name] = spec;
  }
  if (typeof pkg.packageManager === "string") {
    const m = pkg.packageManager.match(/^pnpm@([^+]+)/);
    out.pnpm = m ? m[1] : pkg.packageManager;
  }
  if (pkg.engines?.node) out["node engines"] = pkg.engines.node;
  return out;
}

/** pnpm-workspace.yaml -> entries of the top-level `catalog:` map */
export function readPnpmCatalog(text) {
  const out = {};
  let inCatalog = false;
  for (const line of lines(text)) {
    if (/^\S/.test(line)) {
      inCatalog = /^catalog:\s*$/.test(line);
      continue;
    }
    if (!inCatalog || !line.trim() || line.trim().startsWith("#")) continue;
    const m = line.match(/^\s+((?:"[^"]+")|(?:'[^']+')|[^:\s]+)\s*:\s*(.+?)\s*$/);
    if (m) out[unquote(m[1])] = unquote(m[2]);
  }
  return out;
}

/** Cargo.toml -> third-party (non-path) entries of [workspace.dependencies] */
export function readCargoWorkspaceDeps(text) {
  const out = {};
  let inSection = false;
  for (const line of lines(text)) {
    const t = line.trim();
    if (t.startsWith("[")) {
      inSection = t === "[workspace.dependencies]";
      continue;
    }
    if (!inSection || !t || t.startsWith("#")) continue;
    const m = t.match(/^([A-Za-z0-9_-]+)\s*=\s*(.+)$/);
    if (!m) continue;
    const [, name, value] = m;
    if (value.startsWith('"')) {
      out[name] = unquote(value);
    } else if (value.startsWith("{")) {
      if (/\bpath\s*=/.test(value)) continue;
      const v = value.match(/\bversion\s*=\s*"([^"]+)"/);
      if (v) out[name] = v[1];
    }
  }
  return out;
}

/** deploy-cloud.yml -> { sops, age } from the workflow `env` */
export function readDeployWorkflow(text) {
  const out = {};
  for (const [item, key] of [
    ["sops", "SOPS_VERSION"],
    ["age", "AGE_VERSION"],
  ]) {
    const m = text.match(new RegExp(`^\\s*${key}:\\s*(.+?)\\s*$`, "m"));
    if (m) out[item] = unquote(m[1]);
  }
  return out;
}

/** arena-cloud.Dockerfile -> { <runtime image>: <tag> } (the final FROM) */
export function readDockerfile(text) {
  const froms = [...text.matchAll(/^FROM\s+([^\s:@]+):([^\s@]+)/gm)];
  const last = froms.at(-1);
  return last ? { [last[1]]: last[2] } : {};
}

/** ci.yml -> { ghcr.io/tursodatabase/libsql-server: <tag> } (the rust job's service) */
export function readCiWorkflow(text) {
  const m = text.match(/ghcr\.io\/tursodatabase\/libsql-server:([\w.+-]+)/);
  return m ? { "ghcr.io/tursodatabase/libsql-server": m[1] } : {};
}

/** deploy/README.md -> { k3s: INSTALL_K3S_VERSION } */
export function readDeployReadme(text) {
  const m = text.match(/INSTALL_K3S_VERSION=([^\s"'`]+)/);
  return m ? { k3s: m[1] } : {};
}

/**
 * deploy/dev/compose.yaml -> { <image>: <tag> } for every `image:` line. An `image:` it cannot
 * read as `name:tag` (no tag, a digest, a `${VAR}`, a registry port with no tag) throws, so an
 * unpinned dev image never passes silently.
 */
export function readDevCompose(text) {
  const out = {};
  for (const m of text.matchAll(/^\s*-?\s*image:\s*(.*?)\s*$/gm)) {
    const ref = unquote(m[1]);
    const last = ref.slice(ref.lastIndexOf("/") + 1);
    const colon = last.lastIndexOf(":");
    if (!ref || /[$@\s]/.test(ref) || colon <= 0 || colon === last.length - 1) {
      throw new Error(
        `deploy/dev/compose.yaml: image not pinned by name:tag: ${m[1] || "(empty)"}`,
      );
    }
    const cut = ref.length - last.length + colon;
    out[ref.slice(0, cut)] = ref.slice(cut + 1);
  }
  return out;
}

export const READERS = {
  "rust-toolchain.toml": readRustToolchain,
  ".nvmrc": readNvmrc,
  "package.json": readPackageJson,
  "pnpm-workspace.yaml": readPnpmCatalog,
  "Cargo.toml": readCargoWorkspaceDeps,
  ".github/workflows/deploy-cloud.yml": readDeployWorkflow,
  "deploy/docker/arena-cloud.Dockerfile": readDockerfile,
  ".github/workflows/ci.yml": readCiWorkflow,
  "deploy/README.md": readDeployReadme,
  "deploy/dev/compose.yaml": readDevCompose,
};

/** Manifests whose values must be exact versions (no ranges). */
export const EXACT = {
  "pnpm-workspace.yaml": /^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/,
  "Cargo.toml": /^=\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/,
};

/**
 * Compares README rows against manifest values.
 * @param {Array<{item: string, version: string, source: string}>} table
 * @param {Record<string, Record<string, string>>} sources source file -> item -> version
 * @returns {string[]} error messages (empty when in sync)
 */
export function comparePins(table, sources) {
  const errors = [];
  const seen = new Set();
  for (const row of table) {
    const key = `${row.source}\u0000${row.item}`;
    if (seen.has(key)) {
      errors.push(`duplicate pin row: ${row.item} (${row.source})`);
      continue;
    }
    seen.add(key);
    const manifest = sources[row.source]?.[row.item];
    if (manifest === undefined) {
      errors.push(
        `extra pin row: ${row.item} — README ${row.version}, ${row.source} has no such entry`,
      );
    } else if (manifest !== row.version) {
      errors.push(`pin mismatch: ${row.item} — README ${row.version}, ${row.source} ${manifest}`);
    }
  }
  for (const [source, items] of Object.entries(sources)) {
    for (const [item, version] of Object.entries(items)) {
      const exact =
        EXACT[source] ??
        (source === "package.json" && PINNED_DEV_DEPENDENCIES.includes(item)
          ? EXACT["pnpm-workspace.yaml"]
          : undefined);
      if (exact && !exact.test(version)) {
        errors.push(`non-exact pin: ${item} (${source} ${version})`);
      }
      if (!seen.has(`${source}\u0000${item}`)) {
        errors.push(`missing pin row: ${item} (${source} ${version})`);
      }
    }
  }
  return errors;
}

const DEP_FIELDS = ["dependencies", "devDependencies", "peerDependencies"];

/**
 * Checks that manifests reference catalog packages as `catalog:` and `@arena/*` packages as
 * `workspace:*`.
 * @param {Array<{file: string, pkg: Record<string, any>}>} manifests
 * @param {Record<string, string>} catalog
 * @returns {string[]} error messages
 */
export function checkManifestDeps(manifests, catalog) {
  const errors = [];
  for (const { file, pkg } of manifests) {
    for (const field of DEP_FIELDS) {
      for (const [name, spec] of Object.entries(pkg[field] ?? {})) {
        if (name.startsWith("@arena/")) {
          if (spec !== "workspace:*") {
            errors.push(`non-workspace dependency: ${name} in ${file} (${spec})`);
          }
        } else if (name in catalog && spec !== "catalog:") {
          errors.push(`non-catalog dependency: ${name} in ${file} (${spec})`);
        }
      }
    }
  }
  return errors;
}

/** Root, `apps/*` and `packages/*` package.json files, relative to root. */
export function readManifests(root) {
  const files = ["package.json"];
  for (const dir of ["apps", "packages"]) {
    if (!existsSync(join(root, dir))) continue;
    for (const entry of readdirSync(join(root, dir), { withFileTypes: true })) {
      const file = `${dir}/${entry.name}/package.json`;
      if (entry.isDirectory() && existsSync(join(root, file))) files.push(file);
    }
  }
  return files.map((file) => ({ file, pkg: JSON.parse(readFileSync(join(root, file), "utf8")) }));
}

export function readSources(root) {
  const sources = {};
  for (const [file, reader] of Object.entries(READERS)) {
    sources[file] = reader(readFileSync(join(root, file), "utf8"));
  }
  return sources;
}

export function checkRepo(root) {
  let table;
  try {
    table = parsePinTable(readFileSync(join(root, "README.md"), "utf8"));
  } catch (err) {
    return [err.code === "ENOENT" ? "pin table not found in README.md" : err.message];
  }
  let sources;
  try {
    sources = readSources(root);
  } catch (err) {
    return [err.message];
  }
  return [
    ...comparePins(table, sources),
    ...checkManifestDeps(readManifests(root), sources["pnpm-workspace.yaml"]),
  ];
}

function main() {
  const root = join(dirname(fileURLToPath(import.meta.url)), "..");
  const errors = checkRepo(root);
  if (errors.length > 0) {
    for (const e of errors) console.error(e);
    process.exit(1);
  }
  console.log("pin check: ok");
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
