import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { checkRepo, checkSource } from "./check-hex.mjs";

test("raw hex: color: #FF00AA in a .svelte file fails with file:line", () => {
  const text = "<style>\n  p {\n    color: #FF00AA;\n  }\n</style>\n";
  assert.deepEqual(checkSource("apps/hub/src/routes/+page.svelte", text), [
    "apps/hub/src/routes/+page.svelte:3 raw hex colour #FF00AA",
  ]);
});

test("3, 4, 6 and 8 digit colours are caught; other lengths are not", () => {
  const text =
    "a{color:#fff}\nb{color:#ffff}\nc{color:#ffffff}\nd{color:#ffffff80}\ne{x:#fffff}\nf{x:#fffffff}\n";
  assert.deepEqual(checkSource("x.css", text), [
    "x.css:1 raw hex colour #fff",
    "x.css:2 raw hex colour #ffff",
    "x.css:3 raw hex colour #ffffff",
    "x.css:4 raw hex colour #ffffff80",
  ]);
});

test("HTML entities, ids, private fields and hex-ignore lines pass", () => {
  const text = [
    "<p>&#123; &#x1F600;</p>",
    '<a href="#faded-section">x</a>',
    "class A { #add() {} m() { this.#bad(); } }",
    "const brand = '#FF00AA'; // hex-ignore",
    "<!-- hex-ignore --> <b style='color:#000'></b>",
  ].join("\n");
  assert.deepEqual(checkSource("x.svelte", text), []);
});

test("hex in strings and template code is caught", () => {
  assert.deepEqual(checkSource("x.ts", 'const c = "#0a0b0c";'), ["x.ts:1 raw hex colour #0a0b0c"]);
});

test("in-page anchors and CSS id selectors pass; values still fail", () => {
  const text = [
    "<a href=\"#add\">x</a> <a href='#cafe'>y</a>",
    "#cafe {",
    "  color: red;",
    "}",
    "#bad, #fed {",
    "}",
    "a { box-shadow:",
    "    0 0 1px #fff,",
    "    0 0 2px #000; }",
    "b { color: #abc }",
  ].join("\n");
  assert.deepEqual(checkSource("x.svelte", text), [
    "x.svelte:8 raw hex colour #fff",
    "x.svelte:9 raw hex colour #000",
    "x.svelte:10 raw hex colour #abc",
  ]);
});

test("app.html templates are scanned", () => {
  const root = mkdtempSync(join(tmpdir(), "hex-"));
  try {
    mkdirSync(join(root, "apps/hub/src"), { recursive: true });
    writeFileSync(
      join(root, "apps/hub/src/app.html"),
      '<meta name="theme-color" content="#09071A" />\n',
    );
    assert.deepEqual(checkRepo(root), ["apps/hub/src/app.html:1 raw hex colour #09071A"]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
