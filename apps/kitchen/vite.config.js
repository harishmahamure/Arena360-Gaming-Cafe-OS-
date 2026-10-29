import adapter from "@sveltejs/adapter-static";
import { sveltekit } from "@sveltejs/kit/vite";
import { defineConfig } from "vite";

// The Hub serves this build over plain HTTP at `:7401/kitchen` (Story 7.6), so every path starts
// with `/kitchen`. The Hub answers any extension-less address under it with `index.html`.
export default defineConfig({
  plugins: [
    sveltekit({
      adapter: adapter(),
      paths: { base: "/kitchen", relative: false },
    }),
  ],
  server: { port: 5175, strictPort: true },
});
