import adapter from "@sveltejs/adapter-cloudflare";
import { sveltekit } from "@sveltejs/kit/vite";
import { defineConfig } from "vite";

// `/v1` goes to `arena-cloud --role all --dev` on 127.0.0.1:8080 in dev and preview, so the
// browser sees one origin (production sets PUBLIC_API_BASE to the API host instead).
const proxy = { "/v1": { target: "http://127.0.0.1:8080", changeOrigin: false } };

export default defineConfig({
  plugins: [sveltekit({ adapter: adapter() })],
  server: { port: 5179, strictPort: true, proxy },
  preview: { port: 5179, strictPort: true, proxy },
});
