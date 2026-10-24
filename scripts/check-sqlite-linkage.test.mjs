import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

// Story 2.2: the linkage check's symbol rules fire on crafted `nm` listings (two SQLite copies,
// a libsql C core, no SQLite at all) and on a `cargo tree` with libsql-ffi, and pass a
// single copy. The real binary is checked by the
// CI rust job (Linux) and locally on macOS.

const script = join(dirname(fileURLToPath(import.meta.url)), "check-sqlite-linkage.sh");

test(
  "check-sqlite-linkage --self-test passes",
  { skip: process.platform === "win32" && "bash script (Linux and macOS)" },
  () => {
    const r = spawnSync("bash", [script, "--self-test"], { encoding: "utf8" });
    assert.equal(r.status, 0, `${r.stdout}\n${r.stderr}`);
    assert.match(r.stdout, /two\.txt -> fail/);
    assert.match(r.stdout, /libsql\.txt -> fail/);
    assert.match(r.stdout, /one\.txt -> pass/);
    assert.match(r.stdout, /tree-ffi\.txt -> fail/);
  },
);
