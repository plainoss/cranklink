import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "./e2e",
  timeout: 45_000,
  expect: {
    timeout: 15_000,
  },
  fullyParallel: false,
  workers: 1,
  use: {
    baseURL: "http://127.0.0.1:8030",
    trace: "on-first-retry",
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
    command: "npm run dev -- --host 127.0.0.1 --port 8030",
    url: "http://127.0.0.1:8030",
    reuseExistingServer: !process.env.CI,
    timeout: 30_000,
  },
});
