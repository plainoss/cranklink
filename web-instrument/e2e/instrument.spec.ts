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
    } catch { return false; }
  }, { timeout: 30_000 }).toBe(true);
  const dismissBtn = page.getByTestId("btn-dismiss-warning");
  if (await dismissBtn.isVisible()) {
    await dismissBtn.click();
  }
}

test.describe("cranklink Drive Unit Field Instrument (WASM + Wire Simulator)", () => {
  test("zero-click connect, inline staging, ordered batch commit, and snapshot rollback @smoke", async ({
    page,
  }) => {
    await openInstrument(page);

    // The landing screen keeps the next action prominent and omits an idle status badge.
    await expect(page.locator("main").getByRole("button").first()).toBeVisible();
    await expect(page.getByTestId("empty-state-start-sim")).toBeVisible();

    // 1. Connect to the DU-EP800-CRG simulator — executes connect + probe + schema load + settings read
    await page.getByTestId("btn-connect-sim").click();

    await expect(page.getByTestId("identity-strip")).toContainText("EP800-CRG");
    await expect(page.getByTestId("identity-strip")).toContainText("SN: 010203040506");
    await expect(page.getByTestId("identity-strip")).toContainText("DCAS: 4.2.0.0");

    // 2. Verify Bike Hardware & Geometry parameters are loaded with live values
    const tireRow = page.getByTestId("param-row-tire-circumference");
    await expect(tireRow).toBeVisible();
    await expect(tireRow).toContainText("Live: 2280 mm");

    // 3. Stage changes inline (step tire circumference +1 mm twice -> 2282 mm)
    await page.getByTestId("stepper-plus-tire-circumference").click();
    await page.getByTestId("stepper-plus-tire-circumference").click();
    await expect(tireRow).toContainText("Staged: 2282 mm");

    // Transaction rail appears automatically with staged changes
    const txRail = page.getByTestId("transaction-rail");
    await expect(txRail).toBeVisible();
    await expect(txRail).toContainText("1 change to save");
    await expect(page.getByTestId("tx-item-tire-circumference")).toContainText("Tire Circumference: 2280 mm → 2282 mm");

    // 4. Every save goes through the review dialog, then the real Rust WASM `settings batch`
    await page.getByTestId("btn-review-save").click();
    const review = page.getByTestId("review-dialog");
    await expect(review.getByTestId("review-item-tire-circumference")).toContainText("2282 mm");
    await expect(review.getByLabel("Regulated setting")).toBeVisible();
    await review.getByTestId("btn-commit-batch").click();

    // Transaction rail clears, live value updates, and a confirmation names the backup
    await expect(txRail).toBeHidden();
    await expect(tireRow).toContainText("Live: 2282 mm");
    await expect(page.getByRole("region").getByText("Saved 1 setting")).toBeVisible();

    // 5. Verify pre-write backup was automatically persisted in Snapshots & Rollback journal
    await page.getByTestId("nav-section-journal").click();
    await expect(page.getByTestId("journal-section")).toBeVisible();

    const restoreBtn = page.locator('[data-action="restore-snapshot"]').first();
    await expect(restoreBtn).toBeVisible();

    // Restore pre-write backup value (2280 mm) after confirming what it restores
    await restoreBtn.click();
    await page.getByTestId("btn-confirm-restore").click();
    await expect(page.getByRole("region").getByText("Restored the backup")).toBeVisible();

    // Navigate back to parameters and confirm live value returned to 2280 mm
    await page.getByTestId("nav-group-geometry").click();
    await expect(page.getByTestId("param-row-tire-circumference")).toContainText("Live: 2280 mm");
  });

  test("reloads and reconnects cleanly to persisted storage", async ({ page }) => {
    await openInstrument(page);
    await page.getByTestId("btn-connect-sim").click();
    await expect(page.getByTestId("identity-strip")).toContainText("EP800-CRG");

    // Reload and reconnect to persisted storage
    await page.reload();
    await openInstrument(page);
    await page.getByTestId("btn-connect-sim").click();
    await expect(page.getByTestId("identity-strip")).toContainText("EP800-CRG");
  });

  test("a dismissed link-lost failure keeps a reconnect path and blocks saving", async ({ page }) => {
    await openInstrument(page);
    await page.getByTestId("btn-connect-sim").click();
    await expect(page.getByTestId("identity-strip")).toContainText("EP800-CRG");
    await page.getByTestId("stepper-plus-tire-circumference").click();
    // The link-loss detection itself is covered by the Bluetooth tests.
    await page.evaluate(() => (window as any).cranklinkInstrumentController.update({
      linkLost: true,
      failure: { kind: "save", error: "Bluetooth disconnected" },
    }));
    await page.getByRole("button", { name: "Dismiss error" }).click();
    await expect(page.getByTestId("error-banner")).toHaveCount(0);

    await page.getByTestId("btn-review-save").click();
    await expect(page.getByTestId("review-link-lost")).toBeVisible();
    // Only the lost link blocks saving.
    await expect(page.getByTestId("btn-commit-batch")).toBeDisabled();
    await page.getByRole("button", { name: "Keep editing" }).click();

    await page.getByTestId("btn-link-reconnect").click();
    await expect(page.getByTestId("link-lost-banner")).toHaveCount(0);
    await expect(page.getByTestId("link-status")).toHaveText("Simulator");
    await expect(page.getByTestId("tx-item-tire-circumference")).toContainText("2281 mm");
  });

  test("a possibly interrupted flash offers only the CLI recovery steps", async ({ page }) => {
    await openInstrument(page);
    await page.getByTestId("btn-connect-sim").click();
    await expect(page.getByTestId("identity-strip")).toContainText("EP800-CRG");
    const show = (error: string) => page.evaluate((error) =>
      (window as any).cranklinkInstrumentController.update({ failure: { kind: "firmware", error } }), error);
    const outcome = page.getByTestId("error-outcome");

    // Before any write, a closed link offers Reconnect and another failure a read.
    await show("error: Bluetooth disconnected");
    await expect(page.getByTestId("btn-error-reconnect")).toBeVisible();
    await show("error: verify mismatch");
    await expect(page.getByTestId("btn-error-read")).toBeVisible();

    const rule = "=".repeat(80);
    for (const error of [
      // A Bluetooth write that started, even without the CLI's warning.
      "Programming 8192/65536 bytes (12%)\nerror: Bluetooth disconnected\n[exit 1]",
      `Programming 8192/65536 bytes (12%)\n${rule}\nCRITICAL: FLASH WRITE INTERRUPTED. Motor may be in an unbootable recovery state.\n`
        + "DO NOT POWER CYCLE THE MOTOR.\nTo recover, re-run with:\n  cranklink --port webusb -s 010203040506 firmware flash dcas D.dat "
        + `--companion M.dat --recovery --write\n${rule}\nerror: USB device disconnected`,
    ]) {
      await show(error);
      await expect(outcome).toContainText("Do not power-cycle it");
      await expect(outcome).toContainText("cranklink CLI");
      await expect(page.getByTestId("error-recovery-steps")).toHaveAttribute("href", /#recover-a-motor-stuck-in-bootloader-mode$/);
      await expect(page.getByTestId("btn-error-reconnect")).toHaveCount(0);
      await expect(page.getByTestId("btn-error-read")).toHaveCount(0);
    }
  });

  test("restore waits until staged edits are saved or discarded", async ({ page }) => {
    await openInstrument(page);
    await page.getByTestId("btn-connect-sim").click();
    await expect(page.getByTestId("identity-strip")).toContainText("EP800-CRG");
    await page.getByTestId("stepper-plus-tire-circumference").click();
    await page.getByTestId("btn-review-save").click();
    await page.getByTestId("btn-commit-batch").click();
    await expect(page.getByTestId("transaction-rail")).toBeHidden();

    await page.getByTestId("stepper-plus-tire-circumference").click();
    await page.getByTestId("nav-section-journal").click();
    const restore = page.locator('[data-action="restore-snapshot"]').first();
    await expect(restore).toBeDisabled();
    await expect(page.getByTestId("restore-staged-hint")).toBeVisible();
    await page.getByTestId("btn-discard-staged").click();
    await expect(restore).toBeEnabled();
    await expect(page.getByTestId("restore-staged-hint")).toHaveCount(0);
  });

  test("region and speed controls show what the core enforces", async ({ page }) => {
    await openInstrument(page);
    await page.getByTestId("btn-connect-sim").click();
    await expect(page.getByTestId("identity-strip")).toContainText("EP800-CRG");
    // EP800 destinations are 0, 1, 3 and 4, labelled by market and speed range.
    await page.getByTestId("param-row-destination").getByRole("button", { name: /Market Region/ }).click();
    await expect(page.getByRole("option")).toHaveText(["Europe · 15–25 km/h", "United States · 19–32 km/h", "Region 3 · 15–25 km/h", "Region 4 · 15–25 km/h"]);
    await page.keyboard.press("Escape");

    // Another tool left the speed above the 15-32 km/h cranklink writes.
    await page.evaluate(() => {
      const controller = (window as any).cranklinkInstrumentController;
      const { liveValues } = controller.getState();
      controller.update({ liveValues: { ...liveValues, "speed-limit": { value: 35, raw: "" } } });
    });
    const speed = page.getByTestId("param-row-speed-limit");
    await expect(speed).toContainText("Live: 35 km/h");
    await expect(speed.getByTestId("slider-outside-speed-limit")).toHaveText(
      "The current value, 35 km/h, is outside the range cranklink can write (15 km/h to 32 km/h), so the slider stops at 32 km/h.");

    // The same warning must use physical units for scaled readings and retain
    // both the normal setting help and the warning as accessible descriptions.
    await page.evaluate(() => {
      const controller = (window as any).cranklinkInstrumentController;
      const { liveValues } = controller.getState();
      controller.update({ liveValues: { ...liveValues, "display-speed-adjustment": { value: 75, raw: "" } } });
    });
    const calibration = page.getByTestId("param-row-display-speed-adjustment");
    await expect(calibration).toContainText("Live: 7.5%");
    await expect(calibration.getByTestId("slider-outside-display-speed-adjustment")).toHaveText(
      "The current value, 7.5%, is outside the range cranklink can write (-5% to 5%), so the slider stops at 5%.");
    await expect(calibration.getByRole("slider")).toHaveAccessibleDescription(/Percentage correction[\s\S]*The current value, 7\.5%/);
  });

  test("the review does not promise to restore a value cranklink cannot write", async ({ page }) => {
    await openInstrument(page);
    await page.getByTestId("btn-connect-sim").click();
    await expect(page.getByTestId("identity-strip")).toContainText("EP800-CRG");
    // Another tool left the speed above the 15-32 km/h cranklink writes.
    await page.evaluate(() => {
      const controller = (window as any).cranklinkInstrumentController;
      const { liveValues } = controller.getState();
      controller.update({ liveValues: { ...liveValues, destination: { value: 1, raw: "" }, "speed-limit": { value: 35, raw: "" } } });
    });
    const warning = "cranklink cannot write back the current Assist Speed Limit (35 km/h), so Backup & Restore cannot undo that change.";

    const rpc = page.evaluate(() => (window as any).webMcpServer.handleRequest("tools/call", {
      name: "cranklink_set_setting",
      arguments: { name: "speed-limit", value: 30 },
    }).catch((error: Error) => error.message));
    await expect(page.getByTestId("mcp-approval-unwritable")).toHaveText(warning);
    await page.getByTestId("btn-mcp-reject").click();
    expect(await rpc).toBe("Transaction rejected by operator.");

    await page.evaluate(() => (window as any).cranklinkInstrumentController.stageParameter("speed-limit", 30));
    await page.getByTestId("btn-review-save").click();
    const review = page.getByTestId("review-dialog");
    await expect(review.getByTestId("review-unwritable")).toHaveText(warning);
    await expect(review).not.toContainText("so you can restore it");
  });

  test("edits physical watts and percentages while preserving canonical writes and backup values", async ({ page }) => {
    await openInstrument(page);
    await page.getByTestId("btn-connect-sim").click();
    await expect(page.getByTestId("identity-strip")).toContainText("EP800-CRG");

    const calibration = page.getByTestId("param-row-display-speed-adjustment");
    const slider = calibration.getByRole("slider");
    await expect(slider).toHaveAttribute("min", "-5");
    await expect(slider).toHaveAttribute("max", "5");
    await expect(slider).toHaveAttribute("step", "0.5");
    await slider.focus();
    await slider.press("ArrowRight");
    await expect(calibration).toContainText("Staged: 0.5%");

    await page.getByTestId("nav-group-all").click();
    const light = page.getByTestId("setting-item-light-output");
    const watts = page.getByTestId("setting-input-light-output").locator("input");
    await expect(light).toContainText("Live: 2 W");
    await expect(watts).toHaveValue("2");
    await watts.fill("13.75");
    await watts.press("Tab");
    await expect(light).toContainText("Staged: 13.75 W");
    await expect.poll(() => page.evaluate(() => (window as any).cranklinkInstrumentController.getState().stagedValues)).toEqual({
      "display-speed-adjustment": 5,
      "light-output": 1375,
    });
    await expect(watts).toHaveAccessibleName("Light Port Power Allocation (W)");
    await expect(watts).toHaveAccessibleDescription(/Power budget reserved for a headlight and taillight/);

    await page.getByTestId("btn-review-save").click();
    const review = page.getByTestId("review-dialog");
    await expect(review.getByTestId("review-item-light-output")).toContainText("13.75 W");
    await expect(review.getByTestId("review-item-display-speed-adjustment")).toContainText("0.5%");
    await page.getByTestId("btn-commit-batch").click();
    await expect(page.getByTestId("transaction-rail")).toBeHidden();
    await expect(light).toContainText("Live: 13.75 W");
    await expect.poll(() => page.evaluate(() => (window as any).cranklinkInstrumentController.getState().liveValues["light-output"].value)).toBe(1375);

    await page.getByTestId("nav-section-journal").click();
    await expect(page.getByTestId("restore-point").first()).toContainText("Light Port Power Allocation: 2 W");
    const stored = await page.evaluate(() => {
      const controller = (window as any).cranklinkInstrumentController;
      const backup = controller.getState().snapshots.find((file: any) => file.setting === "light-output");
      return { before: backup.before, summary: backup.summary };
    });
    expect(stored).toEqual({ before: 200, summary: "2 W before the change" });
  });

  test("modern DU-EP600 / DU-EP801 session authentication and multi-setting save", async ({ page }) => {
    await openInstrument(page);

    // Select DU-EP801 flagship simulator (requires --ble-unit-auth and serial binding)
    await page.getByTestId("sim-model-select").getByRole("button").click();
    await page.getByRole("option", { name: "DU-EP801" }).click();
    await page.getByTestId("btn-connect-sim").click();

    await expect(page.getByTestId("identity-strip")).toContainText("EP801");
    await expect(page.getByTestId("identity-strip")).toContainText("SN: 010203040506");
    await expect(page.getByTestId("identity-strip")).toContainText("DCAS: 4.6.0.1");

    // Stage three profile torques and save them in one authenticated batch
    await page.evaluate(() => {
      const controller = (window as any).cranklinkInstrumentController;
      // EP801 accepts only its basic-mode torque levels.
      controller.stageParameter("profile1.boost.torque", 78);
      controller.stageParameter("profile1.trail.torque", 70);
      controller.stageParameter("profile1.eco.torque", 42);
    });
    await expect(page.getByTestId("transaction-rail")).toContainText("3 changes to save");

    await page.getByTestId("btn-review-save").click();
    await page.getByTestId("btn-commit-batch").click();
    await expect(page.getByTestId("transaction-rail")).toBeHidden();
    await expect(page.getByRole("region").getByText("Saved 3 settings")).toBeVisible();
  });

  test("synchronized Dual-MCU firmware workflow (DCAS + RX pair by default)", async ({ page }) => {
    await openInstrument(page);
    await page.getByTestId("btn-connect-sim").click();
    await expect(page.getByTestId("identity-strip")).toContainText("EP800-CRG");

    // Navigate to Synchronized Dual-MCU Firmware section
    await page.getByTestId("nav-section-firmware").click();
    await expect(page.getByTestId("firmware-section")).toBeVisible();
    await expect(page.getByTestId("companion-status")).toContainText(
      "Select a matched DCAS (-D) and RX (-M) firmware pair to begin.",
    );

    // Primary buttons are disabled until a matched companion pair is loaded
    await expect(page.getByTestId("btn-fw-dry-run-pair")).toBeDisabled();
    await expect(page.getByTestId("btn-fw-flash-pair")).toBeDisabled();

    // Verify the single-MCU override is collapsed by default
    const overrideDetails = page.getByTestId("single-mcu-override");
    await expect(overrideDetails.getByRole("button", { name: /program one microcontroller/i })).toHaveAttribute("aria-expanded", "false");

    // Verify firmware risk notice is visible and can open the risk warning modal
    await expect(page.getByTestId("fw-risk-notice")).toBeVisible();
    await page.getByTestId("btn-review-fw-warning").click();
    const modal = page.getByTestId("risk-warning-modal");
    await expect(modal).toBeVisible();
    await expect(modal).toContainText("Tested on EP800-CRG only");
    await expect(modal).toContainText("Limited compatibility checks");
    await page.getByTestId("btn-dismiss-warning").click();
    await expect(modal).toBeHidden();

    // One loaded image enables neither single-MCU program; neither can reach a bootloader.
    await page.evaluate(() => {
      const controller = (window as any).cranklinkInstrumentController;
      const dcasFile = { filename: "DUEP800-D.4.2.0.dat", sizeBytes: 1, sha256: "00".repeat(32), role: "dcas", metadata: null };
      controller.update({ firmware: { ...controller.getState().firmware, dcasFile } });
    });
    await overrideDetails.getByRole("button", { name: /program one microcontroller/i }).click();
    await expect(overrideDetails).toContainText("Bootloader recovery is not available in the browser");
    await expect(overrideDetails.getByRole("button", { name: "Force program DCAS only" })).toBeDisabled();
    await expect(overrideDetails.getByRole("button", { name: "Force program RX only" })).toBeDisabled();

    // A checked pair enables programming both, but over Bluetooth the CLI
    // refuses one MCU alone, and a closed link refuses every write.
    await page.evaluate(() => {
      const controller = (window as any).cranklinkInstrumentController;
      const state = controller.getState();
      const rxFile = { filename: "DUEP800-M.4.1.11.dat", sizeBytes: 1, sha256: "00".repeat(32), role: "rx", metadata: null };
      controller.update({
        firmware: { ...state.firmware, rxFile, companionValid: true, dryRunPassed: true, dryRunSerial: state.identity.serial },
      });
    });
    await expect(page.getByTestId("btn-fw-flash-pair")).toBeEnabled();
    await expect(overrideDetails).toContainText("needs a wired SM-PCE02 connection");
    await expect(overrideDetails.getByRole("button", { name: "Force program DCAS only" })).toBeDisabled();
    await expect(overrideDetails.getByRole("button", { name: "Force program RX only" })).toBeDisabled();
    await page.evaluate(() => (window as any).cranklinkInstrumentController.update({ linkLost: true }));
    await expect(page.getByTestId("btn-fw-flash-pair")).toBeDisabled();
    await expect(page.getByTestId("btn-fw-dry-run-pair")).toBeDisabled();
  });

  test("the non-erasing check reads as a check and leaves its result on the page", async ({ page }) => {
    await openInstrument(page);
    await page.getByTestId("btn-connect-sim").click();
    await expect(page.getByTestId("identity-strip")).toContainText("EP800-CRG");
    await page.getByTestId("nav-section-firmware").click();
    // No firmware is bundled, so the firmware commands are held open here; the
    // session is shown as Bluetooth because simulated sessions give no guidance.
    await page.evaluate(() => {
      const controller = (window as any).cranklinkInstrumentController;
      const image = (role: string, filename: string, version: string) => ({
        filename, sizeBytes: 1, sha256: "00".repeat(32), role,
        metadata: { kind: role === "dcas" ? "dcas-x" : "renesas", series: 0x23, unit: 0, version, source_sha256: "" },
      });
      controller.update({
        transport: "ble",
        firmware: {
          ...controller.getState().firmware, companionValid: true,
          dcasFile: image("dcas", "DUEP800-D.4.2.0.dat", "4.2.0.0"), rxFile: image("rx", "DUEP800-M.4.1.11.dat", "4.1.11.0"),
        },
      });
      const terminal = controller.terminal;
      const run = terminal.run;
      terminal.run = (line: string) => line.includes("firmware flash")
        ? new Promise((resolve) => { (window as any).finishFirmware = () => resolve({ code: 0 }); })
        : run(line);
    });

    const modal = page.getByTestId("operation-modal");
    await page.getByTestId("btn-fw-dry-run-pair").click();
    await expect(modal).toContainText("Checking firmware compatibility…");
    await expect(modal).toContainText("Nothing is written, but the motor enters its bootloader for this check.");
    await expect(modal).not.toContainText("Firmware is being written");
    await page.evaluate(() => (window as any).finishFirmware());
    await expect(modal).toBeHidden();
    const result = page.getByTestId("fw-check-result");
    await expect(result).toContainText("Non-erasing check passed");
    await expect(result).toContainText("Drive unit 010203040506 (DCAS 4.2.0.0, RX 4.1.11.0) with images DCAS 4.2.0.0 and RX 4.1.11.0.");

    // Programming keeps the write dialog.
    await page.getByTestId("btn-fw-flash-pair").click();
    await page.getByTestId("btn-confirm-program").click();
    await expect(modal).toContainText("Updating firmware…");
    await expect(modal).toContainText("Firmware is being written.");
    await page.evaluate(() => (window as any).finishFirmware());
    await expect(modal).toBeHidden();
    await expect(result).toHaveCount(0);
  });

  test("WebMCP external agent authorization flow", async ({ page }) => {
    await openInstrument(page);
    await page.getByTestId("btn-connect-sim").click();
    await expect(page.getByTestId("identity-strip")).toContainText("EP800-CRG");

    await page.getByTestId("nav-section-diagnostics").click();

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
    await page.getByTestId("nav-group-geometry").click();
    await expect(page.getByTestId("param-row-tire-circumference")).toContainText("Live: 2150 mm");
  });

  test("a regulated change is marked on every save path", async ({ page }) => {
    await openInstrument(page);
    await page.getByTestId("btn-connect-sim").click();
    await expect(page.getByTestId("identity-strip")).toContainText("EP800-CRG");

    // An agent's region change is marked as regulated. Region 0
    // keeps the simulator's 25 km/h and has no power cap, so the core accepts it.
    const rpcPromise = page.evaluate(() => (window as any).webMcpServer.handleRequest("tools/call", {
      name: "cranklink_set_setting",
      arguments: { name: "destination", value: 0 },
    }));
    const approvalBanner = page.getByTestId("mcp-approval-banner");
    await expect(approvalBanner.getByLabel("Regulated setting")).toBeVisible();
    await page.getByTestId("btn-mcp-approve").click();
    expect(((await rpcPromise) as any).completed).toBe(1);

    // Save & disconnect opens the review rather than writing straight away.
    await page.evaluate(() => (window as any).cranklinkInstrumentController.stageParameter("destination", 1));
    await page.getByTestId("btn-disconnect").click();
    await page.getByTestId("btn-save-disconnect").click();
    const review = page.getByTestId("review-dialog");
    await expect(review.getByTestId("review-item-destination").getByLabel("Regulated setting")).toBeVisible();
    await review.getByTestId("btn-commit-batch").click();
    await expect(page.getByTestId("btn-connect-sim")).toBeVisible();
  });
});

// Only the browser chooser is replaced. The selected device is the production
// GATT simulator; WebBleGatt, Rust authentication, probing and settings run intact.
async function selectSimulatedBluetoothDevice(page: Page, rejectAuthentication = false, cachePasskey = true, deferAuthenticationFailure = false, di2Bike = false, masterInfo?: number[], stepsWithComponents = false) {
  // Headless Chromium on Linux (CI) has no Web Bluetooth, and the connect view
  // decides on first render whether to offer it. Stand in before the app loads;
  // the simulator's chooser replaces this below.
  await page.addInitScript(() => {
    if (!("bluetooth" in navigator)) {
      Object.defineProperty(navigator, "bluetooth", {
        configurable: true,
        value: { requestDevice: async () => { throw new Error("No simulated Bluetooth device yet"); } },
      });
    }
  });
  if (cachePasskey) {
    // The cached PIN applies only to the gateway it was entered for.
    await page.addInitScript((device) => {
      sessionStorage.setItem("esteps_ble_passkey", "000000");
      sessionStorage.setItem("esteps_ble_passkey_device", device);
    }, `simulated-etube-${rejectAuthentication ? "ep801" : "ep800-crg"}`);
  }
  await openInstrument(page);
  const setup = await page.evaluate(async ({ reject, defer, di2, masterInfo, stepsWithComponents }) => {
    const simulatorModule = "/simulator.js";
    const { DeviceSimulator } = await import(simulatorModule);
    const device = new DeviceSimulator({
      model: reject ? "ep801" : "ep800-crg",
      fault: reject && !defer ? "auth_reject" : "none",
      components: di2 || stepsWithComponents,
      bikeSystem: di2 ? "di2" : "steps",
    });
    if (masterInfo) device.masterInfo = masterInfo;
    const connect = device.gatt.connect;
    let radioShutdown = false;
    device.gatt.connect = async () => {
      if (radioShutdown) throw new Error("Gateway connection mode ended");
      return connect();
    };
    const write = device.write.bind(device);
    device.write = (id: number, bytes: number[]) => {
      const result = write(id, bytes);
      if (id === 0x2afa && bytes[1] === 0x0c && bytes[2] === 0) radioShutdown = true;
      return result;
    };
    (window as any).testBluetoothDevice = device;
    let chooserCalls = 0;
    Object.defineProperty(navigator, "bluetooth", {
      configurable: true,
      value: {
        requestDevice: async () => {
          chooserCalls++;
          return device;
        },
      },
    });
    (window as any).testBluetoothChooserCalls = () => chooserCalls;
    return { hasSimulator: Boolean(device) };
  }, { reject: rejectAuthentication, defer: deferAuthenticationFailure, di2: di2Bike, masterInfo, stepsWithComponents });
  expect(setup).toEqual({ hasSimulator: true });
  await expect(page.getByTestId("btn-connect-ble")).toBeEnabled();
  await page.getByTestId("btn-connect-ble").click();
}

test("Bluetooth passkey prompt takes keyboard input while connecting", async ({ page }) => {
  test.setTimeout(60_000);
  await selectSimulatedBluetoothDevice(page, false, false);
  const passkeyDialog = page.getByRole("dialog", { name: "Bluetooth passkey", exact: true });
  await expect(passkeyDialog).toBeVisible();
  // The busy overlay traps focus, so it must yield while the operator types.
  await expect(page.getByTestId("operation-modal")).toHaveCount(0);
  const field = passkeyDialog.getByLabel("Passkey", { exact: true });
  await expect(field).toBeFocused();
  await page.keyboard.type("00a0000");
  await expect(field).toHaveValue("000000");
  await page.keyboard.press("Enter");
  await expect(passkeyDialog).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => (window as any).cranklinkInstrumentController.getState().statusMessage))
    .toContain("74 parameters read");
});

