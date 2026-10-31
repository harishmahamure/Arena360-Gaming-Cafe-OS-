import { defineConfig, devices } from "@playwright/test";

// Marketing suite: runs against the static build, served like Workers static assets (tests/serve.mjs).
// A second build with `PUBLIC_PSEUDO=1` (every string accented and 30% longer) backs the layout check.
const CI = Boolean(
  (globalThis as { process?: { env: Record<string, string | undefined> } }).process?.env.CI,
);

const PORT = 5180;
const PSEUDO_PORT = 5181;

export default defineConfig({
  testDir: "./tests",
  fullyParallel: true,
  forbidOnly: CI,
  retries: 0,
  reporter: CI ? [["list"], ["html", { open: "never" }]] : "list",
  use: { ...devices["Desktop Chrome"] },
  projects: [
    {
      name: "site",
      testIgnore: /pseudo\.spec\.ts$/,
      use: { baseURL: `http://127.0.0.1:${PORT}` },
    },
    {
      name: "pseudo",
      testMatch: /pseudo\.spec\.ts$/,
      use: { baseURL: `http://127.0.0.1:${PSEUDO_PORT}` },
    },
  ],
  webServer: {
    command: "node tests/serve.mjs",
    url: `http://127.0.0.1:${PORT}/robots.txt`,
    reuseExistingServer: false,
    timeout: 240_000,
  },
});
