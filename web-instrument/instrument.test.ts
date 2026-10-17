import { describe, it } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";

import { enrichSchema, formatSettingValue } from "./src/domain/schema.ts";
import {
  classifyFirmwareRole,
  evaluateCompanionPair,
  buildSynchronizedFlashPlan,
} from "./src/domain/firmwarePackage.ts";
import { SessionController } from "./src/core/sessionController.ts";
import { DeviceSimulator } from "./simulator.js";

describe("Domain Schema Enrichment & Ordering", () => {
  it("loads ep800-crg, ep600, and ep801 schemas and prioritizes Bike Hardware & Geometry", () => {
    for (const model of ["ep800-crg", "ep600", "ep801"] as const) {
      const raw = JSON.parse(
        fs.readFileSync(path.join(import.meta.dirname, "schemas", `${model}.json`), "utf8"),
      );
      const enriched = enrichSchema(raw, model);
      assert.ok(enriched.length > 30, `${model} schema has >30 parameters`);

      const geometryItems = enriched.filter((s) => s.section === "geometry");
      assert.equal(
        geometryItems[0].name,
        "tire-circumference",
        "tire-circumference is first in geometry",
      );
      assert.equal(
        geometryItems[1].name,
        "destination",
        "destination is second in geometry",
      );
      assert.equal(
        geometryItems[2].name,
        "speed-limit",
        "speed-limit is third in geometry",
      );

      const speedSpec = geometryItems[2];
      assert.equal(speedSpec.userStep, 1);
      assert.equal(speedSpec.userMin, model === "ep800-crg" ? 15 : 14);
      assert.equal(formatSettingValue(speedSpec, 25), "25 km/h");
    }
  });
});

describe("Synchronized Dual-MCU Firmware Package Domain", () => {
  it("classifies DCAS (-D) and RX (-M) images and validates companion match", () => {
    assert.equal(classifyFirmwareRole("DUEP800-D.4.2.0.dat"), "dcas");
    assert.equal(classifyFirmwareRole("DUEP800-M.4.1.11.dat"), "rx");

    const dcasFile = {
      filename: "DUEP800-D.4.2.0.dat",
      sizeBytes: 158052,
      sha256: "35521a18a7859482d98fab2523cbb97dfc653def4db00091c6265f2573f46543",
      role: "dcas" as const,
      metadata: {
        kind: "DcasX",
        series: 0x23,
        unit: 0,
        board: 1,
        version: "4.2.0.0",
        compatibility_version: "4.1.11.0",
        source_sha256: "35521a18a7859482d98fab2523cbb97dfc653def4db00091c6265f2573f46543",
      },
    };

    const rxFile = {
      filename: "DUEP800-M.4.1.11.dat",
      sizeBytes: 128192,
      sha256: "94986ebf8c21587a2b54b197f19a51c4118918bdca0aff6296a7727f7dfacc44",
      role: "rx" as const,
      metadata: {
        kind: "Renesas",
        series: 0x23,
        unit: 0,
        version: "4.1.11.0",
        source_sha256: "94986ebf8c21587a2b54b197f19a51c4118918bdca0aff6296a7727f7dfacc44",
      },
    };

    const evalRes = evaluateCompanionPair(dcasFile, rxFile);
    assert.equal(evalRes.valid, true);
    assert.match(evalRes.message, /Synchronized package verified/);

    const blePlan = buildSynchronizedFlashPlan({
      transport: "ble",
      serial: "010203040506",
      dcas: dcasFile,
      rx: rxFile,
      write: true,
      prefix: "test_fw",
    });
    assert.equal(blePlan.mode, "ble-pair");
    assert.equal(blePlan.commands.length, 1);
    assert.match(blePlan.commands[0], /firmware flash ble-pair/);
    assert.match(blePlan.commands[0], /--write/);

    const wiredPlan = buildSynchronizedFlashPlan({
      transport: "usb",
      serial: "010203040506",
      dcas: dcasFile,
      rx: rxFile,
      write: true,
      prefix: "test_wired",
    });
    assert.equal(wiredPlan.mode, "wired-pair");
    assert.equal(wiredPlan.commands.length, 3); // backup + flash-dcas + flash-rx
    assert.match(wiredPlan.commands[1], /firmware flash dcas/);
    assert.match(wiredPlan.commands[2], /firmware flash rx/);
  });
});

