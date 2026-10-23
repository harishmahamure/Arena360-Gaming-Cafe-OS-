import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { checkBundled, checkRepo, checkSource } from "./check-no-remote.mjs";

test("remote font: url(https://fonts.gstatic.com/x.woff2) fails naming the file and url", () => {
  const out = checkSource(
    "apps/hub/build/_app/immutable/assets/0.css",
    "@font-face{font-family:Sora;src:url(https://fonts.gstatic.com/x.woff2) format('woff2')}",
  );
  assert.deepEqual(out, [
    "apps/hub/build/_app/immutable/assets/0.css: url() https://fonts.gstatic.com/x.woff2",
  ]);
});

test("@import, <link>, <script> and cdn hosts fail", () => {
  const out = checkSource(
    "index.html",
    [
      '<style>@import "https://example.com/a.css";</style>',
      '<link rel="stylesheet" href="https://example.com/b.css">',
      '<script type="module" src="http://example.com/c.js"></script>',
      'fetch("https://cdn.jsdelivr.net/npm/x")',
      "<p>fonts.googleapis.com</p>",
      "a{background:url(//example.com/p.png)}",
    ].join("\n"),
  );
  assert.deepEqual(out, [
    "index.html: url() //example.com/p.png",
    "index.html: @import https://example.com/a.css",
    "index.html: <link> https://example.com/b.css",
    "index.html: <script> http://example.com/c.js",
    "index.html: remote host https://cdn.jsdelivr.net/npm/x",
    "index.html: remote host fonts.googleapis.com",
  ]);
});

test("local assets, data urls and plain links in code pass", () => {
  const text = [
    "@font-face{src:url(./sora-latin-wght-normal.woff2) format('woff2-variations')}",
    "a{background:url(data:image/svg+xml;base64,AAAA)}",
    '<link rel="modulepreload" href="/_app/immutable/entry/start.js">',
    '<script src="./_app/x.js"></script>',
    'throw new Error("see https://svelte.dev/e/state_unsafe_mutation")',
    'const ns = "http://www.w3.org/2000/svg";',
  ].join("\n");
  assert.deepEqual(checkSource("x.html", text), []);
});

test("checkRepo scans each app build and reports a missing one", () => {
  const root = mkdtempSync(join(tmpdir(), "no-remote-"));
  try {
    mkdirSync(join(root, "apps/hub/build/_app"), { recursive: true });
    writeFileSync(join(root, "apps/hub/build/index.html"), "<p>ok</p>");
    writeFileSync(
      join(root, "apps/hub/build/_app/a.css"),
      "a{src:url(https://fonts.gstatic.com/x.woff2)}",
    );
    writeFileSync(join(root, "apps/hub/build/_app/a.woff2"), "https://cdn.example.com/binary");
    writeFileSync(join(root, "apps/hub/build/_app/sora-latin.X.woff2"), "font");
    writeFileSync(join(root, "apps/hub/build/_app/b.css"), ":root{--color-bg:#09071a}");
    assert.deepEqual(checkRepo(root, ["hub", "kitchen"]), [
      "apps/hub/build/_app/a.css: url() https://fonts.gstatic.com/x.woff2",
      "apps/kitchen/build: missing build output (run `pnpm --filter @arena/kitchen build` first)",
    ]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("protocol-relative imports, media, frames, srcset, dynamic import, workers and ws fail", () => {
  const out = checkSource(
    "x.html",
    [
      '<style>@import "//example.com/a.css";</style>',
      '<img alt="" src="https://example.com/i.png">',
      '<img srcset="/local.png 1x, //example.com/i@2x.png 2x">',
      '<iframe src="https://example.com/frame"></iframe>',
      '<source src="//example.com/v.mp4">',
      'import("https://example.com/m.js")',
      'new Worker("wss://example.com/w.js")',
      "new SharedWorker('ws://example.com/s.js')",
    ].join("\n"),
  );
  assert.deepEqual(out, [
    "x.html: @import //example.com/a.css",
    "x.html: <img> https://example.com/i.png",
    "x.html: <iframe> https://example.com/frame",
    "x.html: <source> //example.com/v.mp4",
    "x.html: import() https://example.com/m.js",
    "x.html: Worker wss://example.com/w.js",
    "x.html: Worker ws://example.com/s.js",
    "x.html: srcset //example.com/i@2x.png",
  ]);
});

test("an empty build dir fails instead of passing vacuously", () => {
  const root = mkdtempSync(join(tmpdir(), "no-remote-"));
  try {
    mkdirSync(join(root, "apps/kiosk/build"), { recursive: true });
    assert.deepEqual(checkRepo(root, ["kiosk"]), ["apps/kiosk/build: build output is empty"]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("a build must bundle a sora woff2 and CSS declaring --color-bg", () => {
  const files = ["b/index.html", "b/_app/a.css", "b/_app/sora-latin-wght-normal.X.woff2"];
  const css = { "b/index.html": "<p></p>", "b/_app/a.css": ":root{--color-bg:#09071a}" };
  assert.deepEqual(
    checkBundled("b", files, (f) => css[f]),
    [],
  );
  assert.deepEqual(
    checkBundled("b", files.slice(0, 2), (f) => css[f]),
    ["b: no bundled sora-*.woff2 asset (is @arena/ui/base.css imported?)"],
  );
  assert.deepEqual(
    checkBundled("b", files, () => "a{}"),
    ["b: no CSS declares --color-bg (is @arena/ui/base.css imported?)"],
  );
});
