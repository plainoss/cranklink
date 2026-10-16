import { defineConfig, devices } from "@playwright/test";

const deployed = process.env.PLAYWRIGHT_BASE_URL;
export default defineConfig({
  testDir: "./e2e",
  timeout: 60_000,
  expect: { timeout: 20_000 },
  fullyParallel: false,
  workers: 1,
  use: {
    baseURL: deployed || "http://127.0.0.1:8031/cranklink/",
    trace: "retain-on-failure",
    viewport: { width: 1280, height: 840 },
  },
  projects: [
    {
      name: "chromium",
      testMatch: "instrument.spec.ts",
      use: { ...devices["Desktop Chrome"] },
    },
    {
      name: "mobile-chromium",
      testMatch: "mobile.spec.ts",
      use: {
        ...devices["Pixel 5"],
        viewport: { width: 393, height: 851 },
        isMobile: true,
        hasTouch: true,
      },
    },
  ],
  webServer: deployed ? undefined : {
    command: "python3 ../.github/scripts/serve-web-build.py --port 8031",
    url: "http://127.0.0.1:8031/cranklink/",
    reuseExistingServer: false,
    timeout: 15_000,
  },
});
