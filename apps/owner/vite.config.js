import adapter from "@sveltejs/adapter-cloudflare";
import { sveltekit } from "@sveltejs/kit/vite";
import { defineConfig } from "vite";

const previewPort = Number(/** @type {any} */ (globalThis).process?.env?.ARENA_PW_PORT ?? 5176);

export default defineConfig({
  plugins: [sveltekit({ adapter: adapter(), paths: { relative: false } })],
  server: { port: 5176, strictPort: true },
  preview: { port: previewPort, strictPort: true, host: "127.0.0.1" },
});