test("Bluetooth connect reads every setting and keeps routine notices out of errors", async ({ page }) => {
  test.setTimeout(60_000);
  await selectSimulatedBluetoothDevice(page);
  await expect.poll(() => page.evaluate(() => (window as any).cranklinkInstrumentController.getState().statusMessage))
    .toContain("74 parameters read");
  await expect(page.getByTestId("error-banner")).toHaveCount(0);
  const state = await page.evaluate(() => {
    const state = (window as any).cranklinkInstrumentController.getState();
    return {
      phase: state.phase,
      transport: state.transport,
      failure: state.failure,
      settings: Object.keys(state.liveValues),
      logs: state.logs as { stream: string; text: string }[],
      chooserCalls: (window as any).testBluetoothChooserCalls(),
    };
  });
  expect(state.phase).toBe("ready");
  expect(state.transport).toBe("ble");
  expect(state.failure).toBeNull();
  expect(state.settings).toHaveLength(74);
  expect(state.chooserCalls).toBe(1);
  const commands = state.logs.filter((entry) => entry.stream === "cmd").map((entry) => entry.text);
  expect(commands).toEqual([
    "cranklink> connect ble",
    "cranklink> ble detect-system",
    "cranklink> --ble-master=master units",
    "cranklink> --ble-master=master --slot 13 --component 'DU-EP800-CRG' settings read",
  ]);
  expect(state.logs.filter((entry) => entry.stream === "stderr")).toEqual([]);
  expect(state.logs.some((entry) => entry.stream === "notice" && /Bluetooth/i.test(entry.text))).toBe(true);
  expect(state.logs.map((entry) => entry.text).join("\n")).not.toMatch(/firmware operations disabled/i);
  await expect(page.getByTestId("bluetooth-routing-status")).toContainText("STEPS detected");
  expect(await page.evaluate(() => {
    const writes = (window as any).testBluetoothDevice.writes;
    const detect = writes.findIndex((w: any) => w.id === 0x2afa && w.bytes[1] === 0x0d);
    const master = writes.findIndex((w: any) => w.id === 0x2afa && w.bytes[1] === 3 && w.bytes[2] === 0x20);
    const start = writes.findIndex((w: any) => w.id === 0x2afa && w.bytes[1] === 0x0c && w.bytes[2] === 1);
    return detect >= 0 && master > detect && start > master;
  })).toBe(true);
  await expect(page.getByTestId("param-row-tire-circumference")).toBeVisible();
  await page.getByTestId("nav-section-diagnostics").click();
  await expect(page.getByTestId("console-body")).toBeVisible();
  await expect(page.locator(".console-log-line.stderr")).toHaveCount(0);
  await expect(page.locator(".console-log-line.notice").first()).toBeVisible();
  const saved = await page.evaluate(async () => {
    const controller = (window as any).cranklinkInstrumentController;
    const motorStarts = () => (window as any).testBluetoothDevice.writes.filter((w: any) => w.id === 0x2afe && w.bytes[1] === 0x32 && w.bytes[2] === 0x10 && w.bytes[3] === 1).length;
    const initialMotorStarts = motorStarts();
    controller.stageParameter("tire-circumference", 2150);
    await controller.commitStagedBatch();
    const firstMotorStarts = motorStarts() - initialMotorStarts;
    controller.stageParameter("tire-circumference", 2151);
    await controller.commitStagedBatch();
    await controller.readAllParameters();
    const device = (window as any).testBluetoothDevice;
    return {
      wheel: controller.getState().liveValues["tire-circumference"].value,
      firstMotorStarts,
      connections: device.connections,
      connected: device.gatt.connected,
      stops: device.writes.filter((w: any) => w.id === 0x2afa && w.bytes[1] === 0x0c && w.bytes[2] === 0).length,
    };
  });
  expect(saved).toEqual({ wheel: 2151, firstMotorStarts: 1, connections: 1, connected: true, stops: 0 });
  const unloadIsGuarded = () => page.evaluate(() => {
    const event = new Event("beforeunload", { cancelable: true });
    window.dispatchEvent(event);
    return event.defaultPrevented;
  });
  expect(await unloadIsGuarded(), "An idle retained Bluetooth session requires explicit disconnect before leaving").toBe(true);
  await page.evaluate(() => (window as any).cranklinkInstrumentController.disconnect());
  expect(await page.evaluate(() => (window as any).testBluetoothDevice.gatt.connected)).toBe(false);
  expect(await unloadIsGuarded(), "Confirmed disconnect releases the unload guard").toBe(false);
});

