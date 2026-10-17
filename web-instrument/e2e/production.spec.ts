import { test, expect } from "@playwright/test";

test("production build omits development simulator controls", async ({ page }) => {
  await page.goto("./");
  await expect.poll(async () => {
    try {
      return await page.evaluate(() => window.crossOriginIsolated && Boolean((window as any).estepsTerminal?.status().ready));
    } catch {
      return false;
    }
  }, { timeout: 30_000 }).toBe(true);

  await expect(page.getByTestId("empty-state-start-sim")).toHaveCount(0);
  await expect(page.getByTestId("btn-connect-sim")).toHaveCount(0);
  await expect(page.getByTestId("sim-model-select")).toHaveCount(0);
  await expect(page.locator("main").getByRole("button").first()).toBeVisible();
});
