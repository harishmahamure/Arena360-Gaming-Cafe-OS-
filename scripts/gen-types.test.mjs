import { test } from "node:test";
import assert from "node:assert/strict";
import { buildBarrel, HEADER, parseBarrel, typeNamesFromFiles } from "./gen-types.mjs";

test("type names come from the ts-rs files, sorted, barrel excluded", () => {
  assert.deepEqual(typeNamesFromFiles(["index.ts", "ErrorCode.ts", "AppError.ts", "x.json"]), [
    "AppError",
    "ErrorCode",
  ]);
});

test("barrel carries the do-not-edit header and re-exports every type", () => {
  const barrel = buildBarrel(["AppError", "ErrorCode"]);
  assert.ok(barrel.startsWith(`${HEADER}\n`));
  assert.match(barrel, /export type \{ AppError \} from "\.\/AppError";/);
  assert.match(barrel, /export type \{ ErrorCode \} from "\.\/ErrorCode";/);
});

test("parseBarrel round-trips buildBarrel", () => {
  const names = ["AppError", "ErrorCode", "Permission"];
  assert.deepEqual(parseBarrel(buildBarrel(names)), names);
});

test("parseBarrel reads multi-name and renamed exports", () => {
  assert.deepEqual(parseBarrel('export type { A, B as C } from "./x";'), ["A", "C"]);
});
