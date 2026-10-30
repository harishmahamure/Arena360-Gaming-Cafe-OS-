import { defineConfig, devices } from "@playwright/test";

const env = (globalThis as { process?: { env: Record<string, string | undefined> } }).process?.env;
const port = Number(env?.ARENA_PW_PORT ?? 5176);
const json = env?.OWNER_JSON;

// Story 8.42: `reference-phone` runs only `first-render.spec.ts` (Today's first render on the
// reference phone, see that file); `OWNER_JSON=<file>` writes the run's results for
// `scripts/remote-report.mjs`.
export default defineConfig({
  testDir: "./tests",
  fullyParallel: true,
  forbidOnly: Boolean(env?.CI),
  retries: 0,
  reporter: json
    ? [["list"], ["json", { outputFile: json }]]
    : env?.CI
      ? [["list"], ["html", { open: "never" }]]
      : "list",
  use: { baseURL: `http://127.0.0.1:${port}` },
  projects: [
    {
      name: "desktop",
      testIgnore: /first-render/,
      use: { ...devices["Desktop Chrome"], viewport: { width: 1280, height: 800 } },
    },
    {
      name: "phone",
      testIgnore: /first-render/,
      use: { ...devices["Desktop Chrome"], viewport: { width: 360, height: 800 } },
    },
    { name: "reference-phone", testMatch: /first-render/, use: { ...devices["Moto G4"] } },
  ],
  webServer: {
    command: "pnpm build && pnpm exec vite preview",
    url: `http://127.0.0.1:${port}`,
    reuseExistingServer: false,
    timeout: 180_000,
  },
});
