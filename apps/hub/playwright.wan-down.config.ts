import { defineConfig, devices } from "@playwright/test";

// The WAN-down smoke (Story 1.22, `scripts/wan-down/run.sh`): sign-in, floor and settings
// against a real, dev-seeded Hub on 127.0.0.1:7409 with its kiosk connected, served from the
// already-built bundle (`pnpm --filter @arena/hub build` first). Nothing is stubbed.
export default defineConfig({
  testDir: "./tests/wan-down",
  fullyParallel: false,
  workers: 1,
  retries: 0,
  reporter: "list",
  use: {
    baseURL: "http://localhost:5173",
  },
  projects: [
    {
      name: "desktop",
      use: { ...devices["Desktop Chrome"], viewport: { width: 1280, height: 800 } },
    },
  ],
  webServer: {
    command: "pnpm exec vite preview",
    url: "http://localhost:5173",
    reuseExistingServer: false,
    timeout: 60_000,
  },
});
