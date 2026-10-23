import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { gitStatus } from "./check-generated.mjs";
import { DESIGN_MD, generate, TOKENS_DIR } from "./gen-tokens.mjs";
import { checkFresh, tokensReport } from "./check-tokens-fresh.mjs";

test("fresh: clean tokens folder passes", () => {
  assert.deepEqual(tokensReport(""), []);
});

test("stale: a regenerated tokens.css that differs from HEAD is listed", () => {
  const root = mkdtempSync(join(tmpdir(), "tokens-fresh-"));
  try {
    const git = (...args) =>
      execFileSync("git", ["-c", "user.name=t", "-c", "user.email=t@t", ...args], {
        cwd: root,
        stdio: "ignore",
      });
    const dir = join(root, TOKENS_DIR);
    mkdirSync(dir, { recursive: true });
    writeFileSync(join(dir, "tokens.css"), ":root {\n  --color-bg: #09071a;\n}\n");
    writeFileSync(join(dir, "tokens.ts"), "export {};\n");
    mkdirSync(join(root, "packages/ui/src/generated"), { recursive: true });
    writeFileSync(join(root, "packages/ui/src/generated/x.ts"), "export {};\n");
    git("init", "-q");
    git("add", "-A");
    git("commit", "-qm", "fixture");

    // DESIGN.md colour edited and regenerated, but not committed; unrelated changes ignored
    writeFileSync(join(dir, "tokens.css"), ":root {\n  --color-bg: #000000;\n}\n");
    writeFileSync(join(root, "packages/ui/src/generated/x.ts"), "export type X = 1;\n");

    const report = tokensReport(gitStatus(root, TOKENS_DIR));
    assert.deepEqual(report, [
      "generated design tokens are out of date (run `pnpm gen:tokens` and commit):",
      "  stale: packages/ui/src/tokens/tokens.css",
    ]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("checkFresh: committed tokens are fresh; an edited DESIGN.md colour makes tokens.css stale", async () => {
  const root = mkdtempSync(join(tmpdir(), "tokens-fresh-e2e-"));
  try {
    const git = (...args) =>
      execFileSync("git", ["-c", "user.name=t", "-c", "user.email=t@t", ...args], {
        cwd: root,
        stdio: "ignore",
      });
    const design = (bg) =>
      [
        "---",
        "colors:",
        `  bg: '${bg}'`,
        "  text: '#F5F2FF'",
        "  bg-hc: '#000000'",
        "  text-hc: '#FFFFFF'",
        "spacing:",
        "  touch-min: 44px",
        "components:",
        "  panel:",
        "    background: '{colors.bg}'",
        "    foreground: '{colors.text}'",
        "---",
        "# Design",
        "",
      ].join("\n");
    mkdirSync(join(root, DESIGN_MD, ".."), { recursive: true });
    writeFileSync(join(root, DESIGN_MD), design("#09071A"));
    await generate(root);
    git("init", "-q");
    git("add", "-A");
    git("commit", "-qm", "fixture");

    assert.deepEqual(await checkFresh(root), []);

    writeFileSync(join(root, DESIGN_MD), design("#101010"));
    const report = await checkFresh(root);
    assert.match(report[0], /generated design tokens are out of date/);
    assert.ok(report.includes("  stale: packages/ui/src/tokens/tokens.css"), report.join("\n"));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
