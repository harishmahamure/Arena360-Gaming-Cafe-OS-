import { defineConfig, devices } from "@playwright/test";

const env = (globalThis as { process?: { env: Record<string, string | undefined> } }).process?.env;
const port = Number(env?.ARENA_PW_PORT ?? 5176);

export default defineConfig({
  testDir: "./tests",
  fullyParallel: true,
  forbidOnly: Boolean(env?.CI),
  retries: 0,
  reporter: env?.CI ? [["list"], ["html", { open: "never" }]] : "list",
  use: { baseURL: `http://127.0.0.1:${port}` },
  projects: [
    {
      name: "desktop",
      use: { ...devices["Desktop Chrome"], viewport: { width: 1280, height: 800 } },
    },
    { name: "phone", use: { ...devices["Desktop Chrome"], viewport: { width: 360, height: 800 } } },
  ],
  webServer: {
    command: "pnpm build && pnpm exec vite preview",
    url: `http://127.0.0.1:${port}`,
    reuseExistingServer: false,
    timeout: 180_000,
  },
});
