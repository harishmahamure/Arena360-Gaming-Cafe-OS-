import { defineConfig, devices } from "@playwright/test";

// Onboarding suite: runs against `vite preview` of the build; each test stubs the cloud's
// `/v1` API with `page.route`, so no `arena-cloud` is needed.
const CI = Boolean(
  (globalThis as { process?: { env: Record<string, string | undefined> } }).process?.env.CI,
);

export default defineConfig({
  testDir: "./tests",
  fullyParallel: true,
  forbidOnly: CI,
  retries: 0,
  reporter: CI ? [["list"], ["html", { open: "never" }]] : "list",
  use: {
    baseURL: "http://localhost:5179",
  },
  projects: [
    {
      // Pure unit tests (no page): once is enough.
      name: "unit",
      testMatch: /\.unit\.spec\.ts$/,
    },
    {
      name: "desktop",
      testIgnore: /\.unit\.spec\.ts$/,
      use: { ...devices["Desktop Chrome"], viewport: { width: 1280, height: 800 } },
    },
    {
      name: "phone",
      testIgnore: /\.unit\.spec\.ts$/,
      use: { ...devices["Desktop Chrome"], viewport: { width: 360, height: 800 } },
    },
  ],
  webServer: {
    command: "pnpm build && pnpm exec vite preview",
    url: "http://localhost:5179",
    reuseExistingServer: false,
    timeout: 180_000,
  },
});
