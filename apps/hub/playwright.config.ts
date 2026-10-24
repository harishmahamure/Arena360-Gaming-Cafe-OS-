import { defineConfig, devices } from "@playwright/test";

// Hub shell suite: runs against `vite preview` of the build; each test serves the branch
// snapshot with `page.route`, so no Hub service is needed.
const CI = Boolean(
  (globalThis as { process?: { env: Record<string, string | undefined> } }).process?.env.CI,
);

const port = Number(
  (globalThis as { process?: { env: Record<string, string | undefined> } }).process?.env
    .ARENA_PW_PORT ?? 5173,
);

export default defineConfig({
  testDir: "./tests",
  // The WAN-down smoke runs against a real Hub (`playwright.wan-down.config.ts`).
  testIgnore: "**/wan-down/**",
  fullyParallel: true,
  forbidOnly: CI,
  retries: 0,
  reporter: CI ? [["list"], ["html", { open: "never" }]] : "list",
  use: {
    baseURL: `http://127.0.0.1:${port}`,
  },
  projects: [
    {
      name: "desktop",
      use: { ...devices["Desktop Chrome"], viewport: { width: 1280, height: 800 } },
    },
    {
      // Below 840px the rail becomes the bottom bar.
      name: "phone",
      use: { ...devices["Desktop Chrome"], viewport: { width: 400, height: 800 } },
    },
  ],
  webServer: {
    command: "pnpm build && pnpm exec vite preview",
    url: `http://127.0.0.1:${port}`,
    reuseExistingServer: false,
    timeout: 180_000,
  },
});
