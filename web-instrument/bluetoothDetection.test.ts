import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { SessionController, type TerminalHost } from "./src/core/sessionController.ts";

function harness(system = "steps", series: number | null = 0x23, inventorySeries = series) {
  (globalThis as any).window = globalThis;
  const controller = new SessionController();
  const commands: string[] = [];
  const config = { detectError: "", system, series, inventorySeries };
  const terminal: TerminalHost = {
    setPasskeyPrompt() {},
    status: () => ({ ready: true, busy: false, usbSelected: false, transport: "ble" }),
    files: { list: () => [], bytes: () => new Uint8Array(), initialize: async () => {}, import: async () => {} },
    run: async (line) => {
      commands.push(line);
      const print = (value: unknown, stream = "stdout") => (globalThis as any).estepsTerminalPrint(JSON.stringify(value), stream);
      if (line === "connect ble" || line === "disconnect") return { code: 0 };
      if (line === "ble detect-system") {
        if (config.detectError) {
          (globalThis as any).estepsTerminalPrint(config.detectError, "stderr");
          return { code: 1 };
        }
        print({ system: config.system, master_series: config.series, service_started: false });
        return { code: 0 };
      }
      if (line.endsWith("units")) {
        print({ units: config.inventorySeries === null ? [] : [{ slot: 13, model_series: config.inventorySeries, model_unit: 0, model: null, model_candidates: [] }] });
        return { code: 0 };
      }
      if (line.endsWith("settings read")) {
        print({ identity: { slot: 13, cargo: "00-CRG", serial: "010203040506", model_series: config.inventorySeries, model_unit: 0, dcas_version: "4.2.0.0", rx_version: "4.1.11.0" }, values: {}, unavailable: {} });
        return { code: 0 };
      }
      if (line.startsWith("settings schema")) { print([]); return { code: 0 }; }
      assert.fail(`Unexpected command: ${line}`);
    },
  };
  controller.attachTerminal(terminal);
  return { controller, commands, config };
}

describe("Pre-service Bluetooth system selection", () => {
  it("detects STEPS before inventory without another connect or disconnect", async () => {
    const h = harness();
    await h.controller.connectAndSync("ble");
    assert.deepEqual(h.commands.slice(0, 3), ["connect ble", "ble detect-system", "--ble-master=master units"]);
    assert.equal(h.commands.filter((line) => line === "connect ble").length, 1);
    assert.ok(!h.commands.includes("disconnect"));
    assert.equal(h.controller.getState().bluetoothSystem, "steps");
    assert.equal(h.controller.getState().bluetoothSystemSource, "detected");
    assert.equal(h.controller.getState().failure, null);
  });

  it("detects a shifting-only master without assuming Di2 components exclude STEPS", async () => {
    const h = harness("shifting-only", 0x11, null);
    await h.controller.connectAndSync("ble");
    assert.deepEqual(h.commands, ["connect ble", "ble detect-system", "--ble-master=normal units"]);
    assert.equal(h.controller.getState().bluetoothSystem, "di2");
  });

  it("holds an unknown identity before inventory and continues on the original connection", async () => {
    const h = harness("unknown", 0x08, null);
    let stop = () => {};
    const prompt = new Promise<void>((resolve) => { stop = h.controller.subscribe((state) => { if (state.bluetoothSystemPrompt) resolve(); }); });
    const connected = h.controller.connectAndSync("ble");
    await prompt;
    stop();
    assert.deepEqual(h.commands, ["connect ble", "ble detect-system"]);
    assert.equal(h.controller.getState().bluetoothSystem, null);
    h.controller.answerBluetoothSystem("di2");
    await connected;
    assert.deepEqual(h.commands, ["connect ble", "ble detect-system", "--ble-master=normal units"]);
    assert.equal(h.controller.getState().bluetoothSystemSource, "manual");
  });

  it("cancels an unknown identity without starting inventory", async () => {
    const h = harness("unknown", 0xaa, null);
    let stop = () => {};
    const prompt = new Promise<void>((resolve) => { stop = h.controller.subscribe((state) => { if (state.bluetoothSystemPrompt) resolve(); }); });
    const connected = h.controller.connectAndSync("ble");
    await prompt;
    stop();
    h.controller.answerBluetoothSystem(null);
    await assert.rejects(connected, /cancelled before entering service mode/);
    assert.deepEqual(h.commands, ["connect ble", "ble detect-system", "disconnect"]);
    assert.equal(h.controller.getState().bluetoothSystemPrompt, null);
  });

  it("does not silently fall back or retry after a detection timeout", async () => {
    const h = harness();
    h.config.detectError = "BLE response timeout; reconnect before another command";
    await assert.rejects(h.controller.connectAndSync("ble"), /BLE response timeout/);
    assert.deepEqual(h.commands, ["connect ble", "ble detect-system", "disconnect"]);
    assert.equal(h.controller.getState().bluetoothSystemPrompt, null);
    h.controller.setBluetoothBike("di2");
    h.config.inventorySeries = null;
    await h.controller.connectAndSync("ble");
    assert.deepEqual(h.commands.slice(3), ["connect ble", "--ble-master=normal units"]);
    assert.equal(h.controller.getState().bluetoothSystemSource, "manual");
  });

  it("closes an inventory that contradicts detection before reading settings", async () => {
    for (const [system, series, actual] of [["shifting-only", 0x11, 0x23], ["steps", 0x23, 0x35]] as const) {
      const h = harness(system, series, actual);
      await assert.rejects(h.controller.connectAndSync("ble"), /detected STEPS master|detected as shifting only/);
      assert.equal(h.commands.at(-1), "disconnect");
      assert.ok(!h.commands.some((line) => line.endsWith("settings read")));
    }
  });
});
