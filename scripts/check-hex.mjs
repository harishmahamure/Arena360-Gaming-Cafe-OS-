#!/usr/bin/env node
// Hex lint: colours come from the generated tokens. Fails on raw hex colour literals in app and
// UI sources (and `.html` templates) outside packages/ui/src/tokens. HTML entities (`&#123;`) are allowed; exempt a line
// with a `hex-ignore` comment. Reads local files only (no network).

import { readFileSync, realpathSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { EXTENSIONS, listFiles } from "./check-tokens-used.mjs";

export const IGNORE_MARKER = "hex-ignore";
/** The token scan scope plus `.html` (e.g. `apps/*\/src/app.html`). */
export const HEX_EXTENSIONS = [...EXTENSIONS, ".html"];
const HEX = /(?<![&\w.$#])#(?:[0-9a-fA-F]{8}|[0-9a-fA-F]{6}|[0-9a-fA-F]{3,4})(?![\w(-])/g;

/**
 * Raw hex colours in one file.
 * @returns {string[]} `file:line raw hex colour #XXXXXX`
 */
export function checkSource(file, text) {
  const out = [];
  let offset = 0;
  text.split("\n").forEach((raw, i) => {
    const line = raw.replace(/\r$/, "");
    const lineStart = offset;
    offset += raw.length + 1;
    if (line.includes(IGNORE_MARKER)) return;
    for (const m of line.matchAll(HEX)) {
      const before = line.slice(0, m.index);
      const after = line.slice(m.index + m[0].length);
      // In-page anchors: href="#add", href='#cafe'.
      if (/\bhref\s*=\s*["']$/i.test(before)) continue;
      // CSS id selectors (`#cafe {`, `#cafe,`): no `prop:` since the last `{`, `}` or `;`
      // (looking back across lines, so multi-line values like `box-shadow:` still count).
      const statement = text
        .slice(0, lineStart + m.index)
        .split(/[{};]/)
        .pop();
      if (/^\s*[{,]/.test(after) && !statement.includes(":")) continue;
      out.push(`${file}:${i + 1} raw hex colour ${m[0]}`);
    }
  });
  return out;
}

export function checkRepo(root) {
  return listFiles(root, HEX_EXTENSIONS).flatMap((file) =>
    checkSource(file, readFileSync(join(root, file), "utf8")),
  );
}

function main() {
  const root = join(dirname(fileURLToPath(import.meta.url)), "..");
  const errors = checkRepo(root);
  if (errors.length > 0) {
    for (const e of errors) console.error(e);
    process.exit(1);
  }
  console.log("hex check: ok");
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
  main();
}
