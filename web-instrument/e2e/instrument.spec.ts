import { test, expect, type Page } from "@playwright/test";

const runtimeFailures = new WeakMap<Page, string[]>();
test.beforeEach(({ page }) => {
  const failures: string[] = [];
  runtimeFailures.set(page, failures);
  page.on("pageerror", (error) => failures.push(error.message));
  page.on("response", (response) => {
    if (response.status() >= 400 && !response.url().endsWith("/favicon.ico")) {
      failures.push(`${response.status()} ${response.url()}`);
    }
  });
  page.on("requestfailed", (request) => {
    if (!request.failure()?.errorText.includes("ERR_ABORTED")) {
      failures.push(`${request.failure()?.errorText}: ${request.url()}`);
    }
  });
});
test.afterEach(({ page }) => {
  expect(runtimeFailures.get(page), "No browser errors or missing runtime assets").toEqual([]);
});

async function openInstrument(page: Page) {
  await page.goto("./");
  await expect.poll(async () => {
    try {
      return await page.evaluate(() => window.crossOriginIsolated &&
        Boolean((window as any).estepsTerminal?.status().ready));
    } catch { return false; } // A first-visit service worker may reload the page.
  }, { timeout: 30_000 }).toBe(true);
  if (page.url().includes("/cranklink/")) {
    await expect.poll(() => page.evaluate(() =>
      navigator.serviceWorker.controller?.scriptURL || "")).toContain("/cranklink/coi-serviceworker.js");
  }
}

