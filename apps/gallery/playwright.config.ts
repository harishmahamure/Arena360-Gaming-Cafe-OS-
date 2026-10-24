import { defineConfig, devices } from "@playwright/test";

// Accessibility floor suite for the dev-only gallery. Runs against `vite preview` of the build.
const CI = Boolean(
  (globalThis as { process?: { env: Record<string, string | undefined> } }).process?.env.CI,
);

const port = Number(
  (globalThis as { process?: { env: Record<string, string | undefined> } }).process?.env
    .ARENA_PW_PORT ?? 5181,
);

export default defineConfig({
  testDir: "./tests",
  fullyParallel: true,
  forbidOnly: CI,
  retries: 0,
  reporter: CI ? [["list"], ["html", { open: "never" }]] : "list",
  use: {
    baseURL: `http://127.0.0.1:${port}`,
    viewport: { width: 1280, height: 900 },
  },
  projects: [
    { name: "chromium", use: { ...devices["Desktop Chrome"] } },
    {
      // Phone width: targets stay 44px and the pseudo-locale still fits.
      name: "phone",
      grep: /targets:|pseudo-locale:/,
      use: { ...devices["Desktop Chrome"], viewport: { width: 360, height: 800 } },
    },
  ],
  webServer: {
    command: "pnpm build && pnpm exec vite preview",
    url: `http://127.0.0.1:${port}`,
    reuseExistingServer: false,
    timeout: 180_000,
  },
});
