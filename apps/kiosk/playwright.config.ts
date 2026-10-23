import { defineConfig, devices } from "@playwright/test";

// Kiosk UI suite: runs against `vite preview` of the build. The Tauri shell is stubbed in the
// page (`window.__TAURI_INTERNALS__`), so the tests drive the same `hub-status` and
// `restore-state` events the shell emits.
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
    baseURL: "http://localhost:5174",
    ...devices["Desktop Chrome"],
    viewport: { width: 1920, height: 1080 },
  },
  webServer: {
    command: "pnpm build && pnpm exec vite preview",
    url: "http://localhost:5174",
    reuseExistingServer: false,
    timeout: 180_000,
  },
});
