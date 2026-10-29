import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { checkRepo, checkSource, listSources } from "./check-no-manifest.mjs";

test("an upload-manifest route, table or ingest cursor fails with file:line", () => {
  const text = [
    'let r = Router::new().route("/uploads/manifest", get(h));',
    "CREATE TABLE upload_manifest (id INTEGER);",
    "let ingest_cursor = 0;",
    "const IngestCursor = 1;",
    "fn fine() {}",
  ].join("\n");
  assert.deepEqual(checkSource("crates/arena-cloud/src/x.rs", text), [
    "crates/arena-cloud/src/x.rs:1 manifest-endpoint",
    "crates/arena-cloud/src/x.rs:2 manifest-table",
    "crates/arena-cloud/src/x.rs:3 ingest-cursor",
    "crates/arena-cloud/src/x.rs:4 ingest-cursor",
  ]);
});

test("the gap check's own vocabulary passes", () => {
  const text =
    "resend_unavailable last_seq_seen last_epoch_seen changes/<epoch>/ branch manifest of catalogue";
  assert.deepEqual(checkSource("crates/arena-cloud/src/gaps.rs", text), []);
});

test("this script and its test are exempt", () => {
  assert.deepEqual(checkSource("scripts/check-no-manifest.mjs", "upload_manifest"), []);
  assert.deepEqual(checkSource("scripts/check-no-manifest.test.mjs", "/uploads/manifest"), []);
});

test("build output, dependencies and planning documents are not scanned", () => {
  const root = mkdtempSync(join(tmpdir(), "manifest-"));
  try {
    for (const dir of [
      "crates/a/src",
      "target/debug",
      "node_modules/x",
      "_bmad-output/impl",
      "docs",
    ]) {
      mkdirSync(join(root, dir), { recursive: true });
      writeFileSync(join(root, dir, "f.rs"), "upload_manifest\n");
    }
    writeFileSync(join(root, "crates/a/src/readme.txt"), "upload_manifest\n");
    assert.deepEqual(listSources(root), ["crates/a/src/f.rs"]);
    assert.deepEqual(checkRepo(root), ["crates/a/src/f.rs:1 manifest-table"]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