test("Di2 Bluetooth retains ordinary service mode across reads and closes without stop or reset", async ({ page }, testInfo) => {
  await selectSimulatedBluetoothDevice(page, false, true, false, true);
  await expect.poll(() => page.evaluate(() => (window as any).cranklinkInstrumentController.getState().phase)).toBe("ready");
  await expect(page.getByTestId("bluetooth-routing-status")).toContainText("Di2 shifting only detected");
  await page.evaluate(async () => {
    const device = (window as any).testBluetoothDevice;
    // A fixed-width notification can carry one byte beyond the six-byte unit serial.
    device.components.get(5).serial = [...device.components.get(5).serial.slice(0, 6), 0x70];
    await (window as any).cranklinkInstrumentController.selectComponent(5);
    await (window as any).cranklinkInstrumentController.readAllParameters();
  });
  const retained = await page.evaluate(() => {
    const device = (window as any).testBluetoothDevice;
    const writes = device.writes;
    return {
      connected: device.gatt.connected,
      connections: device.connections,
      model: (window as any).cranklinkInstrumentController.getState().identity.model,
      masterModes: writes.filter((w: any) => w.id === 0x2afa && w.bytes[1] === 3 && w.bytes[2] === 0x20).length,
      stops: writes.filter((w: any) => (w.id === 0x2afa && [0x10, 0x0c].includes(w.bytes[1]) && w.bytes[2] !== 1) || (w.id === 0x2afe && w.bytes[1] === 0x32 && w.bytes[2] === 0x10 && w.bytes[3] === 0)).length,
      componentStarts: writes.filter((w: any) => w.id === 0x2afe && w.bytes[0] === 5 && w.bytes[1] === 0x32 && w.bytes[2] === 0x10 && w.bytes[3] === 1).length,
    };
  });
  expect(retained).toEqual({ connected: true, connections: 1, model: "RD-R9150", masterModes: 0, stops: 0, componentStarts: 1 });
  expect(await page.evaluate(() => {
    const writes = (window as any).testBluetoothDevice.writes;
    const detect = writes.findIndex((w: any) => w.id === 0x2afa && w.bytes[1] === 0x0d);
    const start = writes.findIndex((w: any) => w.id === 0x2afa && w.bytes[1] === 0x0c && w.bytes[2] === 1);
    return detect >= 0 && start > detect;
  })).toBe(true);
  // Disconnect stays directly reachable on a phone, without opening the menu.
  await page.setViewportSize({ width: 393, height: 851 });
  const disconnect = page.getByTestId("btn-disconnect");
  await expect(disconnect).toBeInViewport();
  await page.screenshot({ path: testInfo.outputPath("phone-disconnect.png") });
  await page.evaluate(() => {
    const terminal = (window as any).estepsTerminal;
    const run = terminal.run.bind(terminal);
    const gate = new Promise<void>((resolve) => { (window as any).finishTestDisconnect = resolve; });
    terminal.run = async (line: string) => {
      if (line === "disconnect") await gate;
      return run(line);
    };
  });
  await disconnect.click();
  await expect(page.getByTestId("operation-modal")).toContainText("Disconnecting");
  await expect(page.getByTestId("operation-modal")).toBeInViewport();
  await expect(page.getByTestId("btn-connect-ble")).toBeHidden();
  await page.screenshot({ path: testInfo.outputPath("phone-disconnecting.png"), animations: "disabled" });
  await page.evaluate(() => (window as any).finishTestDisconnect());
  const recovery = page.getByTestId("bluetooth-disconnected-view");
  await expect(recovery).toBeVisible();
  await expect(recovery).toContainText("Wait for the bike to return to riding mode");
  await expect(recovery).toContainText("cannot detect when the bike is ready");
  await expect(page.getByTestId("operation-modal")).toBeHidden();
  await expect(page.getByTestId("btn-connect-ble")).toBeHidden();
  await page.screenshot({ path: testInfo.outputPath("phone-disconnected.png") });
  expect(await page.evaluate(() => ({
    connected: (window as any).testBluetoothDevice.gatt.connected,
    stopWrites: (window as any).testBluetoothDevice.writes.filter((w: any) =>
      (w.id === 0x2afa && ((w.bytes[1] === 0x0c && w.bytes[2] === 0) || w.bytes[1] === 0x10)) ||
      (w.id === 0x2afe && w.bytes[1] === 0x32 && w.bytes[2] === 0x10 && w.bytes[3] === 0)).length,
  }))).toEqual({ connected: false, stopWrites: 0 });
  await page.getByTestId("btn-back-to-connect").click();
  await expect(page.getByTestId("btn-connect-ble")).toBeVisible();
});

