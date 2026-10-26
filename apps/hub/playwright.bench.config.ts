import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "./tests/bench",
  workers: 1,
  retries: 0,
  reporter: "list",
  use: { baseURL: "http://127.0.0.1:5173" },
  projects: [
    {
      name: "reference",
      use: { ...devices["Desktop Chrome"], viewport: { width: 1920, height: 1080 } },
    },
  ],
  webServer: {
    command: "pnpm build && pnpm exec vite preview",
    url: "http://127.0.0.1:5173",
    reuseExistingServer: false,
    timeout: 180_000,
  },
});
