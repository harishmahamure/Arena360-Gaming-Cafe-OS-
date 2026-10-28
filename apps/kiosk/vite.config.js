import adapter from "@sveltejs/adapter-static";
import { sveltekit } from "@sveltejs/kit/vite";
// @ts-ignore (this package has no Node types)
import { readFileSync } from "node:fs";
import { defineConfig } from "vite";

// The kiosk app's version is the workspace version (what the service reports to the Hub).
const cargo = /** @type {string} */ (
  readFileSync(new URL("../../Cargo.toml", import.meta.url), "utf8")
);
const version = /\[workspace\.package\][^[]*?\nversion\s*=\s*"([^"]+)"/.exec(cargo)?.[1] ?? "0.0.0";

const previewPort = Number(/** @type {any} */ (globalThis).process?.env?.ARENA_PW_PORT ?? 5174);

export default defineConfig({
  plugins: [sveltekit({ adapter: adapter({ fallback: "index.html" }) })],
  define: { __KIOSK_VERSION__: JSON.stringify(version) },
  server: { port: 5174, strictPort: true },
  preview: { port: previewPort, strictPort: true, host: "127.0.0.1" },
});