test("STEPS with Di2 components still detects the motor system", async ({ page }) => {
  await selectSimulatedBluetoothDevice(page, false, true, false, false, undefined, true);
  await expect.poll(() => page.evaluate(() => (window as any).cranklinkInstrumentController.getState().phase)).toBe("ready");
  await expect(page.getByTestId("bluetooth-routing-status")).toContainText("STEPS detected");
  await expect(page.getByTestId("identity-strip")).toContainText("EP800-CRG");
  expect(await page.evaluate(() => ({ connected: (window as any).testBluetoothDevice.connections,
    components: (window as any).cranklinkInstrumentController.getState().units.length })))
    .toEqual({ connected: 1, components: 7 });
});

test("unknown master asks for routing before service and keeps the original connection", async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 320, height: 851 });
  await selectSimulatedBluetoothDevice(page, false, true, false, false, [1, 0x08, 1]);
  const choice = page.getByTestId("bluetooth-system-dialog");
  await expect(choice).toBeVisible();
  await expect(choice).toBeInViewport();
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(320);
  await page.screenshot({ path: testInfo.outputPath("phone-system-choice.png"), animations: "disabled" });
  await expect(page.getByTestId("operation-modal")).toBeHidden();
  expect(await page.evaluate(() => {
    const device = (window as any).testBluetoothDevice;
    return { connections: device.connections, serviceStarted: device.writes.some((w: any) =>
      w.id === 0x2afa && ((w.bytes[1] === 3 && w.bytes[2] === 0x20) || (w.bytes[1] === 0x0c && w.bytes[2] === 1))) };
  })).toEqual({ connections: 1, serviceStarted: false });
  await choice.getByRole("button", { name: "STEPS e-bike", exact: true }).click();
  await expect(page.getByTestId("bluetooth-routing-status")).toContainText("STEPS selected manually");
  await expect(page.getByTestId("param-row-tire-circumference")).toBeVisible();
  expect(await page.evaluate(() => (window as any).testBluetoothDevice.connections)).toBe(1);
});

