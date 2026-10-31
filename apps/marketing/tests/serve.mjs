// Builds the site twice (normal, and the pseudo-locale with every string 30% longer) and serves
// both static outputs: `dist` on 5180 and `dist-pseudo` on 5181. Used by playwright.config.ts.

import { spawn } from "node:child_process";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { serve } from "./static-server.mjs";

const root = resolve(fileURLToPath(new URL("..", import.meta.url)));
// Asynchronous, so the manifest fixture below keeps answering while Astro builds.
const astro = (args, env = {}) =>
  new Promise((done, fail) => {
    const child = spawn("pnpm", ["exec", "astro", "build", ...args], {
      cwd: root,
      env: { ...process.env, ...env },
      stdio: "inherit",
      shell: process.platform === "win32",
    });
    child.on("error", fail);
    child.on("exit", (code) =>
      code === 0 ? done() : fail(new Error(`astro build exited with ${code}`)),
    );
  });

// The Downloads page reads the release manifest at build time. Builds read a fixture served here,
// never the live bucket, so the suite is the same with or without a network.
const MANIFEST_PORT = 5182;
const manifest = await serve(join(root, "tests", "fixtures"), MANIFEST_PORT);
const releases = {
  PUBLIC_RELEASES_MANIFEST: `http://127.0.0.1:${MANIFEST_PORT}/releases/manifest.json`,
  // The Help centre reads its hardware data file from the fixture too, so the table has rows.
  PUBLIC_HARDWARE_DATA: join(root, "tests", "fixtures", "hardware.json"),
};

try {
  await astro([], releases);
  await astro(["--outDir", "dist-pseudo"], { ...releases, PUBLIC_PSEUDO: "1" });
} finally {
  manifest.close();
}

await serve(join(root, "dist"), 5180);
await serve(join(root, "dist-pseudo"), 5181);