describe("SessionController Finite State Machine & Staging", () => {
  it("stages, unstages on revert-to-live, orders destination before speed-limit, and commits via WASM host", async () => {
    const controller = new SessionController();
    const executedCommands: string[] = [];
    const virtualFiles = new Map<string, Uint8Array>();

    controller.attachTerminal({
      status: () => ({ ready: true, busy: false, usbSelected: true, transport: "sim" }),
      files: {
        list: () => Array.from(virtualFiles.keys()).map((p) => ({ path: p })),
        bytes: (p: string) => virtualFiles.get(p) || new Uint8Array(),
        import: async (p: string, b: Uint8Array) => {
          virtualFiles.set(p, b);
        },
        initialize: async () => {},
      },
      run: async (line: string) => {
        executedCommands.push(line);
        const print = (globalThis as any).window?.estepsTerminalPrint;
        if (line.startsWith("connect")) {
          return { code: 0 };
        }
        if (line === "probe") {
          const out = JSON.stringify({
            slot: 13,
            cargo: "00-CRG",
            dcas_version: "4.1.11.0",
            rx_version: "4.1.11.0",
            serial: "010203040506",
            model_series: 0x23,
            model_unit: 0,
          });
          print?.(out, "stdout");
          return { code: 0 };
        }
        if (line.includes("settings schema")) {
          const raw = fs.readFileSync(
            path.join(import.meta.dirname, "schemas", "ep800-crg.json"),
            "utf8",
          );
          print?.(raw, "stdout");
          return { code: 0 };
        }
        if (line.includes("settings read")) {
          const out = JSON.stringify({
            model: "DU-EP800/DU-EP800-CRG",
            slot: 13,
            values: {
              "tire-circumference": { value: 2200, raw: "98080000" },
              destination: { value: 1, raw: "0101ffff" },
              "speed-limit": { value: 25, raw: "c409ffff" },
              lamp: { value: 0, raw: "00" },
            },
            unavailable: {},
          });
          print?.(out, "stdout");
          return { code: 0 };
        }
        if (line.includes("settings batch")) {
          const out = JSON.stringify({
            serial: "010203040506",
            slot: 13,
            completed: 2,
            results: [
              { setting: "destination", before: 1, after: 3, changed: true },
              { setting: "tire-circumference", before: 2200, after: 2280, changed: true },
            ],
          });
          print?.(out, "stdout");
          return { code: 0 };
        }
        return { code: 0 };
      },
    });

    (globalThis as any).window = globalThis;
    controller.attachTerminal((controller as any).terminal);

    await controller.connectAndSync("sim", { model: "ep800-crg" });
    assert.equal(controller.getState().phase, "ready");
    assert.equal(controller.getState().identity?.model, "DU-EP800-CRG");
    assert.equal(controller.getState().liveValues["tire-circumference"]?.value, 2200);

    // Stage tire-circumference to 2280 and destination to 3
    controller.stageParameter("tire-circumference", 2280);
    controller.stageParameter("destination", 3);
    assert.deepEqual(controller.getState().stagedValues, {
      "tire-circumference": 2280,
      destination: 3,
    });

    // Setting back to live value (2200) automatically unstages it
    controller.stageParameter("tire-circumference", 2200);
    assert.deepEqual(controller.getState().stagedValues, { destination: 3 });

    // Stage again and commit
    controller.stageParameter("tire-circumference", 2280);
    await controller.commitStagedBatch();

    assert.deepEqual(controller.getState().stagedValues, {});
    assert.equal(controller.getState().liveValues["tire-circumference"]?.value, 2280);
    assert.equal(controller.getState().liveValues["destination"]?.value, 3);
  });
});