test("cancelling unknown detection closes Bluetooth without entering service", async ({ page }) => {
  await selectSimulatedBluetoothDevice(page, false, true, false, false, [1, 0xaa, 1]);
  await expect(page.getByTestId("bluetooth-system-dialog")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("bluetooth-system-dialog")).toBeHidden();
  await expect(page.getByTestId("error-banner")).toContainText("cancelled before entering service mode");
  expect(await page.evaluate(() => {
    const device = (window as any).testBluetoothDevice;
    return { connected: device.gatt.connected, pcLink: device.writes.some((w: any) =>
      w.id === 0x2afa && w.bytes[1] === 0x0c) };
  })).toEqual({ connected: false, pcLink: false });
});

test("invalid master metadata closes Bluetooth before service without a manual-choice prompt", async ({ page }) => {
  await selectSimulatedBluetoothDevice(page, false, true, false, false, [0, 0x23, 1]);
  await expect(page.getByTestId("error-banner")).toContainText("invalid BLE master metadata");
  await expect(page.getByTestId("bluetooth-system-dialog")).toBeHidden();
  expect(await page.evaluate(() => {
    const device = (window as any).testBluetoothDevice;
    return { connected: device.gatt.connected, serviceStarted: device.writes.some((w: any) =>
      w.id === 0x2afa && w.bytes[1] === 0x0c) };
  })).toEqual({ connected: false, serviceStarted: false });
});

