import adapter from "@sveltejs/adapter-static";
import { sveltekit } from "@sveltejs/kit/vite";
import { defineConfig } from "vite";

const previewPort = Number(/** @type {any} */ (globalThis).process?.env?.ARENA_PW_PORT ?? 5174);

export default defineConfig({
  plugins: [sveltekit({ adapter: adapter({ fallback: "index.html" }) })],
  server: { port: 5174, strictPort: true },
  preview: { port: previewPort, strictPort: true, host: "127.0.0.1" },
});
