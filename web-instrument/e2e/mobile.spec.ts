import { test, expect, type Locator, type Page } from "@playwright/test";

async function openInstrument(page: Page) {
  await page.goto("./");
  await expect.poll(async () => {
    try {
      return await page.evaluate(() => window.crossOriginIsolated &&
        Boolean((window as any).estepsTerminal?.status().ready));
    } catch { return false; }
  }, { timeout: 30_000 }).toBe(true);
  const dismissBtn = page.getByTestId("btn-dismiss-warning");
  if (await dismissBtn.isVisible()) {
    await dismissBtn.click();
  }
}

async function checkResponsiveBounds(page: Page, state: string) {
  for (const width of [320, 393, 768, 1024]) {
    await page.setViewportSize({ width, height: 851 });
    const dimensions = await page.evaluate(() => ({
      viewport: window.innerWidth,
      content: Math.max(document.documentElement.scrollWidth, document.body.scrollWidth),
      overflowing: [...document.querySelectorAll("body *")]
        .map((element) => ({
          tag: element.tagName,
          testid: element.getAttribute("data-testid"),
          className: typeof element.className === "string" ? element.className : "",
          right: Math.round(element.getBoundingClientRect().right),
          width: Math.round(element.getBoundingClientRect().width),
        }))
        .filter((element) => element.right > window.innerWidth + 1),
    }));
    expect(dimensions.viewport, `${state}: viewport respects ${width}px`).toBe(width);
    expect(dimensions.content, `${state}: no horizontal page overflow at ${width}px; offenders: ${JSON.stringify(dimensions.overflowing)}`)
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
  await page.evaluate(() => {
    (window as any).testPasskeyAnswer = (window as any).cranklinkInstrumentController.requestPasskey(new AbortController().signal);
  });
  const passkeyDialog = page.getByRole("dialog", { name: "Bluetooth passkey", exact: true });
  const dialogBounds = await passkeyDialog.boundingBox();
  expect(dialogBounds).not.toBeNull();
  expect(dialogBounds!.x).toBeGreaterThanOrEqual(0);
  expect(dialogBounds!.x + dialogBounds!.width).toBeLessThanOrEqual(321);
  expect(dialogBounds!.y).toBeGreaterThanOrEqual(0);
  expect(dialogBounds!.y + dialogBounds!.height).toBeLessThanOrEqual(852);
  await expect(passkeyDialog.getByLabel("Passkey", { exact: true })).toBeFocused();
  await checkTouchTarget(page.getByTestId("btn-passkey-cancel"));
  await page.getByTestId("btn-passkey-cancel").tap();
  await expect(passkeyDialog).toHaveCount(0);
  expect(await page.evaluate(() => (window as any).testPasskeyAnswer)).toBeNull();
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
  await checkResponsiveBounds(page, "connected");

  const navMenuBtn = page.getByLabel("Open navigation menu");
  await checkTouchTarget(navMenuBtn);
  await navMenuBtn.tap();
  await expect(page.getByTestId("mobile-device-summary")).toContainText("EP8 Cargo");
  const geometryBtn = page.getByTestId("nav-group-geometry");
  await checkTouchTarget(geometryBtn);
  await geometryBtn.tap();
  await page.getByTestId("btn-open-search").tap();
  const search = page.getByTestId("search-filter-input");
  await search.fill("tire");
  expect(await search.evaluate((element) => parseFloat(getComputedStyle(element).fontSize))).toBeGreaterThanOrEqual(16);
  const tire = page.getByTestId("param-row-tire-circumference");
  await expect(tire).toContainText("Live: 2280 mm");
  const increment = page.getByTestId("stepper-plus-tire-circumference");
  await checkTouchTarget(increment);
  await increment.tap();
  await increment.tap();
  await expect(tire).toContainText("Staged: 2282 mm");
  await checkResponsiveBounds(page, "staged");
  await page.screenshot({ path: testInfo.outputPath("mobile-staged.png"), fullPage: true });

  const review = page.getByTestId("btn-review-changes");
  await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight));
  await expect(review).toBeInViewport();
  await checkTouchTarget(review);
  await review.tap();
  const reviewDialog = page.getByTestId("review-dialog");
  await expect(reviewDialog).toBeInViewport({ ratio: 0.2 });
  await expect(reviewDialog.getByTestId("review-item-tire-circumference")).toContainText("2282 mm");
  // Measure after the dialog's entry animation settles.
  await page.waitForFunction(() => document.getAnimations().every((animation) => animation.playState !== "running"));
  await reviewDialog.getByTestId("ack-regulated").tap();
  await checkTouchTarget(page.getByTestId("btn-commit-batch"));
  await page.getByTestId("btn-commit-batch").tap();
  await expect(reviewDialog).toBeHidden();
  await expect(tire).toContainText("Live: 2282 mm");
  await expect(page.getByRole("region").getByText("Saved 1 setting")).toBeVisible();

  await page.getByLabel("Open navigation menu").tap();
  await page.getByTestId("nav-section-journal").tap();
  await expect(page.getByTestId("journal-section")).toBeVisible();
  await checkResponsiveBounds(page, "journal");
  await page.locator('[data-action="restore-snapshot"]').first().tap();
  await page.getByTestId("btn-confirm-restore").tap();
  await expect(page.getByRole("region").getByText("Restored the backup")).toBeVisible();
  await page.getByLabel("Open navigation menu").tap();
  await page.getByTestId("nav-group-geometry").tap();
  await expect(tire).toContainText("Live: 2280 mm");

  await page.getByLabel("Open navigation menu").tap();
  await page.getByTestId("nav-section-firmware").tap();
  await expect(page.getByTestId("firmware-section")).toBeVisible();
  await expect(page.getByTestId("btn-fw-flash-pair")).toBeDisabled();
  await checkResponsiveBounds(page, "firmware");
  await page.getByLabel("Open navigation menu").tap();
  await page.getByTestId("nav-section-diagnostics").tap();
  await expect(page.getByTestId("console-body")).toBeVisible();
  const consoleInput = page.getByTestId("console-input");
  expect(await consoleInput.evaluate((element) => parseFloat(getComputedStyle(element).fontSize))).toBeGreaterThanOrEqual(16);
  await checkResponsiveBounds(page, "expanded console");
  await consoleInput.fill("settings read tire-circumference");
  await page.getByLabel("Open navigation menu").tap();
  await page.getByTestId("nav-section-journal").tap();
  await expect(consoleInput).toBeHidden();
  await page.getByLabel("Open navigation menu").tap();
  await page.getByTestId("nav-section-firmware").tap();
  await expect(consoleInput).toBeHidden();
  await page.getByLabel("Open navigation menu").tap();
  await page.getByTestId("nav-section-diagnostics").tap();
  await expect(consoleInput).toHaveValue("");
  expect(failures, "No missing runtime assets or browser errors during the touch workflow").toEqual([]);
});