describe("Speed and destination staging", () => {
  function connected(model: "ep800-crg" | "ep600" | "ep801", destination: number, speed: number) {
    const controller = new SessionController();
    (controller as any).update({
      phase: "ready", transport: "sim",
      identity: { modelSlug: model, serial: "010203040506" },
      schema: enrichSchema(JSON.parse(fs.readFileSync(path.join(import.meta.dirname, "schemas", `${model}.json`), "utf8")), model),
      liveValues: { destination: { value: destination, raw: "" }, "speed-limit": { value: speed, raw: "" } },
    });
    return controller;
  }

  it("pairs 32 km/h with destination 1 and removes the pair when speed is reverted", () => {
    const controller = connected("ep800-crg", 0, 25);
    controller.stageParameter("speed-limit", 32);
    assert.deepEqual(controller.getState().stagedValues, { destination: 1, "speed-limit": 32 });
    controller.revertParameter("speed-limit");
    assert.deepEqual(controller.getState().stagedValues, {});
    controller.stageParameter("destination", 3);
    controller.stageParameter("speed-limit", 32);
    assert.deepEqual(controller.getState().stagedValues, { destination: 1, "speed-limit": 32 });
    controller.stageParameter("speed-limit", 25);
    assert.deepEqual(controller.getState().stagedValues, { destination: 3 });
  });

  it("preserves compatible regions and adjusts speed when a region is selected explicitly", () => {
    const controller = connected("ep800-crg", 1, 32);
    controller.stageParameter("speed-limit", 20);
    assert.deepEqual(controller.getState().stagedValues, { "speed-limit": 20 });
    controller.stageParameter("speed-limit", 18);
    assert.deepEqual(controller.getState().stagedValues, { destination: 0, "speed-limit": 18 });
    controller.stageParameter("speed-limit", 32);
    assert.deepEqual(controller.getState().stagedValues, {});
    controller.stageParameter("destination", 3);
    assert.deepEqual(controller.getState().stagedValues, { destination: 3, "speed-limit": 25 });
  });

  it("uses modern destination ranges and rejects unsupported legacy speeds", () => {
    for (const model of ["ep600", "ep801"] as const) {
      const controller = connected(model, 2, 24);
      controller.stageParameter("speed-limit", 45);
      assert.deepEqual(controller.getState().stagedValues, { destination: 5, "speed-limit": 45 });
      controller.stageParameter("destination", 2);
      assert.deepEqual(controller.getState().stagedValues, {});
      controller.stageParameter("speed-limit", 14);
      assert.deepEqual(controller.getState().stagedValues, { "speed-limit": 14 });
    }
    const controller = connected("ep800-crg", 0, 25);
    controller.stageParameter("speed-limit", 45);
    assert.deepEqual(controller.getState().stagedValues, {});
    assert.match(controller.getState().errorBanner!, /No supported destination/);
  });

  it("writes destination before speed and keeps a failed save in settings", async () => {
    (globalThis as any).window = globalThis;
    const controller = connected("ep800-crg", 0, 25);
    let operations: any[] = [];
    let fail = false;
    const files = new Map<string, Uint8Array>();
    controller.attachTerminal({
      status: () => ({ ready: true, busy: false, usbSelected: false, transport: "sim" }),
      files: {
        list: () => [], bytes: (p) => files.get(p)!, initialize: async () => {},
        import: async (p, bytes) => { files.set(p, bytes); },
      },
      run: async () => {
        operations = JSON.parse(new TextDecoder().decode([...files.values()].at(-1)));
        if (fail) {
          (globalThis as any).estepsTerminalPrint("write rejected", "stderr");
          return { code: 1 };
        }
        (globalThis as any).estepsTerminalPrint(JSON.stringify({ results: operations.map((op) => ({ setting: op.name, after: Number(op.value) })) }), "stdout");
        return { code: 0 };
      },
    });
    controller.stageParameter("speed-limit", 32);
    await controller.commitStagedBatch();
    assert.deepEqual(operations.map(({ name, value }) => [name, value]), [["destination", "1"], ["speed-limit", "32"]]);
    assert.equal(controller.getState().liveValues.destination.value, 1);
    assert.equal(controller.getState().liveValues["speed-limit"].value, 32);
    controller.stageParameter("speed-limit", 31);
    fail = true;
    await assert.rejects(controller.commitStagedBatch(), /write rejected/);
    assert.equal(controller.getState().phase, "ready");
    assert.equal(controller.getState().errorBanner, "write rejected");
    assert.deepEqual(controller.getState().stagedValues, { "speed-limit": 31 });
  });
});

