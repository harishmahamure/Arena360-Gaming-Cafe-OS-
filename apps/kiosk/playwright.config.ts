import { defineConfig, devices } from "@playwright/test";

// Kiosk UI suite: runs against `vite preview` of the build. The Tauri shell is stubbed in the
// page (`window.__TAURI_INTERNALS__`), so the tests drive the same `hub-status` and
// `restore-state` events the shell emits.
const CI = Boolean(
  (globalThis as { process?: { env: Record<string, string | undefined> } }).process?.env.CI,
);

const port = Number(
  (globalThis as { process?: { env: Record<string, string | undefined> } }).process?.env
    .ARENA_PW_PORT ?? 5174,
);

export default defineConfig({
  testDir: "./tests",
  fullyParallel: true,
  forbidOnly: CI,
  retries: 0,
  reporter: CI ? [["list"], ["html", { open: "never" }]] : "list",
  use: {
    baseURL: `http://127.0.0.1:${port}`,
    ...devices["Desktop Chrome"],
    viewport: { width: 1920, height: 1080 },
  },
  webServer: {
    command: "pnpm build && pnpm exec vite preview",
    url: `http://127.0.0.1:${port}`,
    reuseExistingServer: false,
    timeout: 180_000,
  },
});