test.describe("cranklink Drive Unit Field Instrument (WASM + Wire Simulator)", () => {
  test("zero-click connect, inline staging, atomic batch commit, and snapshot rollback", async ({
    page,
  }) => {
    await openInstrument(page);

    // Verify clean initial disconnected state
    await expect(page.getByTestId("header-status-badge")).toContainText("DISCONNECTED");
    await expect(page.getByTestId("empty-state-start-sim")).toBeVisible();

    // 1. Connect to DU-EP800-CRG simulator via "..." menu — executes connect + probe + schema load + settings read
    await page.getByTestId("btn-more-menu").click();
    await page.getByTestId("btn-connect-sim").click();

    await expect(page.getByTestId("header-status-badge")).toContainText("SIM: DU-EP800-CRG");
    await expect(page.getByTestId("identity-strip")).toContainText("SN: 010203040506");
    await expect(page.getByTestId("identity-strip")).toContainText("DCAS: 4.2.0.0");

    // Reload under the existing service worker and reconnect to persisted storage.
    await page.reload();
    await openInstrument(page);
    await page.getByTestId("btn-more-menu").click();
    await page.getByTestId("btn-connect-sim").click();
    await expect(page.getByTestId("header-status-badge")).toContainText("SIM: DU-EP800-CRG");

    // 2. Verify Bike Hardware & Geometry parameters are loaded with live values
    const tireRow = page.getByTestId("param-row-tire-circumference");
    await expect(tireRow).toBeVisible();
    await expect(tireRow).toContainText("Live: 2280 mm");

    // 3. Stage changes inline (step tire circumference +10mm twice -> 2300 mm)
    await page.getByTestId("stepper-plus-tire-circumference").click();
    await page.getByTestId("stepper-plus-tire-circumference").click();
    await expect(tireRow).toContainText("Staged: 2300 mm");

    // Transaction rail appears automatically with staged changes
    const txRail = page.getByTestId("transaction-rail");
    await expect(txRail).toBeVisible();
    await expect(txRail).toContainText("Staged Changes (1)");
    await expect(page.getByTestId("tx-item-tire-circumference")).toContainText("2280 mm → 2300 mm");

    // 4. Commit batch via real Rust WASM `settings batch` into GATT simulator
    await page.getByTestId("btn-commit-batch").click();

    // Transaction rail clears and live value updates to 2300 mm
    await expect(txRail).toBeHidden();
    await expect(tireRow).toContainText("Live: 2300 mm");
    await expect(page.getByTestId("status-message")).toContainText("Committed 1 change");

    // 5. Verify pre-write backup was automatically persisted in Snapshots & Rollback journal
    await page.getByTestId("nav-section-journal").click();
    await expect(page.getByTestId("journal-section")).toBeVisible();

    const restoreBtn = page.locator('[data-action="restore-snapshot"]').first();
    await expect(restoreBtn).toBeVisible();

    // Restore pre-write backup value (2280 mm)
    await restoreBtn.click();
    await expect(page.getByTestId("status-message")).toContainText("Restored ");

    // Navigate back to parameters and confirm live value returned to 2280 mm
    await page.getByTestId("nav-group-geometry").click();
    await expect(page.getByTestId("param-row-tire-circumference")).toContainText("Live: 2280 mm");
  });

  test("modern DU-EP600 / DU-EP801 session authentication and preset macros", async ({ page }) => {
    await openInstrument(page);

    // Select DU-EP801 flagship simulator (requires --ble-unit-auth and serial binding)
    await page.getByTestId("btn-more-menu").click();
    await page.getByTestId("sim-model-select").selectOption("ep801");
    await page.getByTestId("btn-connect-sim").click();

    await expect(page.getByTestId("header-status-badge")).toContainText("SIM: DU-EP801");
    await expect(page.getByTestId("identity-strip")).toContainText("SN: 010203040506");
    await expect(page.getByTestId("identity-strip")).toContainText("DCAS: 4.6.0.1");

    // Apply "Trail Climbing (85 Nm Boost)" preset macro
    await page.getByTestId("macro-trail-85nm").click();
    await expect(page.getByTestId("transaction-rail")).toBeVisible();

    // Commit macro batch to EP801
    await page.getByTestId("btn-commit-batch").click();
    await expect(page.getByTestId("transaction-rail")).toBeHidden();
    await expect(page.getByTestId("status-message")).toContainText("Committed");
  });

  test("synchronized Dual-MCU firmware workflow (DCAS + RX pair by default)", async ({ page }) => {
    await openInstrument(page);
    await page.getByTestId("btn-more-menu").click();
    await page.getByTestId("btn-connect-sim").click();
    await expect(page.getByTestId("header-status-badge")).toContainText("SIM: DU-EP800-CRG");

    // Navigate to Synchronized Dual-MCU Firmware section
    await page.getByTestId("nav-section-firmware").click();
    await expect(page.getByTestId("firmware-section")).toBeVisible();
    await expect(page.getByTestId("companion-status")).toContainText(
      "Select a matched DCAS (-D) and RX (-M) firmware pair to begin.",
    );

    // Primary buttons are disabled until a matched companion pair is loaded
    await expect(page.getByTestId("btn-fw-dry-run-pair")).toBeDisabled();
    await expect(page.getByTestId("btn-fw-flash-pair")).toBeDisabled();

    // Verify single-MCU recovery override is collapsed by default inside <details>
    const overrideDetails = page.getByTestId("single-mcu-override");
    await expect(overrideDetails).not.toHaveAttribute("open", "");
  });

  test("WebMCP external agent authorization flow", async ({ page }) => {
    await openInstrument(page);
    await page.getByTestId("btn-more-menu").click();
    await page.getByTestId("btn-connect-sim").click();
    await expect(page.getByTestId("header-status-badge")).toContainText("SIM: DU-EP800-CRG");

    // Trigger an external agent write request via window.webMcpServer.handleRequest
    const rpcPromise = page.evaluate(() => {
      // @ts-ignore
      return window.webMcpServer.handleRequest("tools/call", {
        name: "esteps_set_setting",
        arguments: {
          name: "tire-circumference",
          value: 2150,
        },
      });
    });

    // Operator authorization banner appears inline
    const approvalBanner = page.getByTestId("mcp-approval-banner");
    await expect(approvalBanner).toBeVisible();
    await expect(approvalBanner).toContainText("Tire Circumference: 2280 mm → 2150 mm");

    // Operator clicks Authorize Write
    await page.getByTestId("btn-mcp-approve").click();

    const rpcResult: any = await rpcPromise;
    expect(rpcResult.completed).toBe(1);

    // Verify live parameter updated to 2150 mm
    await expect(page.getByTestId("param-row-tire-circumference")).toContainText("Live: 2150 mm");
  });
});
