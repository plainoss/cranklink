import { defineConfig, devices } from "@playwright/test";

// Test the built app so edits in a parallel development session cannot reload
// a real firmware transaction through Vite HMR. Build assets before this suite.
const deployed = process.env.PLAYWRIGHT_BASE_URL;
export default defineConfig({
  testDir: "./e2e",
  testMatch: "stock-components.spec.ts",
  timeout: 120_000,
  expect: { timeout: 15_000 },
  workers: 1,
  use: {
    ...devices["Desktop Chrome"],
    baseURL: deployed || "http://127.0.0.1:8032/",
    trace: "retain-on-failure",
  },
  webServer: deployed ? undefined : {
    command: "python3 ../.github/scripts/serve-web-build.py --port 8032",
    url: "http://127.0.0.1:8032/",
    reuseExistingServer: false,
    timeout: 15_000,
  },
});