test("Bluetooth drive-unit authentication rejection closes the session and retains staged changes", async ({ page }) => {
  await selectSimulatedBluetoothDevice(page, true, true, true);
  await expect(page.getByTestId("identity-strip")).toContainText("DU-EP801");
  await expect.poll(() => page.evaluate(() => (window as any).cranklinkInstrumentController.getState().phase)).toBe("ready");
  await page.evaluate(async () => {
    const controller = (window as any).cranklinkInstrumentController;
    (window as any).testBluetoothDevice.setFault("auth_reject");
    controller.stageParameter("walk-assist", 0);
    await controller.commitStagedBatch().catch(() => {});
  });
  await expect(page.getByTestId("error-banner")).toBeVisible();
  await expect(page.getByTestId("error-banner")).toContainText("closed before the bike confirmed");
  await expect(page.getByTestId("link-status")).toHaveText("Link closed");
  const state = await page.evaluate(() => {
    const state = (window as any).cranklinkInstrumentController.getState();
    return {
      phase: state.phase,
      settings: Object.keys(state.liveValues),
      logs: state.logs as { stream: string; text: string }[],
      chooserCalls: (window as any).testBluetoothChooserCalls(),
    };
  });
  expect(state.phase).toBe("ready");
  expect(state.settings.length).toBeGreaterThan(0);
  expect(state.chooserCalls).toBe(1);
  const commands = state.logs.filter((entry) => entry.stream === "cmd").map((entry) => entry.text);
  expect(commands).toHaveLength(5);
  expect(commands.slice(0, 3)).toEqual(["cranklink> connect ble", "cranklink> ble detect-system", "cranklink> --ble-master=master units"]);
  expect(commands[3]).toContain("--component 'DU-EP801'");
  expect(commands[3]).toContain("settings read");
  expect(commands[4]).toContain("settings batch");
  expect(await page.evaluate(() => (window as any).cranklinkInstrumentController.getState().stagedValues)).toEqual({ "walk-assist": 0 });
  expect(state.logs.some((entry) => entry.stream === "stderr" && /error:.*(?:auth|reject|status|challenge)/is.test(entry.text)))
    .toBe(true);
  const authLogs = await page.evaluate(() => (window as any).cranklinkInstrumentController.getState().logs);
  expect(authLogs.some((entry: { stream: string }) => entry.stream === "stderr")).toBe(true);
  expect(await page.evaluate(() => (window as any).testBluetoothDevice.gatt.connected)).toBe(false);
});

