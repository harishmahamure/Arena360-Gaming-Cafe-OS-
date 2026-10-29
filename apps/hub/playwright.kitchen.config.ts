import { defineConfig, devices } from "@playwright/test";

// The Kitchen display suite (Story 7.7): runs against `vite preview` of the Kitchen app's build;
// each test stands in for the Hub with `page.routeWebSocket`, so no Hub service is needed. It
// lives here because the Hub already carries Playwright and the browsers.
const port = 5176;

export default defineConfig({
  testDir: "./tests/kitchen-board",
  fullyParallel: true,
  retries: 0,
  reporter: "list",
  use: {
    baseURL: `http://127.0.0.1:${port}`,
  },
  projects: [
    {
      // A tablet in landscape.
      name: "tablet",
      use: { ...devices["Desktop Chrome"], viewport: { width: 1280, height: 800 } },
    },
  ],
  webServer: {
    command: `pnpm --filter @arena/kitchen build && pnpm --filter @arena/kitchen exec vite preview --port ${port} --strictPort --host 127.0.0.1`,
    cwd: "../..",
    url: `http://127.0.0.1:${port}/kitchen/`,
    reuseExistingServer: false,
    timeout: 180_000,
  },
});
