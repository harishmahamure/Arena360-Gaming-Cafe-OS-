import adapter from "@sveltejs/adapter-static";
import { sveltekit } from "@sveltejs/kit/vite";
import { defineConfig } from "vite";

const previewPort = Number(/** @type {any} */ (globalThis).process?.env?.ARENA_PW_PORT ?? 5173);

export default defineConfig({
  plugins: [sveltekit({ adapter: adapter({ fallback: "index.html" }) })],
  // The Hub service's CORS allows this origin for dev and preview (Playwright runs on preview).
  server: { port: 5173, strictPort: true },
  preview: { port: previewPort, strictPort: true, host: "127.0.0.1" },
});
