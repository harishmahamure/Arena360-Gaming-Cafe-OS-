import { defineConfig, devices } from "@playwright/test";

// The Kitchen display suite (Story 7.7): runs against `vite preview` of the Kitchen app's build;
// each test stands in for the Hub with `page.routeWebSocket`, so no Hub service is needed. It
// lives here because the Hub already carries Playwright and the browsers.
//
// Story 7.11: with `KITCHEN_DEVICES=1` it also runs on a Chrome-on-Android tablet and a
// Safari-on-iPad tablet (WebKit), the two browsers the release is proven on. Those need the
// `webkit` browser installed, so the ordinary run leaves them out. `KITCHEN_JSON=<file>` writes
// the run's results there (the release bench runs one project per file, see
// `scripts/kitchen-report.mjs`).
const port = 5176;
const devicesToo = process.env.KITCHEN_DEVICES === "1";
const json = process.env.KITCHEN_JSON;

export default defineConfig({
  testDir: "./tests/kitchen-board",
  fullyParallel: true,
  retries: 0,
  reporter: json ? [["list"], ["json", { outputFile: json }]] : "list",
  use: {
    baseURL: `http://127.0.0.1:${port}`,
  },
  projects: [
    {
      // A tablet in landscape.
      name: "tablet",
      use: { ...devices["Desktop Chrome"], viewport: { width: 1280, height: 800 } },
    },
    ...(devicesToo
      ? [
          { name: "android-chrome", use: { ...devices["Galaxy Tab S4 landscape"] } },
          { name: "ipad-safari", use: { ...devices["iPad (gen 7) landscape"] } },
        ]
      : []),
  ],
  webServer: {
    command: `pnpm --filter @arena/kitchen build && pnpm --filter @arena/kitchen exec vite preview --port ${port} --strictPort --host 127.0.0.1`,
    cwd: "../..",
    url: `http://127.0.0.1:${port}/kitchen/`,
    reuseExistingServer: false,
    timeout: 180_000,
  },
});