// The old port is already closed and unlocked when a switch fails, so it must
// not stay selected for the next command.
test("a failed switch to Bluetooth leaves no transport selected", async ({ page }) => {
  await openInstrument(page);
  const state = await page.evaluate(async () => {
    const terminal = (window as any).estepsTerminal;
    await terminal.run("connect sim");
    const before = terminal.status().transport;
    const simulatorModule = "/simulator.js";
    const { DeviceSimulator } = await import(simulatorModule);
    const device = new DeviceSimulator({ model: "ep800-crg" });
    let listenerRemoved = false;
    const remove = device.removeEventListener.bind(device);
    device.removeEventListener = (type: string, listener: EventListener) => {
      if (type === "gattserverdisconnected") listenerRemoved = true;
      remove(type, listener);
    };
    Object.defineProperty(navigator, "bluetooth", {
      configurable: true,
      value: { requestDevice: async () => device },
    });
    // Another tab holds the Bluetooth session.
    await new Promise<void>((held) => {
      navigator.locks.request("cranklink-ble-session", () => {
        held();
        return new Promise(() => {});
      });
    });
    const error = await terminal.run("connect ble").then(() => null, (e: Error) => e.message);
    return { before, error, listenerRemoved, ...terminal.status() };
  });
  expect(state.before).toBe("sim");
  expect(state.error).toContain("Another tab has an active Bluetooth session");
  expect(state.listenerRemoved).toBe(true);
  expect(state.transport).toBeNull();
  expect(state.usbSelected).toBe(false);
});

