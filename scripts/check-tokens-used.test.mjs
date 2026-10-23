import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { checkRepo, checkSource, declaredNames, listFiles } from "./check-tokens-used.mjs";

const known = new Set(["--color-bg", "--text"]);

test("known tokens pass", () => {
  assert.deepEqual(
    checkSource("a.svelte", "p { color: var(--text); background: var( --color-bg); }", known),
    [],
  );
});

test("removed token: var(--color-gone) fails with file:line", () => {
  const text = "<style>\n  p {\n    color: var(--color-gone);\n  }\n</style>\n";
  assert.deepEqual(checkSource("apps/hub/src/routes/+page.svelte", text, known), [
    "apps/hub/src/routes/+page.svelte:3 unknown token --color-gone",
  ]);
});

test("tokens declared in the same file are allowed", () => {
  const text = [
    ".x { --local-gap: 4px; gap: var(--local-gap); }",
    '<div style:--bar={w} style="--inline: 1"></div>',
    "el.style.setProperty('--js-set', v);",
    "a { width: var(--bar); height: var(--inline); top: var(--js-set); }",
  ].join("\n");
  assert.deepEqual(checkSource("a.svelte", text, known), []);
  assert.deepEqual(
    [...declaredNames(":root {\n  --color-bg: #000;\n  --text: var(--color-bg);\n}")],
    ["--color-bg", "--text"],
  );
});

test("scan scope: app and ui sources, not the generated tokens", () => {
  const root = mkdtempSync(join(tmpdir(), "tokens-used-"));
  try {
    const write = (rel, text) => {
      mkdirSync(join(root, rel, ".."), { recursive: true });
      writeFileSync(join(root, rel), text);
    };
    write("packages/ui/src/tokens/tokens.css", ":root {\n  --text: #fff;\n}\n");
    write("packages/ui/src/tokens/tokens.ts", "// var(--anything)\n");
    write("packages/ui/src/base.css", "html { color: var(--text); }\n");
    write("apps/hub/src/routes/+page.svelte", "<style>p { color: var(--color-gone); }</style>\n");
    write("apps/hub/src/app.css", "a { color: var(--text); }\n");
    write("apps/marketing/src/pages/index.astro", '<p style="color: var(--nope)"></p>\n');
    write("apps/hub/build/x.css", "a { color: var(--built); }\n");
    write("apps/hub/src/node_modules/x.js", "var(--dep)\n");
    assert.deepEqual(listFiles(root), [
      "apps/hub/src/app.css",
      "apps/hub/src/routes/+page.svelte",
      "apps/marketing/src/pages/index.astro",
      "packages/ui/src/base.css",
    ]);
    assert.deepEqual(checkRepo(root), [
      "apps/hub/src/routes/+page.svelte:1 unknown token --color-gone",
      "apps/marketing/src/pages/index.astro:1 unknown token --nope",
    ]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("names built in a template literal are skipped", () => {
  const text = "const c = `var(--color-${name})`;\nconst d = `var(--gone-${x}-y)`;\n";
  assert.deepEqual(checkSource("a.ts", text, known), []);
  assert.deepEqual(checkSource("a.ts", "`var(--color-gone)${x}`", known), [
    "a.ts:1 unknown token --color-gone",
  ]);
});
