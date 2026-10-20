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

  // Verify unofficial software warning modal is present and can be dismissed
  const modal = page.getByTestId("risk-warning-modal");
  await expect(modal).toBeVisible();
  await expect(modal).toContainText("Unofficial Software & Hardware Risk");
  await expect(modal).toContainText("Tested on EP800-CRG only");
  await expect(modal).toContainText("Limited compatibility checks");
  await page.getByTestId("btn-dismiss-warning").click();
  await expect(modal).toBeHidden();

  await expect(page.getByTestId("empty-state-start-sim")).toHaveCount(0);
  await expect(page.getByTestId("btn-connect-sim")).toHaveCount(0);
  await expect(page.getByTestId("sim-model-select")).toHaveCount(0);
  await expect(page.locator("main").getByRole("button").first()).toBeVisible();

  // Verify modal can be reopened via the notice link
  await page.getByTestId("btn-open-warning-modal").click();
  await expect(modal).toBeVisible();
  await page.getByTestId("btn-dismiss-warning").click();
  await expect(modal).toBeHidden();
});
