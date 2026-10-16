import { test, expect, type Locator, type Page } from "@playwright/test";

async function openInstrument(page: Page) {
  await page.goto("./");
  await expect.poll(async () => {
    try {
      return await page.evaluate(() => window.crossOriginIsolated &&
        Boolean((window as any).estepsTerminal?.status().ready));
    } catch { return false; } // The service worker reloads a first visit.
  }, { timeout: 30_000 }).toBe(true);
}

async function checkResponsiveBounds(page: Page, state: string) {
  for (const width of [320, 393, 768, 1024]) {
    await page.setViewportSize({ width, height: 851 });
    const dimensions = await page.evaluate(() => ({
      viewport: window.innerWidth,
      content: Math.max(document.documentElement.scrollWidth, document.body.scrollWidth),
    }));
    expect(dimensions.viewport, `${state}: viewport respects ${width}px`).toBe(width);
    expect(dimensions.content, `${state}: no horizontal page overflow at ${width}px`)
      .toBeLessThanOrEqual(width + 1);
  }
  await page.setViewportSize({ width: 393, height: 851 });
}

async function checkTouchTarget(control: Locator) {
  const bounds = await control.boundingBox();
  expect(bounds, "Touch control has a rendered target").not.toBeNull();
  expect(bounds!.width).toBeGreaterThanOrEqual(43.9);
  expect(bounds!.height).toBeGreaterThanOrEqual(43.9);
}

test("phone touch workflow and responsive bounds through recovery views", async ({ page }, testInfo) => {
  test.setTimeout(110_000);
  const failures: string[] = [];
  page.on("pageerror", (error) => failures.push(error.message));
  page.on("response", (response) => {
    if (response.status() >= 400 && !response.url().endsWith("/favicon.ico")) {
      failures.push(`${response.status()} ${response.url()}`);
    }
  });
  await openInstrument(page);
  const viewportMeta = await page.locator('meta[name="viewport"]').getAttribute("content");
  expect(viewportMeta).not.toMatch(/maximum-scale\s*=\s*1(?:[,.\s]|$)|user-scalable\s*=\s*no/i);
  await checkResponsiveBounds(page, "disconnected");
  // Exercise the small-screen passkey layout without requesting any radio access.
  await page.setViewportSize({ width: 320, height: 851 });
  const passkeyDialog = page.locator("#ble-passkey-dialog");
  await passkeyDialog.evaluate((element: HTMLDialogElement) => element.showModal());
  const dialogBounds = await passkeyDialog.boundingBox();
  expect(dialogBounds).not.toBeNull();
  expect(dialogBounds!.x).toBeGreaterThanOrEqual(0);
  expect(dialogBounds!.x + dialogBounds!.width).toBeLessThanOrEqual(321);
  expect(dialogBounds!.y).toBeGreaterThanOrEqual(0);
  expect(dialogBounds!.y + dialogBounds!.height).toBeLessThanOrEqual(852);
  await checkTouchTarget(page.locator("#ble-passkey-cancel"));
  await passkeyDialog.evaluate((element: HTMLDialogElement) => element.close());
  await page.setViewportSize({ width: 393, height: 851 });

  await checkTouchTarget(page.getByTestId("btn-more-menu"));
  await page.getByTestId("btn-more-menu").tap();
  for (const width of [320, 393, 768, 1024]) {
    await page.setViewportSize({ width, height: 851 });
    const bounds = await page.locator(".menu-dropdown").boundingBox();
    expect(bounds).not.toBeNull();
    expect(bounds!.x, `menu left edge at ${width}px`).toBeGreaterThanOrEqual(-1);
    expect(bounds!.x + bounds!.width, `menu right edge at ${width}px`).toBeLessThanOrEqual(width + 1);
  }
  await page.setViewportSize({ width: 393, height: 851 });
  await checkTouchTarget(page.getByTestId("btn-connect-sim"));
  await page.getByTestId("btn-connect-sim").tap();
  await expect(page.getByTestId("header-status-badge")).toContainText("SIM: DU-EP800-CRG");
  await checkResponsiveBounds(page, "connected");

  const groups = page.getByTestId("mobile-parameter-group");
  await expect(groups).toBeVisible();
  await groups.focus();
  await groups.selectOption("all");
  await expect(groups).toBeFocused();
  await groups.selectOption("geometry");
  await expect(groups).toBeFocused();
  const search = page.getByTestId("search-filter-input");
  await search.fill("tire");
  expect(await search.evaluate((element) => parseFloat(getComputedStyle(element).fontSize))).toBeGreaterThanOrEqual(16);
  const tire = page.getByTestId("param-row-tire-circumference");
  await expect(tire).toContainText("Live: 2280 mm");
  const increment = page.getByTestId("stepper-plus-tire-circumference");
  await checkTouchTarget(increment);
  await increment.tap();
  await increment.tap();
  await expect(tire).toContainText("Staged: 2300 mm");
  await checkResponsiveBounds(page, "staged");
  await page.screenshot({ path: testInfo.outputPath("mobile-staged.png"), fullPage: true });

  const review = page.getByTestId("btn-review-changes");
  await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight));
  await expect(review).toBeInViewport();
  await checkTouchTarget(review);
  await review.tap();
  const transaction = page.getByTestId("transaction-rail");
  await expect(transaction).toBeInViewport({ ratio: 0.2 });
  await expect(transaction).toContainText("2280 mm → 2300 mm");
  await page.getByTestId("btn-commit-batch").tap();
  await expect(transaction).toBeHidden();
  await expect(tire).toContainText("Live: 2300 mm");
  await expect(page.getByTestId("status-message")).toContainText("Committed 1 change");

  await page.getByTestId("nav-section-journal").tap();
  await expect(page.getByTestId("journal-section")).toBeVisible();
  await checkResponsiveBounds(page, "journal");
  await page.locator('[data-action="restore-snapshot"]').first().tap();
  await expect(page.getByTestId("status-message")).toContainText("Restored ");
  await expect(groups).toHaveValue("");
  await groups.selectOption("geometry");
  await expect(tire).toContainText("Live: 2280 mm");

  await page.getByTestId("nav-section-firmware").tap();
  await expect(page.getByTestId("firmware-section")).toBeVisible();
  await expect(page.getByTestId("btn-fw-flash-pair")).toBeDisabled();
  await checkResponsiveBounds(page, "firmware");
  await page.getByTestId("console-toggle").tap();
  await expect(page.getByTestId("console-body")).toBeVisible();
  const consoleInput = page.getByTestId("console-input");
  expect(await consoleInput.evaluate((element) => parseFloat(getComputedStyle(element).fontSize))).toBeGreaterThanOrEqual(16);
  await checkResponsiveBounds(page, "expanded console");
  await consoleInput.fill("settings read tire-circumference");
  await page.getByTestId("nav-section-journal").tap();
  await expect(consoleInput).toHaveValue("settings read tire-circumference");
  await page.getByTestId("nav-section-firmware").tap();
  await expect(consoleInput).toHaveValue("settings read tire-circumference");
  await page.getByTestId("console-toggle").tap();
  await expect(consoleInput).toBeHidden();
  await page.getByTestId("console-toggle").tap();
  await expect(consoleInput).toHaveValue("settings read tire-circumference");
  expect(failures, "No missing runtime assets or browser errors during the touch workflow").toEqual([]);
});
