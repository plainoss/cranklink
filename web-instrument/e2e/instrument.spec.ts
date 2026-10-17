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
  const dismissBtn = page.getByTestId("btn-dismiss-warning");
  if (await dismissBtn.isVisible()) {
    await dismissBtn.click();
  }
  if (page.url().includes("/cranklink/")) {
    await expect.poll(() => page.evaluate(() =>
      navigator.serviceWorker.controller?.scriptURL || "")).toContain("/cranklink/coi-serviceworker.js");
  }
}

test.describe("cranklink Drive Unit Field Instrument (WASM + Wire Simulator)", () => {
  test("zero-click connect, inline staging, atomic batch commit, and snapshot rollback @smoke", async ({
    page,
  }) => {
    await openInstrument(page);

    // The landing screen keeps the next action prominent and omits an idle status badge.
    await expect(page.locator("main").getByRole("button").first()).toBeVisible();
    await expect(page.getByTestId("empty-state-start-sim")).toBeVisible();

    // 1. Connect to DU-EP800-CRG simulator via "..." menu — executes connect + probe + schema load + settings read
    await page.getByTestId("btn-more-menu").click();
    await page.getByTestId("btn-connect-sim").click();

    await expect(page.getByTestId("identity-strip")).toContainText("EP800-CRG");
    await expect(page.getByTestId("identity-strip")).toContainText("SN: 010203040506");
    await expect(page.getByTestId("identity-strip")).toContainText("DCAS: 4.2.0.0");

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
    await expect(txRail).toContainText("Changes to save (1)");
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

  test("reloads under existing service worker and reconnects cleanly", async ({ page }) => {
    await openInstrument(page);
    await page.getByTestId("btn-more-menu").click();
    await page.getByTestId("btn-connect-sim").click();
    await expect(page.getByTestId("identity-strip")).toContainText("EP800-CRG");

    // Reload under the existing service worker and reconnect to persisted storage
    await page.reload();
    await openInstrument(page);
    await page.getByTestId("btn-more-menu").click();
    await page.getByTestId("btn-connect-sim").click();
    await expect(page.getByTestId("identity-strip")).toContainText("EP800-CRG");
  });

  test("modern DU-EP600 / DU-EP801 session authentication and preset macros", async ({ page }) => {
    await openInstrument(page);

    // Select DU-EP801 flagship simulator (requires --ble-unit-auth and serial binding)
    await page.getByTestId("btn-more-menu").click();
    await page.getByTestId("sim-model-select").selectOption("ep801");
    await page.getByTestId("btn-connect-sim").click();

    await expect(page.getByTestId("identity-strip")).toContainText("EP801");
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

    // Verify single-MCU recovery override is collapsed by default inside <details>
    const overrideDetails = page.getByTestId("single-mcu-override");
    await expect(overrideDetails).not.toHaveAttribute("open", "");

    // Verify firmware risk notice is visible and can open the risk warning modal
    await expect(page.getByTestId("fw-risk-notice")).toBeVisible();
    await page.getByTestId("btn-review-fw-warning").click();
    const modal = page.getByTestId("risk-warning-modal");
    await expect(modal).toBeVisible();
    await expect(modal).toContainText("Tested on EP800-CRG only");
    await expect(modal).toContainText("No compatibility verification");
    await page.getByTestId("btn-dismiss-warning").click();
    await expect(modal).toBeHidden();
  });

  test("WebMCP external agent authorization flow", async ({ page }) => {
    await openInstrument(page);
    await page.getByTestId("btn-more-menu").click();
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
});

// Only the browser chooser is replaced. The selected device is the production
// GATT simulator; WebBleGatt, Rust authentication, probing and settings run intact.
async function selectSimulatedBluetoothDevice(page: Page, rejectAuthentication = false) {
  await page.addInitScript(() => {
    sessionStorage.setItem("esteps_ble_passkey", "000000");
  });
  await openInstrument(page);
  const setup = await page.evaluate(async (reject) => {
    const simulatorModule = "/simulator.js";
    const { DeviceSimulator } = await import(simulatorModule);
    const device = new DeviceSimulator({
      model: reject ? "ep801" : "ep800-crg",
      fault: reject ? "auth_reject" : "none",
    });
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
    // Re-evaluate supported browser transports after installing the chooser.
    (window as any).estepsInstrumentController.setSection("parameters");
    return { hasSimulator: Boolean(device) };
  }, rejectAuthentication);
  expect(setup).toEqual({ hasSimulator: true });
  await expect(page.getByTestId("btn-connect-ble")).toBeEnabled();
  await page.getByTestId("btn-connect-ble").click();
}

test("Bluetooth connect reads every setting and keeps routine notices out of errors", async ({ page }) => {
  test.setTimeout(60_000);
  await selectSimulatedBluetoothDevice(page);
  await expect(page.getByTestId("status-message")).toContainText("74 parameters read");
  await expect(page.getByTestId("error-banner")).toHaveCount(0);
  const state = await page.evaluate(() => {
    const state = (window as any).estepsInstrumentController.getState();
    return {
      phase: state.phase,
      transport: state.transport,
      errorBanner: state.errorBanner,
      settings: Object.keys(state.liveValues),
      logs: state.logs as { stream: string; text: string }[],
      chooserCalls: (window as any).testBluetoothChooserCalls(),
    };
  });
  expect(state.phase).toBe("ready");
  expect(state.transport).toBe("ble");
  expect(state.errorBanner).toBeNull();
  expect(state.settings).toHaveLength(74);
  expect(state.chooserCalls).toBe(1);
  const commands = state.logs.filter((entry) => entry.stream === "cmd").map((entry) => entry.text);
  expect(commands).toEqual(["cranklink> connect ble", "cranklink> --ble-master=master settings read"]);
  expect(state.logs.filter((entry) => entry.stream === "stderr")).toEqual([]);
  expect(state.logs.some((entry) => entry.stream === "notice" && /Bluetooth/i.test(entry.text))).toBe(true);
  expect(state.logs.map((entry) => entry.text).join("\n")).not.toMatch(/firmware operations disabled/i);
  await expect(page.getByTestId("param-row-tire-circumference")).toBeVisible();
  await page.getByTestId("nav-section-diagnostics").click();
  await expect(page.getByTestId("console-body")).toBeVisible();
  await expect(page.locator(".console-log-line.stderr")).toHaveCount(0);
  await expect(page.locator(".console-log-line.notice").first()).toBeVisible();
  const saved = await page.evaluate(async () => {
    const controller = (window as any).estepsInstrumentController;
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
  await page.evaluate(() => (window as any).estepsInstrumentController.disconnect());
  expect(await page.evaluate(() => (window as any).testBluetoothDevice.gatt.connected)).toBe(false);
  expect(await unloadIsGuarded(), "Confirmed disconnect releases the unload guard").toBe(false);
});

test("Bluetooth drive-unit authentication rejection closes the session and retains staged changes", async ({ page }) => {
  await selectSimulatedBluetoothDevice(page, true);
  await expect.poll(() => page.evaluate(() => (window as any).estepsInstrumentController.getState().phase)).toBe("ready");
  await page.evaluate(async () => {
    const controller = (window as any).estepsInstrumentController;
    controller.stageParameter("walk-assist", 0);
    await controller.commitStagedBatch().catch(() => {});
  });
  await expect(page.getByTestId("error-banner")).toBeVisible();
  await expect(page.getByTestId("status-message")).toContainText("cleanup wasn’t confirmed");
  const state = await page.evaluate(() => {
    const state = (window as any).estepsInstrumentController.getState();
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
  expect(commands).toHaveLength(3);
  expect(commands.slice(0, 2)).toEqual(["cranklink> connect ble", "cranklink> --ble-master=master settings read"]);
  expect(commands[2]).toContain("settings batch");
  expect(state.logs.some((entry) => entry.stream === "stderr" && /error:.*(?:auth|reject|status|challenge)/is.test(entry.text)))
    .toBe(true);
  const authLogs = await page.evaluate(() => (window as any).estepsInstrumentController.getState().logs);
  expect(authLogs.some((entry: { stream: string }) => entry.stream === "stderr")).toBe(true);
  expect(await page.evaluate(() => (window as any).testBluetoothDevice.gatt.connected)).toBe(false);
});
