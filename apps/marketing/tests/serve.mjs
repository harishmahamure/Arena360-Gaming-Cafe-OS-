// Builds the site twice (normal, and the pseudo-locale with every string 30% longer) and serves
// both static outputs: `dist` on 5180 and `dist-pseudo` on 5181. Used by playwright.config.ts.
// Like Workers static assets with `not_found_handling: "404-page"`: an unknown path gets 404.html.

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, statSync } from "node:fs";
import { createServer } from "node:http";
import { extname, join, normalize, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(fileURLToPath(new URL("..", import.meta.url)));
const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".css": "text/css",
  ".js": "text/javascript",
  ".xml": "application/xml",
  ".txt": "text/plain",
  ".woff2": "font/woff2",
  ".svg": "image/svg+xml",
};

const astro = (args, env = {}) =>
  execFileSync("pnpm", ["exec", "astro", "build", ...args], {
    cwd: root,
    env: { ...process.env, ...env },
    stdio: "inherit",
    shell: process.platform === "win32",
  });

astro([]);
astro(["--outDir", "dist-pseudo"], { PUBLIC_PSEUDO: "1" });

function serve(dir, port) {
  const base = join(root, dir);
  createServer((request, response) => {
    const path = decodeURIComponent(new URL(request.url ?? "/", "http://x").pathname);
    const candidates = [path, join(path, "index.html"), path + ".html"].map((p) =>
      normalize(join(base, p)),
    );
    const file = candidates.find(
      (p) => p.startsWith(base) && existsSync(p) && statSync(p).isFile(),
    );
    const found = file ?? join(base, "404.html");
    response.writeHead(file ? 200 : 404, {
      "Content-Type": TYPES[extname(found)] ?? "application/octet-stream",
    });
    response.end(readFileSync(found));
  }).listen(port, "127.0.0.1");
}

serve("dist", 5180);
serve("dist-pseudo", 5181);
