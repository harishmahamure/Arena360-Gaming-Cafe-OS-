import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { checkRepo, checkSource, inScope } from "./check-no-blur.mjs";

test("backdrop-filter in a style block fails with file:line", () => {
  const text = "<style>\n  .sheet {\n    backdrop-filter: blur(8px);\n  }\n</style>\n";
  assert.deepEqual(checkSource("apps/kiosk/src/lib/A.svelte", text), [
    "apps/kiosk/src/lib/A.svelte:3 backdrop-filter",
  ]);
});

test("the -webkit- prefix and any case are caught", () => {
  const text = ".a { -webkit-backdrop-filter: blur(2px); }\n.b { BACKDROP-FILTER: none; }\n";
  assert.deepEqual(checkSource("x.css", text), [
    "x.css:1 backdrop-filter",
    "x.css:2 backdrop-filter",
  ]);
});

test("comments and blur-ignore lines pass", () => {
  const text = [
    "/* no backdrop-filter here */",
    "// never backdrop-filter",
    " * backdrop-filter is banned",
    "<!-- backdrop-filter -->",
    "a { backdrop-filter: none; } /* blur-ignore */",
  ].join("\n");
  assert.deepEqual(checkSource("x.svelte", text), []);
});

test("filter: blur is caught, backdrop-filter is not double counted, other filters pass", () => {
  const text = [
    ".a { filter: blur(4px); }",
    ".b { -webkit-filter: blur(2px) brightness(1); }",
    ".c { filter: brightness(0.5); }",
    ".d { backdrop-filter: blur(8px); }",
    '<div style="filter:blur(1px)"></div>',
  ].join("\n");
  assert.deepEqual(checkSource("x.svelte", text), [
    "x.svelte:1 filter-blur",
    "x.svelte:2 filter-blur",
    "x.svelte:4 backdrop-filter",
    "x.svelte:5 filter-blur",
  ]);
});

test("autoplaying unmuted media fails, muted video and plain media pass", () => {
  const text = [
    "<video autoplay muted loop></video>",
    "<video autoplay></video>",
    "<video",
    "  src={a}",
    "  autoplay",
    "></video>",
    "<audio autoplay muted></audio>",
    "<video controls></video>",
    "<audio src={a}></audio>",
    "<video autoplay></video> <!-- blur-ignore -->",
  ].join("\n");
  assert.deepEqual(checkSource("x.svelte", text), [
    "x.svelte:2 autoplay-unmuted",
    "x.svelte:3 autoplay-unmuted",
    "x.svelte:7 autoplay-unmuted",
  ]);
});

test("only branch surfaces and the shared UI package are in scope", () => {
  assert.equal(inScope("apps/hub/src/a.css"), true);
  assert.equal(inScope("apps/kiosk/src/a.css"), true);
  assert.equal(inScope("apps/kitchen/src/a.css"), true);
  assert.equal(inScope("packages/ui/src/components/A.svelte"), true);
  assert.equal(inScope("apps/marketing/src/a.css"), false);
  assert.equal(inScope("apps/owner/src/a.css"), false);
});

test("the repo scan reports a branch surface and ignores a web surface", () => {
  const root = mkdtempSync(join(tmpdir(), "blur-"));
  try {
    mkdirSync(join(root, "apps/kitchen/src"), { recursive: true });
    mkdirSync(join(root, "apps/marketing/src"), { recursive: true });
    writeFileSync(join(root, "apps/kitchen/src/a.css"), ".a { backdrop-filter: blur(1px); }\n");
    writeFileSync(join(root, "apps/marketing/src/a.css"), ".a { backdrop-filter: blur(1px); }\n");
    assert.deepEqual(checkRepo(root), ["apps/kitchen/src/a.css:1 backdrop-filter"]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
