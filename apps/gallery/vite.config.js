import adapter from "@sveltejs/adapter-static";
import { sveltekit } from "@sveltejs/kit/vite";
import { defineConfig } from "vite";

// Dev-only component gallery: never deployed, never bundled into a surface.
const previewPort = Number(/** @type {any} */ (globalThis).process?.env?.ARENA_PW_PORT ?? 5181);

export default defineConfig({
  plugins: [sveltekit({ adapter: adapter() })],
  server: { port: 5181, strictPort: true },
  preview: { port: previewPort, strictPort: true, host: "127.0.0.1" },
});
