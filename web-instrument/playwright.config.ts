import { defineConfig, devices } from "@playwright/test";

// CRANKLINK_E2E_PORT lets worktrees run the suite side by side.
const port = Number(process.env.CRANKLINK_E2E_PORT) || 8030;

export default defineConfig({
  testDir: "./e2e",
  // Both have dedicated configs: built assets, and serial stock-firmware runs.
  testIgnore: ["**/production.spec.ts", "**/stock-components.spec.ts"],
  timeout: 45_000,
  expect: {
    timeout: 15_000,
  },
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  workers: process.env.CI ? 4 : undefined,
  use: {
    baseURL: `http://127.0.0.1:${port}`,
    // There are no retries, so "on-first-retry" would never record a trace.
    trace: "retain-on-failure",
    viewport: { width: 1280, height: 840 },
  },
  projects: [
    {
      name: "chromium",
      testMatch: /.*\.spec\.ts/,
      use: { ...devices["Desktop Chrome"], hasTouch: true },
    },
  ],
  webServer: {
    // Loopback only: the dev server serves every file under web-instrument/.
    command: `npm run dev -- --host 127.0.0.1 --port ${port} --strictPort`,
    url: `http://127.0.0.1:${port}`,
    // Opt-in: a server already on the port may be another worktree's code.
    reuseExistingServer: !process.env.CI && process.env.CRANKLINK_E2E_REUSE === "1",
    timeout: 30_000,
  },
});