describe("Wire-Level GATT Simulator Register Completeness", () => {
  it("handles EP800-CRG, EP600, and EP801 identity and full register queries", () => {
    const sim = new DeviceSimulator({ model: "ep800-crg" });
    const replies: number[][] = [];
    const char2afd = sim.chars.find((c: any) => c.id === 0x2afd)!;
    char2afd.addEventListener("characteristicvaluechanged", () => {
      replies.push(Array.from(new Uint8Array(char2afd.value!.buffer)));
    });

    // Query model identity (group 1, cmd 0x34)
    sim.write(0x2afe, [13, 1, 0x34, 0]);
    assert.deepEqual(replies.at(-1), [13, 1, 0x36, 0, 0x23, 0]);

    // Write tire circumference via group 0x35 cmd 0x00 (2300 = 0x08fc)
    sim.write(0x2afe, [13, 0x35, 0x00, 0xfc, 0x08, 0, 0]);
    assert.equal(sim.getRegisters().tireCircumferenceMm, 2300);

    // Read tire circumference via group 0x35 cmd 0x04
    sim.write(0x2afe, [13, 0x35, 0x04, 0]);
    assert.deepEqual(replies.at(-1), [13, 0x35, 0x06, 0xfc, 0x08, 0, 0]);

    // Legacy EP800-CRG drive unit authentication exchange (0x16 0xd8 -> 0xda challenge, 0x16 0xe0 -> 0xe2 receipt)
    replies.length = 0;
    sim.write(0x2afe, [13, 0x16, 0xd8, 1, 2, 3, 4, 5, 6, 7]);
    assert.equal(replies.length, 3, "challenge emits 3 fragments");
    assert.equal(replies[0][2], 0xda);
    assert.equal(replies[0][3], 0x16);
    assert.equal(replies[1][3], 0x26);
    assert.equal(replies[2][3], 0x34);

    replies.length = 0;
    sim.write(0x2afe, [13, 0x16, 0xe0, 0x16, 1, 2, 3, 4, 5, 6]);
    sim.write(0x2afe, [13, 0x16, 0xe0, 0x26, 7, 8, 9, 10, 11, 12]);
    assert.equal(replies.length, 0, "first two proof fragments do not emit receipt");
    sim.write(0x2afe, [13, 0x16, 0xe0, 0x34, 13, 14, 15, 16, 0xff, 0xff]);
    assert.equal(replies.length, 1, "third proof fragment emits receipt");
    assert.deepEqual(replies[0], [13, 0x16, 0xe2, 0xff, 0xff, 0xff, 0xff]);
    assert.equal(sim.unitAuthenticated, true);
  });
});

describe("Brave Browser Detection & Web Bluetooth Availability", () => {
  const origNav = globalThis.navigator;

  it("detects Brave browser via navigator.brave", async () => {
    const { detectPlatform, isBraveBrowser } = await import("./src/core/platform.ts");

    // Simulate Brave
    Object.defineProperty(globalThis, "navigator", {
      value: {
        userAgent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
        brave: { isBrave: async () => true },
        bluetooth: {},
      },
      configurable: true,
    });

    assert.equal(detectPlatform().isBrave, true);
    assert.equal(await isBraveBrowser(), true);

    // Simulate standard Chrome
    Object.defineProperty(globalThis, "navigator", {
      value: {
        userAgent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
        bluetooth: {},
      },
      configurable: true,
    });

    assert.equal(detectPlatform().isBrave, false);
    assert.equal(await isBraveBrowser(), false);

    Object.defineProperty(globalThis, "navigator", { value: origNav, configurable: true });
  });

  it("checks Bluetooth availability properly in Brave vs Chrome", async () => {
    const { checkBluetoothAvailability } = await import("./src/core/platform.ts");

    // Brave with Bluetooth disabled globally (default)
    Object.defineProperty(globalThis, "navigator", {
      value: {
        brave: { isBrave: async () => true },
        bluetooth: {
          getAvailability: async () => false,
        },
      },
      configurable: true,
    });
    assert.equal(await checkBluetoothAvailability(), false);

    // Brave without getAvailability implemented
    Object.defineProperty(globalThis, "navigator", {
      value: {
        brave: { isBrave: async () => true },
        bluetooth: {},
      },
      configurable: true,
    });
    assert.equal(await checkBluetoothAvailability(), false);

    // Brave with Web Bluetooth flag enabled by user
    Object.defineProperty(globalThis, "navigator", {
      value: {
        brave: { isBrave: async () => true },
        bluetooth: {
          getAvailability: async () => true,
        },
      },
      configurable: true,
    });
    assert.equal(await checkBluetoothAvailability(), true);

    // Standard Chrome with Bluetooth available
    Object.defineProperty(globalThis, "navigator", {
      value: {
        bluetooth: {
          getAvailability: async () => true,
        },
      },
      configurable: true,
    });
    assert.equal(await checkBluetoothAvailability(), true);

    Object.defineProperty(globalThis, "navigator", { value: origNav, configurable: true });
  });

  it("WebBleGatt.choose provides actionable guidance on globally disabled error", async () => {
    const { WebBleGatt } = await import("./ble.js");
    (globalThis as any).isSecureContext = true;

    const mockBluetooth = {
      requestDevice: async () => {
        const err = new Error("Web Bluetooth API globally disabled.");
        err.name = "NotFoundError";
        throw err;
      },
    };

    await assert.rejects(
      WebBleGatt.choose(mockBluetooth),
      /Web Bluetooth is disabled globally in this browser\. Use Google Chrome or Microsoft Edge to connect wirelessly, or enable brave:\/\/flags\/#enable-web-bluetooth in Brave\./,
    );
  });
});
