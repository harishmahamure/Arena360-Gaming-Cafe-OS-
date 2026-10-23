import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  checkRepo,
  checkSource,
  formatViolation,
  importedNames,
  isAllowedSpecifier,
} from "./check-protocol-types.mjs";

const names = new Set(["AppError", "ErrorCode", "Permission"]);
const lint = (file, text) => checkSource(file, text, names).map(formatViolation);

test("hand copy: a declaration reusing a protocol type name fails with file:line", () => {
  assert.deepEqual(lint("apps/hub/src/x.ts", "type AppError = { code: string };\n"), [
    "apps/hub/src/x.ts:1 hand-written protocol type AppError",
  ]);
});

test("every declaration form is caught", () => {
  const src = [
    "export interface AppError {}",
    "export enum ErrorCode { A }",
    "declare type Permission = never;",
    "export class AppError {}",
    "export const enum ErrorCode { B }",
  ].join("\n");
  assert.deepEqual(
    checkSource("apps/hub/src/x.ts", src, names).map((v) => v.line),
    [1, 2, 3, 4, 5],
  );
});

test("unrelated declarations pass", () => {
  assert.deepEqual(lint("apps/hub/src/x.ts", "type AppErrorView = {};\ninterface Foo {}\n"), []);
});

test("wrong import: a protocol type from a relative path fails naming the required specifier", () => {
  const src = 'import type { AppError } from "../../packages/ui/src/generated/AppError";\n';
  assert.deepEqual(lint("apps/hub/src/x.ts", src), [
    'apps/hub/src/x.ts:1 protocol type AppError must be imported from "@arena/ui/generated" (found "../../packages/ui/src/generated/AppError")',
  ]);
});

test("wrong import inside a .svelte script reports the right line", () => {
  const src =
    '<script lang="ts">\n  import { type ErrorCode as Code } from "@arena/ui";\n</script>\n';
  const out = checkSource("apps/hub/src/routes/+page.svelte", src, names);
  assert.equal(out.length, 1);
  assert.equal(out[0].line, 2);
  assert.match(out[0].message, /ErrorCode must be imported from "@arena\/ui\/generated"/);
});

test("re-exports and dynamic type imports from the wrong place fail", () => {
  const src = [
    'export type { AppError } from "./somewhere";',
    'let e: import("../generated/AppError").AppError;',
  ].join("\n");
  assert.deepEqual(
    checkSource("apps/hub/src/x.ts", src, names).map((v) => v.line),
    [1, 2],
  );
});

test("imports from @arena/ui/generated pass, multi-line too", () => {
  const src = 'import type {\n  AppError,\n  ErrorCode,\n} from "@arena/ui/generated";\n';
  assert.deepEqual(lint("apps/hub/src/x.ts", src), []);
});

test("non-protocol names from other specifiers pass", () => {
  assert.deepEqual(lint("apps/hub/src/x.ts", 'import { Placeholder } from "@arena/ui";\n'), []);
});

test("packages/ui may import its own ./generated barrel; apps may not", () => {
  const src = 'import type { AppError } from "./generated";\n';
  assert.deepEqual(lint("packages/ui/src/x.ts", src), []);
  assert.equal(lint("apps/hub/src/x.ts", src).length, 1);
  assert.ok(isAllowedSpecifier("../generated/index.ts", "packages/ui/src/a/b.ts"));
  assert.ok(!isAllowedSpecifier("./generated/AppError", "packages/ui/src/x.ts"));
});

test("importedNames handles default, named, type-only and renamed bindings", () => {
  assert.deepEqual(importedNames("A, { type B as C, D }"), ["A", "B", "D"]);
  assert.deepEqual(importedNames("* as ns"), []);
});

test("checkRepo scans apps and packages, skipping generated and build folders", () => {
  const root = mkdtempSync(join(tmpdir(), "protocol-"));
  try {
    const put = (path, text) => {
      mkdirSync(join(root, path, ".."), { recursive: true });
      writeFileSync(join(root, path), text);
    };
    put("packages/ui/src/generated/index.ts", 'export type { AppError } from "./AppError";\n');
    put("packages/ui/src/generated/AppError.ts", "export type AppError = {};\n");
    put("apps/hub/build/x.js", "class AppError {}\n");
    put("apps/hub/node_modules/x/y.ts", "type AppError = 1;\n");
    put("apps/hub/src/ok.ts", 'import type { AppError } from "@arena/ui/generated";\n');
    put("apps/marketing/src/pages/index.astro", "---\ninterface AppError {}\n---\n");
    assert.deepEqual(checkRepo(root), [
      "apps/marketing/src/pages/index.astro:2 hand-written protocol type AppError",
    ]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("star exports and namespace imports of generated code fail unless allowed", () => {
  const src = [
    'export * from "../../packages/ui/src/generated";',
    'import * as P from "../generated/AppError";',
    'export type * from "./generated/index";',
    'import * as P2 from "@arena/ui/generated";',
    'import * as utils from "./utils";',
  ].join("\n");
  assert.deepEqual(
    checkSource("apps/hub/src/x.ts", src, names).map((v) => v.line),
    [1, 2, 3],
  );
  assert.match(lint("apps/hub/src/x.ts", src)[0], /star import\/export/);
  assert.deepEqual(lint("packages/ui/src/x.ts", 'export * from "./generated";\n'), []);
});

test("only packages/ui/src/generated is skipped; other generated folders are scanned", () => {
  const root = mkdtempSync(join(tmpdir(), "protocol-"));
  try {
    const put = (path, text) => {
      mkdirSync(join(root, path, ".."), { recursive: true });
      writeFileSync(join(root, path), text);
    };
    put("packages/ui/src/generated/index.ts", 'export type { AppError } from "./AppError";\n');
    put("packages/ui/src/generated/AppError.ts", "export type AppError = {};\n");
    put("apps/hub/src/generated/AppError.ts", "export type AppError = {};\n");
    assert.deepEqual(checkRepo(root), [
      "apps/hub/src/generated/AppError.ts:1 hand-written protocol type AppError",
    ]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
