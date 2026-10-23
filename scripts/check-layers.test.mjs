import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { findViolations, formatViolation, LAYER_RULES } from "./check-layers.mjs";

// Builds minimal `cargo metadata` output: { crate: [dep | [dep, kind]] }.
function metadata(graph) {
  const packages = Object.entries(graph).map(([name, deps]) => ({
    id: `path+file:///ws/crates/${name}#0.1.0`,
    name,
    dependencies: deps.map((d) => {
      const [depName, kind] = Array.isArray(d) ? d : [d, null];
      return { name: depName, kind };
    }),
  }));
  return { packages, workspace_members: packages.map((p) => p.id) };
}

const clean = {
  "arena-proto": [],
  "arena-store": ["arena-proto"],
  "arena-core": ["arena-store", "arena-proto"],
  "arena-api": ["arena-core", "arena-proto"],
  "arena-tasks": ["arena-core", "arena-store", "arena-proto"],
  "arena-replay": ["arena-store", "arena-proto"],
  "arena-cloud": ["arena-proto", "arena-replay", "arena-store"],
  "arena-kiosk-core": ["arena-proto"],
  "arena-hub": ["arena-api", "arena-tasks", "arena-core", "arena-store", "arena-proto"],
  "arena-kiosk-svc": ["arena-kiosk-core", "arena-proto"],
  "arena-pi": ["arena-proto"],
};

test("clean graph has no violations", () => {
  assert.deepEqual(findViolations(metadata(clean), LAYER_RULES), []);
});

test("proto depending on store is forbidden", () => {
  const v = findViolations(metadata({ ...clean, "arena-proto": ["arena-store"] }), LAYER_RULES);
  assert.deepEqual(v.map(formatViolation), ["forbidden edge: arena-proto -> arena-store"]);
});

test("core depending on api is forbidden and all edges are listed", () => {
  const graph = {
    ...clean,
    "arena-core": ["arena-store", "arena-proto", "arena-api", "arena-tasks"],
    "arena-replay": ["arena-store", "arena-proto", "arena-core"],
  };
  assert.deepEqual(findViolations(metadata(graph), LAYER_RULES).map(formatViolation), [
    "forbidden edge: arena-core -> arena-api",
    "forbidden edge: arena-core -> arena-tasks",
    "forbidden edge: arena-replay -> arena-core",
  ]);
});

test("build dependencies are checked, dev dependencies are ignored", () => {
  const graph = {
    ...clean,
    "arena-store": ["arena-proto", ["arena-core", "dev"]],
    "arena-proto": [["arena-store", "build"]],
  };
  assert.deepEqual(findViolations(metadata(graph), LAYER_RULES).map(formatViolation), [
    "forbidden edge: arena-proto -> arena-store",
  ]);
});

test("unrestricted crates and external deps are not flagged", () => {
  const graph = {
    ...clean,
    "arena-hub": ["arena-cloud", "arena-replay", "arena-api"],
    "arena-core": ["arena-store", "arena-proto", "serde"],
  };
  assert.deepEqual(findViolations(metadata(graph), LAYER_RULES), []);
});

test("the shared hub client may depend on arena-proto only", () => {
  const graph = { ...clean, "arena-kiosk-core": ["arena-proto", "arena-core"] };
  assert.deepEqual(findViolations(metadata(graph), LAYER_RULES).map(formatViolation), [
    "forbidden edge: arena-kiosk-core -> arena-core",
  ]);
});

test("an edge listed twice (normal + build) is reported once", () => {
  const graph = { ...clean, "arena-proto": ["arena-store", ["arena-store", "build"]] };
  assert.equal(findViolations(metadata(graph), LAYER_RULES).length, 1);
});

test("the real workspace passes", () => {
  const out = execFileSync(
    "cargo",
    ["metadata", "--format-version", "1", "--no-deps", "--offline"],
    { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 },
  );
  assert.deepEqual(findViolations(JSON.parse(out), LAYER_RULES), []);
});
