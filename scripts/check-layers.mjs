#!/usr/bin/env node
// Layer direction check: enforces the spine layer table on workspace crate edges.
// Reads `cargo metadata --no-deps` (offline, no network). Dev-dependencies are ignored.

import { execFileSync } from "node:child_process";
import { realpathSync } from "node:fs";
import { fileURLToPath } from "node:url";

/** Allowed workspace dependencies per crate. Crates not listed are unrestricted. */
export const LAYER_RULES = {
  "arena-proto": [],
  "arena-store": ["arena-proto"],
  "arena-core": ["arena-store", "arena-proto"],
  "arena-api": ["arena-core", "arena-proto"],
  "arena-tasks": ["arena-core", "arena-store", "arena-proto"],
  "arena-replay": ["arena-store", "arena-proto"],
  "arena-cloud": ["arena-proto", "arena-replay", "arena-store"],
  "arena-kiosk-core": ["arena-proto"],
};

/**
 * Returns every forbidden edge between workspace crates.
 * @param {{packages: Array<{id: string, name: string, dependencies: Array<{name: string, kind: string|null}>}>, workspace_members: string[]}} metadata
 * @param {Record<string, string[]>} rules
 * @returns {Array<{from: string, to: string}>}
 */
export function findViolations(metadata, rules) {
  const members = new Set(metadata.workspace_members);
  const workspace = metadata.packages.filter((p) => members.has(p.id));
  const names = new Set(workspace.map((p) => p.name));
  const violations = [];
  const seen = new Set();
  for (const pkg of workspace) {
    const allowed = rules[pkg.name];
    if (!allowed) continue;
    for (const dep of pkg.dependencies) {
      if (dep.kind === "dev") continue;
      if (!names.has(dep.name)) continue;
      if (allowed.includes(dep.name)) continue;
      const key = `${pkg.name}->${dep.name}`;
      if (seen.has(key)) continue;
      seen.add(key);
      violations.push({ from: pkg.name, to: dep.name });
    }
  }
  violations.sort((a, b) => a.from.localeCompare(b.from) || a.to.localeCompare(b.to));
  return violations;
}

export function formatViolation(v) {
  return `forbidden edge: ${v.from} -> ${v.to}`;
}

function readMetadata() {
  const out = execFileSync(
    "cargo",
    ["metadata", "--format-version", "1", "--no-deps", "--offline"],
    {
      encoding: "utf8",
      maxBuffer: 64 * 1024 * 1024,
    },
  );
  return JSON.parse(out);
}

function main() {
  const violations = findViolations(readMetadata(), LAYER_RULES);
  if (violations.length > 0) {
    for (const v of violations) console.error(formatViolation(v));
    process.exit(1);
  }
  console.log("layer check: ok");
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
