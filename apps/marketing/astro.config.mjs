import svelte from "@astrojs/svelte";
import { defineConfig } from "astro/config";

export default defineConfig({
  output: "static",
  server: { port: 5180 },
  integrations: [svelte()],
});
