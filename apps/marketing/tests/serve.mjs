// Builds the site twice (normal, and the pseudo-locale with every string 30% longer) and serves
// both static outputs: `dist` on 5180 and `dist-pseudo` on 5181. Used by playwright.config.ts.

import { execFileSync } from "node:child_process";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { serve } from "./static-server.mjs";

const root = resolve(fileURLToPath(new URL("..", import.meta.url)));
const astro = (args, env = {}) =>
  execFileSync("pnpm", ["exec", "astro", "build", ...args], {
    cwd: root,
    env: { ...process.env, ...env },
    stdio: "inherit",
    shell: process.platform === "win32",
  });

astro([]);
astro(["--outDir", "dist-pseudo"], { PUBLIC_PSEUDO: "1" });

await serve(join(root, "dist"), 5180);
await serve(join(root, "dist-pseudo"), 5181);
