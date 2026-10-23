import adapter from "@sveltejs/adapter-static";
import { sveltekit } from "@sveltejs/kit/vite";
import { defineConfig } from "vite";

// Dev-only component gallery: never deployed, never bundled into a surface.
export default defineConfig({
  plugins: [sveltekit({ adapter: adapter() })],
  server: { port: 5181, strictPort: true },
  preview: { port: 5181, strictPort: true },
});
