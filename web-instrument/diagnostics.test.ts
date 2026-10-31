import { strict as assert } from "node:assert";
import { test } from "node:test";
import { SessionController } from "./src/core/sessionController.ts";
import { formatDiagnosticReport, redactDiagnosticText } from "./src/domain/diagnostics.ts";

test("failed connection report keeps full cause, cleanup, routing and command history", () => {
  const state = new SessionController().getState();
  state.transport = "ble";
  state.phase = "error";
  state.bluetoothBike = "steps";
  state.bluetoothSystem = "steps";
  state.bluetoothMasterSeries = 0x12;
  state.failure = { kind: "connect", error: "GATT write failed (gateway PCE 03 mode 20)\nCleanup also failed: release unconfirmed" };
  state.logs = [{ id: 1, timestamp: "14:00:00", stream: "cmd", text: "cranklink> --ble-master units" }];
  const report = formatDiagnosticReport(state, { url: "https://example.test/diagnostics?token=private#secret", userAgent: "Test browser", generatedAt: "2026-10-31T14:00:00Z" });
  assert.ok(report.includes(state.failure.error));
  assert.ok(report.includes("Bluetooth override: steps"));
  assert.ok(report.includes("Master series: 0x12"));
  assert.ok(report.includes("[14:00:00] cmd: cranklink> --ble-master units"));
  assert.ok(report.includes("Page: https://example.test/diagnostics"));
  assert.ok(!report.includes("private"));
  assert.ok(!report.includes("secret"));
});

test("explicit PIN arguments are redacted from reports without removing other flags", () => {
  for (const argument of ['--passkey 123456', '--passkey=123456', '--passkey "123456"', "--passkey='123456'"]) {
    const result = redactDiagnosticText(`cranklink ${argument} --ble-master units`);
    assert.ok(!result.includes("123456"));
    assert.ok(result.includes("[redacted]"));
    assert.ok(result.includes("--ble-master units"));
  }
});
