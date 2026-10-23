#!/usr/bin/env node
// Freshness gate: regenerates the design tokens and fails if the committed files differ.

import { realpathSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { formatReport, gitStatus, parsePorcelain } from "./check-generated.mjs";
import { generate, TOKENS_DIR } from "./gen-tokens.mjs";

/** Failure report for porcelain output limited to the tokens folder, or `[]`. */
export function tokensReport(porcelain) {
  return formatReport(parsePorcelain(porcelain), "generated design tokens", "pnpm gen:tokens");
}

/** Regenerates the tokens in `root` and returns the failure report (`[]` when fresh). */
export async function checkFresh(root) {
  await generate(root);
  return tokensReport(gitStatus(root, TOKENS_DIR));
}

async function main() {
  const root = join(dirname(fileURLToPath(import.meta.url)), "..");
  const report = await checkFresh(root);
  if (report.length > 0) {
    for (const line of report) console.error(line);
    process.exit(1);
  }
  console.log("tokens fresh check: ok");
}

const isMain = (() => {
  try {
    return (
      Boolean(process.argv[1]) &&
      realpathSync(fileURLToPath(import.meta.url)) === realpathSync(process.argv[1])
    );
  } catch {
    return false;
  }
})();

if (isMain) {
  main().catch((err) => {
    console.error(err.message);
    process.exit(1);
  });
}