// The simulator flag makes host_sleep skip the Rust pacing sleeps, so it must
// never outlive the simulator on a real adapter.
test("adopting a granted USB adapter after the simulator ends simulated timing", async ({ page }) => {
  await openInstrument(page);
  const state = await page.evaluate(async () => {
    const terminal = (window as any).estepsTerminal;
    await terminal.run("connect sim");
    const simulated = Boolean((window as any).estepsDeviceSimulator);
    Object.defineProperty(navigator, "usb", {
      configurable: true,
      value: { getDevices: async () => [{ vendorId: 0x1e44, productId: 0x7151, configurations: [] }] },
    });
    await terminal.selectGranted("usb");
    return {
      simulated,
      simulator: Boolean((window as any).estepsDeviceSimulator),
      transport: terminal.status().transport,
    };
  });
  expect(state).toEqual({ simulated: true, simulator: false, transport: "usb" });
});

/**
 * Moves the simulated drive unit's session onto a granted USB adapter, then
 * unplugs another SM-PCE02. `window.unplugUsb()` unplugs the selected one;
 * app.js listens on the browser's own navigator.usb. Reopening it finds no
 * adapter, as when the chooser is cancelled after a replug.
 */
async function moveSessionToUsb(page: Page): Promise<boolean> {
  await openInstrument(page);
  await page.getByTestId("btn-connect-sim").click();
  await expect(page.getByTestId("identity-strip")).toContainText("EP800-CRG");
  await page.getByTestId("stepper-plus-tire-circumference").click();
  return page.evaluate(async () => {
    const usb = (navigator as any).usb as EventTarget | undefined;
    if (!usb) return false;
    const adapter = { vendorId: 0x1e44, productId: 0x7151, configurations: [] };
    Object.defineProperty(navigator, "usb", {
      configurable: true,
      value: {
        getDevices: async () => [adapter],
        requestDevice: async () => { throw new DOMException("No device selected.", "NotFoundError"); },
      },
    });
    await (window as any).estepsTerminal.selectGranted("usb");
    (window as any).cranklinkInstrumentController.update({ transport: "usb" });
    const unplug = (device: object) => usb.dispatchEvent(Object.defineProperty(new Event("disconnect"), "device", { value: device }));
    (window as any).unplugUsb = () => unplug(adapter);
    unplug({ vendorId: 0x1e44, productId: 0x7151, configurations: [] });
    return true;
  });
}

// Without this the sidebar stayed Connected until the next operation failed.
test("unplugging the selected USB adapter between commands offers Reconnect at once", async ({ page }) => {
  test.skip(!(await moveSessionToUsb(page)), "This browser exposes no WebUSB");
  await expect(page.getByTestId("link-status")).toHaveText("Connected");
  await expect(page.getByTestId("error-banner")).toHaveCount(0);

  await page.evaluate(() => (window as any).unplugUsb());
  const banner = page.getByTestId("error-banner");
  await expect(banner).toContainText("USB adapter disconnected");
  await expect(banner.getByTestId("btn-error-reconnect")).toBeVisible();
  await expect(page.getByTestId("link-status")).toHaveText("Link closed");
  await expect(page.getByTestId("tx-item-tire-circumference")).toContainText("2281 mm");
});

// After a replug the browser may show its chooser again; cancelling it lost the edits.
test("a cancelled Reconnect keeps staged edits until the same drive unit connects", async ({ page }) => {
  test.skip(!(await moveSessionToUsb(page)), "This browser exposes no WebUSB");
  await page.evaluate(() => (window as any).unplugUsb());
  await page.getByTestId("btn-error-reconnect").click();
  await expect(page.getByTestId("error-banner")).toContainText("Couldn’t connect");
  await expect(page.getByTestId("error-outcome")).toHaveText("Your unsaved changes are kept: connect to DU-EP800-CRG "
    + "(serial 010203040506) again to restore them. Connecting to another component discards them.");
  await page.getByTestId("btn-connect-sim").click();
  await expect(page.getByTestId("identity-strip")).toContainText("SN: 010203040506");
  await expect(page.getByTestId("tx-item-tire-circumference")).toContainText("2281 mm");
});

// Windows exposes the SM-PCE02 as an FTDI COM port; WebUSB needs WinUSB there.
test("Windows offers the SM-PCE02 COM port before WebUSB", async ({ page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(Navigator.prototype, "platform", { configurable: true, get: () => "Win32" });
    Object.defineProperty(Navigator.prototype, "userAgentData", { configurable: true, get: () => undefined });
    Object.defineProperty(Navigator.prototype, "bluetooth", {
      configurable: true,
      get: () => ({ getAvailability: async () => true, requestDevice: async () => { throw new Error("not used"); } }),
    });
  });
  await openInstrument(page);
  await expect(page.getByTestId("btn-connect-ble")).toHaveText("Connect via Bluetooth");
  await page.getByTestId("btn-more-menu").click();
  await expect(page.locator(".menu-dropdown").getByRole("button")).toHaveText([
    "SM-PCE02 (COM port)",
    "USB adapter (SM-PCE02, needs WinUSB)",
  ]);
});
