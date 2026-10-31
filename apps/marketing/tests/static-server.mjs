// A static file server shaped like Workers static assets with `not_found_handling: "404-page"`:
// an unknown path gets 404.html. Shared by the Playwright suite and the Lighthouse gate.

import { existsSync, readFileSync, statSync } from "node:fs";
import { createServer } from "node:http";
import { extname, join, normalize, sep } from "node:path";

const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".css": "text/css",
  ".js": "text/javascript",
  ".xml": "application/xml",
  ".txt": "text/plain",
  ".woff2": "font/woff2",
  ".svg": "image/svg+xml",
  ".ico": "image/x-icon",
  ".png": "image/png",
  ".json": "application/json",
};

/** Serves the folder `base` (absolute) on `port`; resolves once it is listening. */
export function serve(base, port) {
  return new Promise((resolve) => {
    const server = createServer((request, response) => {
      let path;
      try {
        path = decodeURIComponent(new URL(request.url ?? "/", "http://x").pathname);
      } catch {
        response.writeHead(400).end();
        return;
      }
      const candidates = [path, join(path, "index.html"), path + ".html"].map((p) =>
        normalize(join(base, p)),
      );
      const file = candidates.find(
        (p) => p.startsWith(base + sep) && existsSync(p) && statSync(p).isFile(),
      );
      const found = file ?? join(base, "404.html");
      response.writeHead(file ? 200 : 404, {
        "Content-Type": TYPES[extname(found)] ?? "application/octet-stream",
      });
      response.end(readFileSync(found));
    });
    server.listen(port, "127.0.0.1", () => resolve(server));
  });
}
