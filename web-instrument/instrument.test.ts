import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";

import { enrichSchema, TIRE_PRESETS, settingAcceptsValue, clampSettingValue } from "./src/domain/schema.ts";
import {
  classifyFirmwareRole,
  evaluateCompanionPair,
  buildSynchronizedFlashPlan,
  shellQuote,
} from "./src/domain/firmwarePackage.ts";
import { describeFailure, userFacingError } from "./src/domain/errors.ts";
import { WebUsbPort } from "./usb.js";
import { WebSerialPort } from "./serial.js";
import { formatSetting, formatValue, isRegulatedSetting } from "./src/domain/settingFormat.ts";
import { detectMotorIdentity, friendlyMotorName } from "./src/domain/motorIdentity.ts";
import { SessionController } from "./src/core/sessionController.ts";
import { preferredTransport } from "./src/core/platform.ts";
import { DeviceSimulator } from "./simulator.js";
import type { SettingsModel } from "./src/types.ts";

describe("Domain Schema Enrichment & Ordering", () => {
  it("keeps E6000 geometry controls and presets within the detected schema", () => {
    const schema = enrichSchema(JSON.parse(fs.readFileSync(path.join(import.meta.dirname, "schemas", "e6000.json"), "utf8")), "e6000");
    const tire = schema.find((setting) => setting.name === "tire-circumference")!;
    const angle = schema.find((setting) => setting.name === "mounting-angle")!;
    assert.deepEqual([tire.userMin, tire.userMax, tire.userStep], [2000, 2400, 1]);
    assert.deepEqual([angle.userMin, angle.userMax, angle.userStep], [0, 45, 1]);
    assert.deepEqual(TIRE_PRESETS.filter((preset) => settingAcceptsValue(tire, Number(preset.value))).map((preset) => preset.value), [2288, 2180, 2055]);
    assert.equal(clampSettingValue(tire, 1990), 2000);
    assert.equal(clampSettingValue(tire, 2405), 2400);
    assert.equal(clampSettingValue({ ...tire, userStep: 5 }, 2003), 2005);
    assert.equal(settingAcceptsValue({ ...tire, userStep: 5 }, 2003), false);
  });

  it("loads supported motor schemas and prioritizes Bike Hardware & Geometry", () => {
    for (const model of ["ep800-crg", "ep600", "ep801", "ep500"] as const) {
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
      assert.equal(formatValue(speedSpec, 25), "25 km/h");
    }
  });

  it("offers modern torque levels only for OEM bounds with a vendor table", async () => {
    const { modernTorqueLevels } = await import("./src/domain/schema.ts");
    assert.deepEqual(modernTorqueLevels("ep801", "eco", 20, 85), [20, 27, 34, 42, 49, 56, 63, 70]);
    assert.deepEqual(modernTorqueLevels("ep600", "boost", 20, 50), [27, 30, 33, 36, 40, 43, 46, 50]);
    assert.deepEqual(modernTorqueLevels("ep801", "trail", 25, 80), []);
    assert.equal(modernTorqueLevels("ep800-crg", "trail", 25, 80), undefined);
  });
});

describe("Synchronized Dual-MCU Firmware Package Domain", () => {
  it("classifies DCAS (-D) and RX (-M) images and validates companion match", () => {
    assert.equal(classifyFirmwareRole("DUEP800-D.4.2.0.dat"), "dcas");
    assert.equal(classifyFirmwareRole("DUEP800-M.4.1.11.dat"), "rx");
    // Metadata from the WASM core wins over the filename.
    const meta = { series: 0x23, unit: 0, version: "4.2.0.0", source_sha256: "" };
    assert.equal(classifyFirmwareRole("motor.bin", { ...meta, kind: "dcas-x" }), "dcas");
    assert.equal(classifyFirmwareRole("motor.bin", { ...meta, kind: "dcas2i" }), "dcas");
    assert.equal(classifyFirmwareRole("motor-D.bin", { ...meta, kind: "renesas" }), "rx");

    const dcasFile = {
      filename: "DUEP800-D.4.2.0.dat",
      sizeBytes: 158052,
      sha256: "35521a18a7859482d98fab2523cbb97dfc653def4db00091c6265f2573f46543",
      role: "dcas" as const,
      metadata: {
        kind: "dcas-x" as const,
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
        kind: "renesas" as const,
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
      setPasskeyPrompt: () => {},
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
        if (line === "units") {
          const out = JSON.stringify({ units: [{ slot: 13, model_series: 0x23, model_unit: 0, model: "DU-EP800-CRG", model_candidates: ["DU-EP800-CRG"] }] });
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
            identity: { slot: 13, cargo: "00-CRG", dcas_version: "4.1.11.0", rx_version: "4.1.11.0", serial: "010203040506", model_series: 0x23, model_unit: 0, model_name: "DU-EP800-CRG", settings_model: "ep800-crg" },
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

describe("Firmware image import", () => {
  it("stores a different image under a taken name beside the stored one", async () => {
    (globalThis as any).window = globalThis;
    const controller = new SessionController();
    const stored = new Map<string, Uint8Array>();
    const commands: string[] = [];
    controller.attachTerminal({
      setPasskeyPrompt: () => {},
      status: () => ({ ready: true, busy: false, usbSelected: false, transport: null }),
      files: {
        list: () => [...stored.keys()].map((p) => ({ path: p })),
        bytes: (p: string) => stored.get(p)!,
        import: async (p: string, b: Uint8Array) => {
          if (stored.has(p)) throw new Error(`File already exists: ${p}`);
          stored.set(p, b);
        },
        initialize: async () => {},
      },
      run: async (line: string) => {
        commands.push(line);
        // Like the WASM inspect, report the version held in the stored bytes.
        const name = /^firmware inspect '([^']+)'$/.exec(line)?.[1];
        if (name) {
          const meta = { kind: "dcas-x", series: 0x23, unit: 0, version: `4.6.${stored.get(name)![0]}`, source_sha256: "" };
          (globalThis as any).window.estepsTerminalPrint(JSON.stringify(meta), "stdout");
        }
        return { code: 0 };
      },
    });
    const name = "DUEP800-D.4.6.0.dat";
    const load = async (bytes: number[]) => {
      await controller.importFirmwarePackageFiles([new File([new Uint8Array(bytes)], name)]);
      return controller.getState().firmware.dcasFile!;
    };

    assert.equal((await load([1, 1])).filename, name);
    const replacement = await load([2, 2]);
    assert.equal(replacement.sha256, createHash("sha256").update(new Uint8Array([2, 2])).digest("hex"));
    assert.equal(replacement.filename, `DUEP800-D.4.6.0.${replacement.sha256.slice(0, 12)}.dat`);
    assert.equal(commands.at(-1), `firmware inspect '${replacement.filename}'`);
    assert.equal(replacement.metadata?.version, "4.6.2");
    assert.equal(replacement.role, "dcas");
    assert.deepEqual(stored.get(name), new Uint8Array([1, 1]));

    // Re-importing either image reuses its stored copy.
    assert.equal((await load([1, 1])).filename, name);
    assert.equal((await load([2, 2])).filename, replacement.filename);
    assert.equal(stored.size, 2);
  });

  it("reuses an image another tab stored after this tab loaded the store", async () => {
    (globalThis as any).window = globalThis;
    const controller = new SessionController();
    const durable = new Map<string, Uint8Array>();
    const loaded = new Map<string, Uint8Array>();
    const commands: string[] = [];
    controller.attachTerminal({
      setPasskeyPrompt: () => {},
      status: () => ({ ready: true, busy: false, usbSelected: false, transport: null }),
      files: {
        list: () => [...loaded.keys()].map((p) => ({ path: p })),
        bytes: (p: string) => loaded.get(p)!,
        // Like IndexedDB's add(), refuse a name any tab already stored.
        import: async (p: string, b: Uint8Array) => {
          if (durable.has(p)) throw new Error(`File already exists: ${p}`);
          durable.set(p, b);
          loaded.set(p, b);
        },
        initialize: async () => {
          for (const [p, b] of durable) loaded.set(p, b);
        },
      },
      run: async (line: string) => {
        commands.push(line);
        return { code: 0 };
      },
    });
    const name = "DUEP800-D.4.6.0.dat";
    durable.set(name, new Uint8Array([1, 1]));
    await controller.importFirmwarePackageFiles([new File([new Uint8Array([1, 1])], name)]);
    assert.equal(controller.getState().firmware.dcasFile?.filename, name);
    assert.deepEqual(commands, [`firmware inspect '${name}'`]);
  });

  it("refuses an image whose hash-tagged name holds other bytes", async () => {
    (globalThis as any).window = globalThis;
    const controller = new SessionController();
    const name = "DUEP800-D.4.6.0.dat";
    const tag = createHash("sha256").update(new Uint8Array([2, 2])).digest("hex").slice(0, 12);
    const stored = new Map<string, Uint8Array>([
      [name, new Uint8Array([1, 1])],
      [`DUEP800-D.4.6.0.${tag}.dat`, new Uint8Array([3, 3])],
    ]);
    const commands: string[] = [];
    controller.attachTerminal({
      setPasskeyPrompt: () => {},
      status: () => ({ ready: true, busy: false, usbSelected: false, transport: null }),
      files: {
        list: () => [...stored.keys()].map((p) => ({ path: p })),
        bytes: (p: string) => stored.get(p)!,
        import: async () => assert.fail("nothing may be stored"),
        initialize: async () => {},
      },
      run: async (line: string) => {
        commands.push(line);
        return { code: 0 };
      },
    });
    await assert.rejects(
      controller.importFirmwarePackageFiles([new File([new Uint8Array([2, 2])], name)]),
      /already holds a different image/,
    );
    // The firmware view ignores the rejection, so the banner must say it.
    assert.equal(controller.getState().failure?.kind, "firmware");
    assert.deepEqual(commands, []);
  });

  it("quotes a file name that contains an apostrophe", async () => {
    (globalThis as any).window = globalThis;
    const controller = new SessionController();
    const commands: string[] = [];
    controller.attachTerminal({
      setPasskeyPrompt: () => {},
      status: () => ({ ready: true, busy: false, usbSelected: false, transport: null }),
      files: { list: () => [], bytes: () => new Uint8Array(), import: async () => {}, initialize: async () => {} },
      run: async (line: string) => {
        commands.push(line);
        return { code: 0 };
      },
    });
    // The terminal parser reads 'a'\''b' as the single argument a'b.
    assert.equal(shellQuote("a'b"), "'a'\\''b'");
    await controller.importFirmwarePackageFiles([new File([new Uint8Array([1])], "Bob's DUEP800-D.dat")]);
    assert.deepEqual(commands, ["firmware inspect 'Bob'\\''s DUEP800-D.dat'"]);
  });
});

describe("Firmware programming preconditions", () => {
  it("explains a refused single-MCU program instead of failing silently", async () => {
    (globalThis as any).window = globalThis;
    const controller = new SessionController();
    const commands: string[] = [];
    controller.attachTerminal({
      setPasskeyPrompt: () => {},
      status: () => ({ ready: true, busy: false, usbSelected: true, transport: "usb" }),
      files: { list: () => [], bytes: () => new Uint8Array(), import: async () => {}, initialize: async () => {} },
      run: async (line: string) => {
        commands.push(line);
        return { code: 0 };
      },
    });
    const image = (role: "dcas" | "rx", filename: string) => ({ filename, sizeBytes: 1, sha256: "00".repeat(32), role, metadata: null });
    (controller as any).update({
      phase: "ready", transport: "usb",
      identity: { slot: 0, modelSlug: "ep800-crg", model: "DU-EP800-CRG", isMotor: true, modelUnit: 0, serial: "010203040506" },
      firmware: { ...controller.getState().firmware, dcasFile: image("dcas", "DUEP800-D.4.2.0.dat") },
    });
    await assert.rejects(controller.executeFirmwareSync(true, "dcas"), /companion pair first/);
    assert.equal(controller.getState().failure?.kind, "firmware");

    // A check that passed on another drive unit does not permit programming this one.
    controller.dismissError();
    (controller as any).update({
      firmware: {
        ...controller.getState().firmware, rxFile: image("rx", "DUEP800-M.4.1.11.dat"),
        companionValid: true, dryRunPassed: true, dryRunSerial: "0A0B0C0D0E0F",
      },
    });
    await assert.rejects(controller.executeFirmwareSync(true, "rx"), /non-erasing compatibility check on this drive unit/);
    assert.equal(controller.getState().failure?.kind, "firmware");

    // A closed link refuses every firmware operation, even after a passed check.
    controller.dismissError();
    (controller as any).update({ linkLost: true, firmware: { ...controller.getState().firmware, dryRunSerial: "010203040506" } });
    await assert.rejects(controller.executeFirmwareSync(true), /Reconnect to an idle drive unit/);
    assert.equal(controller.getState().failure?.kind, "firmware");
    await assert.rejects(controller.executeFirmwareSync(false), /Reconnect to an idle drive unit/);
    assert.deepEqual(commands, []);
  });
});

describe("Firmware programming progress", () => {
  it("shows the write progress the CLI reports on stderr", async () => {
    (globalThis as any).window = globalThis;
    const controller = new SessionController();
    const seen: string[] = [];
    controller.attachTerminal({
      setPasskeyPrompt: () => {},
      status: () => ({ ready: true, busy: false, usbSelected: true, transport: "usb" }),
      files: { list: () => [], bytes: () => new Uint8Array(), import: async () => {}, initialize: async () => {} },
      run: async () => {
        (globalThis as any).estepsTerminalPrint("Programming 8192/65536 bytes (12%)\n", "stderr");
        seen.push(controller.getState().statusMessage);
        return { code: 0 };
      },
    });
    const image = (role: "dcas" | "rx", filename: string) => ({ filename, sizeBytes: 1, sha256: "00".repeat(32), role, metadata: null });
    (controller as any).update({
      phase: "ready", transport: "usb",
      identity: { slot: 0, modelSlug: "ep800-crg", model: "DU-EP800-CRG", isMotor: true, modelUnit: 0, serial: "010203040506" },
      firmware: {
        ...controller.getState().firmware,
        dcasFile: image("dcas", "DUEP800-D.4.2.0.dat"), rxFile: image("rx", "DUEP800-M.4.2.0.dat"),
        companionValid: true, dryRunPassed: true, dryRunSerial: "010203040506",
      },
    });
    await controller.executeFirmwareSync(true, "dcas");
    assert.deepEqual(seen, ["Writing DCAS firmware: 12% (8192 of 65536 bytes)"]);
  });

  it("names the chip ble-pair is writing", async () => {
    (globalThis as any).window = globalThis;
    const controller = new SessionController();
    const seen: string[] = [];
    controller.attachTerminal({
      setPasskeyPrompt: () => {},
      status: () => ({ ready: true, busy: false, usbSelected: true, transport: "ble" }),
      files: { list: () => [], bytes: () => new Uint8Array(), import: async () => {}, initialize: async () => {} },
      run: async () => {
        for (const line of ["Programming 65536/65536 bytes (100%)", "Programming 8192/32768 bytes (25%)", "Programming 16384/32768 bytes (50%)"]) {
          (globalThis as any).estepsTerminalPrint(`${line}\n`, "stderr");
          seen.push(controller.getState().statusMessage);
        }
        return { code: 0 };
      },
    });
    const image = (role: "dcas" | "rx", filename: string) => ({ filename, sizeBytes: 1, sha256: "00".repeat(32), role, metadata: null });
    (controller as any).update({
      phase: "ready", transport: "ble",
      identity: { slot: 0, modelSlug: "ep800-crg", model: "DU-EP800-CRG", isMotor: true, modelUnit: 0, serial: "010203040506" },
      firmware: {
        ...controller.getState().firmware,
        dcasFile: image("dcas", "DUEP800-D.4.2.0.dat"), rxFile: image("rx", "DUEP800-M.4.2.0.dat"),
        companionValid: true, dryRunPassed: true, dryRunSerial: "010203040506",
      },
    });
    await controller.executeFirmwareSync(true);
    assert.deepEqual(seen, [
      "Writing DCAS firmware: 100% (65536 of 65536 bytes)",
      "Writing RX firmware: 25% (8192 of 32768 bytes)",
      "Writing RX firmware: 50% (16384 of 32768 bytes)",
    ]);
  });

  it("treats a failed Bluetooth write as critical, not as a lost link", async () => {
    (globalThis as any).window = globalThis;
    const controller = new SessionController();
    const stderr = "Programming 8192/65536 bytes (12%)\nerror: Bluetooth disconnected; reconnect before another command\n";
    controller.attachTerminal({
      setPasskeyPrompt: () => {},
      status: () => ({ ready: true, busy: false, usbSelected: true, transport: "ble" }),
      files: { list: () => [], bytes: () => new Uint8Array(), import: async () => {}, initialize: async () => {} },
      run: async () => {
        (globalThis as any).estepsTerminalPrint(stderr, "stderr");
        return { code: 1 };
      },
    });
    const image = (role: "dcas" | "rx", filename: string) => ({ filename, sizeBytes: 1, sha256: "00".repeat(32), role, metadata: null });
    (controller as any).update({
      phase: "ready", transport: "ble",
      identity: { slot: 0, modelSlug: "ep800-crg", model: "DU-EP800-CRG", isMotor: true, modelUnit: 0, serial: "010203040506" },
      firmware: {
        ...controller.getState().firmware,
        dcasFile: image("dcas", "DUEP800-D.4.2.0.dat"), rxFile: image("rx", "DUEP800-M.4.2.0.dat"),
        companionValid: true, dryRunPassed: true, dryRunSerial: "010203040506",
      },
    });
    await assert.rejects(controller.executeFirmwareSync(true), /Bluetooth disconnected/);
    const state = controller.getState();
    // A lost link would bring back Reconnect once the banner is dismissed.
    assert.equal(state.linkLost, false);
    assert.equal(describeFailure(state.failure!, state.transport, []).critical, true);
  });
});

describe("BLE initial settings connection", () => {
  function terminalFor(snapshot: any, commands: string[]) {
    return {
      setPasskeyPrompt: () => {},
      status: () => ({ ready: true, busy: false, usbSelected: true, transport: "ble" }),
      files: { list: () => [], bytes: () => new Uint8Array(), import: async () => {}, initialize: async () => {} },
      run: async (line: string) => {
        commands.push(line);
        const print = (globalThis as any).estepsTerminalPrint;
        if (line === "connect ble") return { code: 0 };
        if (line === "--ble-master=master units") {
          print(JSON.stringify({ units: [{ slot: snapshot.identity?.slot ?? 13, model_series: snapshot.identity?.model_series ?? 0x23, model_unit: snapshot.identity?.model_unit ?? 0, model: null, model_candidates: [] }] }), "stdout");
          return { code: 0 };
        }
        if (line.startsWith("settings schema")) {
          const model = line.split("--model ")[1].replaceAll("'", "");
          print(fs.readFileSync(path.join(import.meta.dirname, "schemas", `${model}.json`), "utf8"), "stdout");
          return { code: 0 };
        }
        assert.match(line, /^--ble-master=master --slot \d+ (?:--ble-unit-auth )?settings read$/, "Read the selected target without a preliminary probe");
        assert.equal(commands.filter((command) => command.endsWith("settings read")).length, 1);
        print(JSON.stringify(snapshot), "stdout");
        return { code: 0 };
      },
    };
  }

  it("loads verified identity and parameters from one read, preserving unavailable fields", async () => {
    (globalThis as any).window = globalThis;
    const controller = new SessionController();
    const commands: string[] = [];
    controller.attachTerminal(terminalFor({
      identity: { slot: 0, cargo: "00-CRG", serial: "010203040506", model_series: 0x23, model_unit: 0, dcas_version: "4.1.11.0", rx_version: "4.1.11.0" },
      values: { destination: { value: 0, raw: "01000000" }, "speed-limit": { value: 25, raw: "c409ffff" } },
      unavailable: { lamp: "not available" },
    }, commands));
    const identity = await controller.connectAndSync("ble");
    assert.ok(identity);
    assert.equal(identity.serial, "010203040506");
    assert.equal(identity.slot, 0);
    assert.equal(controller.getState().phase, "ready");
    assert.equal(controller.getState().liveValues["speed-limit"].value, 25);
    assert.equal(controller.getState().unavailableValues.lamp, "not available");
    assert.equal(commands.filter((command) => command.endsWith("settings read")).length, 1);
    assert.ok(!commands.includes("probe"));
  });

  it("refuses settings responses without a verified serial and does not reconnect", async () => {
    (globalThis as any).window = globalThis;
    const controller = new SessionController();
    const commands: string[] = [];
    controller.attachTerminal(terminalFor({ identity: { serial: null, model_series: 0x23 }, values: {} }, commands));
    assert.equal(await controller.connectAndSync("ble"), null);
    assert.equal(controller.getState().phase, "ready");
    assert.match(controller.getState().failure!.error, /verified serial identity/);
    assert.equal(controller.getState().identity, null);
    assert.equal(commands.filter((command) => command.endsWith("settings read")).length, 1);
  });

  it("selects each modern family schema and preserves standard/cargo identity", async () => {
    (globalThis as any).window = globalThis;
    for (const [series, cargo, slug, model] of [
      [0x30, "00", "ep600", "DU-EP600"],
      [0x30, "00-CRG", "ep600", "DU-EP600-CRG"],
      [0x35, "01", "ep801", "DU-EP801"],
      [0x35, "01-CRG", "ep801", "DU-EP801-CRG"],
      [0x37, "00", "ep500", "DU-EP500"],
    ] as const) {
      const controller = new SessionController();
      const commands: string[] = [];
      controller.attachTerminal(terminalFor({
        identity: { slot: 13, cargo, serial: "010203040506", model_series: series, model_unit: 0, dcas_version: "4.7.1.0", rx_version: "4.7.1.0" },
        values: {}, unavailable: {},
      }, commands));
      const identity = await controller.connectAndSync("ble");
      assert.ok(identity);
      assert.equal(identity.modelSlug, slug);
      assert.equal(identity.model, model);
      assert.equal(identity.isCargo, cargo.endsWith("-CRG"));
      assert.ok(commands.includes(`settings schema --model '${slug}'`));
      assert.equal(controller.getState().schema.find((setting) => setting.name === "speed-limit")?.userMax, 45);
    }
  });

  it("authenticates a modern motor's first read only over Bluetooth, as later reads do", async () => {
    (globalThis as any).window = globalThis;
    for (const [transport, prefix, auth] of [["usb", "", ""], ["serial", "", ""], ["ble", "--ble-master=master ", "--ble-unit-auth "]] as const) {
      const controller = new SessionController();
      const reads: string[] = [];
      controller.attachTerminal({
        setPasskeyPrompt: () => {},
        status: () => ({ ready: true, busy: false, usbSelected: true, transport }),
        files: { list: () => [], bytes: () => new Uint8Array(), import: async () => {}, initialize: async () => {} },
        run: async (line: string) => {
          const print = (globalThis as any).estepsTerminalPrint;
          if (line === `connect ${transport}`) return { code: 0 };
          if (line === `${prefix}units`) {
            print(JSON.stringify({ units: [{ slot: 13, model_series: 0x35, model_unit: 0, model: null, model_candidates: [] }] }), "stdout");
            return { code: 0 };
          }
          if (line.startsWith("settings schema")) {
            print(fs.readFileSync(path.join(import.meta.dirname, "schemas", "ep801.json"), "utf8"), "stdout");
            return { code: 0 };
          }
          assert.match(line, /settings read$/);
          reads.push(line);
          print(JSON.stringify({
            identity: { slot: 13, cargo: "01", serial: "010203040506", model_series: 0x35, model_unit: 0, dcas_version: "4.7.1.0", rx_version: "4.7.1.0" },
            values: {}, unavailable: {},
          }), "stdout");
          return { code: 0 };
        },
      });
      assert.equal((await controller.connectAndSync(transport))?.modelSlug, "ep801");
      assert.deepEqual(reads, [`${prefix}--slot 13 ${auth}settings read`]);
    }
  });

  it("loads actual older motor schemas without EP8 speed or profile defaults", async () => {
    (globalThis as any).window = globalThis;
    for (const [series, slug, model, count] of [
      [0x38, "e5100", "DU-E5100", 14],
      [0x12, "e8000", "DU-E8080", 3],
      [0x21, "e7000", "DU-E7000", 3],
      [0x20, "e6100", "DU-E6110", 3],
      [0x20, "e6100", "DU-E6180", 3],
      [0x20, "e6100", "DU-E6100-CRG", 3],
      [0x20, "e6100", "DU-E6180-TRK", 3],
      [0x08, "e6000", "E6000 family", 3],
    ] as const) {
      const controller = new SessionController();
      const commands: string[] = [];
      controller.attachTerminal(terminalFor({
        identity: { slot: 13, model_series: series, model_unit: 0, model_name: model, settings_model: slug, serial: "010203040506", cargo: "binary attributes" },
        values: {}, unavailable: {},
      }, commands));
      const identity = await controller.connectAndSync("ble");
      assert.ok(identity);
      assert.equal(identity.modelSlug, slug);
      assert.equal(identity.model, model);
      assert.equal(identity.isCargo, /-(CRG|TRK)$/.test(model));
      assert.ok(commands.includes(`settings schema --model '${slug}'`));
      const schema = controller.getState().schema;
      assert.equal(schema.length, count);
      assert.equal(schema.some((setting) => setting.section === "profiles"), false);
      assert.equal(schema.some((setting) => setting.name === "speed-limit"), slug === "e5100");
      if (slug === "e5100") {
        assert.equal(schema.find((setting) => setting.name === "speed-limit")?.userMax, 32);
        assert.deepEqual(Object.keys(schema.find((setting) => setting.name === "destination")!.choiceLabels!), ["0", "1", "3", "4"]);
      }
    }
  });
});

describe("Actual motor identity classification", () => {
  it("uses safe family labels without guessing a shared-series variant", () => {
    for (const [series, slug, name] of [[0x08, "e6000", "E6000 family"], [0x12, "e8000", "E8000 family"], [0x20, "e6100", "E6100 family"]] as const) {
      assert.deepEqual(detectMotorIdentity({ model_series: series }), { modelSlug: slug, model: name, isCargo: false });
    }
  });

  it("rejects unknown series, non-motor units, and inconsistent backend schema metadata", () => {
    assert.throws(() => detectMotorIdentity({ model_series: 0xff }), /not supported/);
    assert.throws(() => detectMotorIdentity({ model_series: 0x12, model_unit: 1 }), /not supported/);
    assert.throws(() => detectMotorIdentity({ model_series: 0x12, settings_model: "ep801" }), /do not match/);
  });

  it("keeps cargo variants in the friendly model label", () => {
    assert.equal(friendlyMotorName({ modelSlug: "ep801", model: "DU-EP801-CRG" } as any), "Shimano DU-EP801-CRG");
    assert.equal(friendlyMotorName({ modelSlug: "ep600", model: "DU-EP600-CRG" } as any), "Shimano DU-EP600-CRG");
  });
});

describe("Speed and destination staging", () => {
  function connected(model: "ep800-crg" | "ep600" | "ep801" | "ep500", destination: number, speed: number) {
    const controller = new SessionController();
    (controller as any).update({
      phase: "ready", transport: "sim",
      identity: { slot: 13, modelSlug: model, model: model === "ep800-crg" ? "DU-EP800-CRG" : `DU-${model.toUpperCase()}`, isMotor: true, modelUnit: 0, serial: "010203040506" },
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
    for (const model of ["ep600", "ep801", "ep500"] as const) {
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
    assert.match(controller.getState().failure!.error, /No supported destination/);
  });

  it("rejects another save while a transaction is committing", async () => {
    const controller = connected("ep800-crg", 0, 25);
    (controller as any).update({ phase: "committing" });
    await assert.rejects(controller.commitStagedBatch(), /save is already running/);
    assert.equal(controller.getState().phase, "committing");
  });

  it("writes destination before speed and keeps a failed save in settings", async () => {
    (globalThis as any).window = globalThis;
    const controller = connected("ep800-crg", 0, 25);
    let operations: any[] = [];
    let fail = false;
    const files = new Map<string, Uint8Array>();
    controller.attachTerminal({
      setPasskeyPrompt: () => {},
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
        (globalThis as any).estepsTerminalPrint(JSON.stringify({ completed: operations.length, results: operations.map((op) => ({ setting: op.name, after: Number(op.value) })) }), "stdout");
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
    assert.equal(controller.getState().failure?.error, "write rejected");
    assert.deepEqual(controller.getState().stagedValues, { "speed-limit": 31 });
  });
});

describe("Wire-Level GATT Simulator Register Completeness", () => {
  it("returns exact standard and cargo product attributes for authentication", () => {
    for (const [model, attributes] of [
      ["ep800-crg", "00-CRG\0"], ["ep600", "00\0\0\0\0\0"], ["ep801", "01\0\0\0\0\0"],
    ] as const) {
      const sim = new DeviceSimulator({ model });
      const replies: number[][] = [];
      const characteristic = sim.chars.find((entry: any) => entry.id === 0x2afd)!;
      characteristic.addEventListener("characteristicvaluechanged", () => {
        replies.push(Array.from(new Uint8Array(characteristic.value!.buffer)));
      });
      sim.write(0x2afe, [13, 0x16, 0x7c, 0]);
      assert.deepEqual(replies.at(-1), [13, 0x16, 0x7e, ...new TextEncoder().encode(attributes)]);
    }
  });

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

describe("Connect screen transport choice", () => {
  it("offers Web Serial first for a wired connection on Windows, as connect auto does", () => {
    const linux = {
      isIOS: false, isBluefy: false, isWindows: false, isMacOS: false, isAndroid: false, isLinux: true,
      supportsSerial: true, supportsBluetooth: true, supportsUsb: true,
    };
    const windows = { ...linux, isLinux: false, isWindows: true };
    assert.equal(preferredTransport(windows, true), "ble");
    assert.equal(preferredTransport(windows, false), "serial");
    assert.equal(preferredTransport({ ...windows, supportsSerial: false }, false), "usb");
    assert.equal(preferredTransport(linux, false), "usb");
    assert.equal(preferredTransport({ ...linux, supportsUsb: false }, false), "serial");
    assert.equal(preferredTransport({ ...linux, supportsUsb: false, supportsSerial: false }, false), null);
  });
});

describe("Web Bluetooth Availability & Globally Disabled Error Handling", () => {
  const origNav = globalThis.navigator;

  it("checks Bluetooth availability properly via getAvailability", async () => {
    const { checkBluetoothAvailability } = await import("./src/core/platform.ts");

    // Bluetooth disabled globally or adapter off
    Object.defineProperty(globalThis, "navigator", {
      value: {
        bluetooth: {
          getAvailability: async () => false,
        },
      },
      configurable: true,
    });
    assert.equal(await checkBluetoothAvailability(), false);

    // Bluetooth available
    Object.defineProperty(globalThis, "navigator", {
      value: {
        bluetooth: {
          getAvailability: async () => true,
        },
      },
      configurable: true,
    });
    assert.equal(await checkBluetoothAvailability(), true);

    // No bluetooth property
    Object.defineProperty(globalThis, "navigator", {
      value: {},
      configurable: true,
    });
    assert.equal(await checkBluetoothAvailability(), false);

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
      /Web Bluetooth is disabled globally in this browser\. Use Google Chrome or Microsoft Edge to connect wirelessly, or check your browser's Bluetooth settings\./,
    );
  });

  it("userFacingError translates globally disabled error into clear guidance", () => {
    const message = userFacingError("error: Web Bluetooth API globally disabled.\n[exit 1]");
    assert.match(message, /Web Bluetooth is disabled globally in this browser/);
    assert.match(message, /Google Chrome or Microsoft Edge/);
  });
});

describe("Concise connection errors", () => {
  it("keeps known connection failures actionable without backend details", () => {
    assert.match(userFacingError("error: BLE authentication failed: rejected\n[exit 1]"), /PIN was not accepted/);
    const cleanup = userFacingError("error: cleanup also failed: BLE PC-link stop was not confirmed: BLE response timeout or notification overflow; reconnect before another command\n[exit 1]");
    assert.equal(cleanup, "The Bluetooth link closed before the bike confirmed it was released. Turn on the bike’s connection mode, then reconnect to read its current settings.");
    assert.doesNotMatch(cleanup, /overflow|PC-link|exit/);
    assert.equal(userFacingError("error: Invalid value\n    at host_call (host.js:1)\n[exit 1]"), "Invalid value");
  });

  it("maps the core's motor support and authorization refusals", () => {
    assert.match(userFacingError("error: settings/authentication is unsupported for this motor identity"), /aren’t supported for this motor identity/);
    assert.match(userFacingError("error: protected writes require an unambiguous E6000-family model identity on this firmware"), /isn’t supported on this motor firmware/);
    assert.equal(
      describeFailure({ kind: "save", error: "error: settings/authentication is unsupported for this motor identity" }, "ble", []).outcome,
      "No change was written. Your staged values are kept.",
    );
    for (const error of [
      "error: modern drive-unit authentication challenge rejected, malformed or out of order; no retry",
      "error: drive-unit authentication reply rejected, misrouted or malformed; no retry",
      "error: modern authentication challenge rejected or malformed; no retry",
    ]) {
      assert.match(userFacingError(error), /rejected authorization/, error);
      assert.equal(describeFailure({ kind: "save", error }, "ble", []).outcome, "No change was written. Your staged values are kept.");
    }
    // Refusals that mention the handshake are not the motor rejecting it.
    assert.match(userFacingError("error: drive-unit authentication motor serial mismatch"), /serial doesn’t match/);
    for (const error of [
      "error: drive-unit authentication already attempted; no automatic retry",
      "error: setting requires drive-unit authentication, but --no-auth was specified",
      "error: EP800 OEM destination is unset (255); factory destination GET selector 0 is unsupported by the verified legacy firmware. Set an explicit destination using drive-unit authentication before changing speed or maximum power",
    ]) {
      assert.doesNotMatch(userFacingError(error), /rejected authorization/, error);
    }
  });

  it("treats an unplugged USB or serial adapter as a lost link", () => {
    for (const error of [
      "error: USB device disconnected; device outcome may be uncertain",
      "error: Serial device disconnected",
      "error: Serial port is closed",
      "USB disconnected; preserve the journal before recovery.",
      "error: cleanup also failed: PC-link Stop acknowledgement: USB device disconnected; device outcome may be uncertain: read settings: USB device disconnected",
    ]) {
      assert.match(userFacingError(error, "usb"), /USB adapter disconnected/);
      assert.equal(describeFailure({ kind: "read", error }, "usb", []).linkLost, true);
    }
    assert.doesNotMatch(userFacingError("error: cleanup also failed: PC-link stop could not be confirmed", "usb"), /Bluetooth/);
  });

  it("treats a USB or serial port closed after an uncertain transfer as a lost link", () => {
    // Every later open fails with "cannot reopen"; only a reconnect makes a new port.
    for (const error of [
      "error: PC-link Stop acknowledgement: USB write timed out after 500 ms; device outcome uncertain",
      "error: USB write incomplete (3/8); device outcome uncertain",
      "error: USB write failed: NetworkError: A transfer error has occurred.; device outcome uncertain",
      "error: Serial write timed out after 500 ms; device outcome uncertain",
      "error: Serial write returned an incomplete or unsupported receipt; device outcome uncertain",
      "error: Serial session changed during write; device outcome uncertain",
      "error: read settings: FTDI UART line error 0x8",
      "error: read settings: USB receive queue exceeded 1 MiB",
      "error: read settings: USB receive stall",
      "error: Serial receive queue exceeded 1 MiB",
      "error: read settings: USB receive failed: Failed to execute 'transferIn' on 'USBDevice': A transfer error has occurred.",
      "error: read settings: Serial receive failed: Framing error",
      "error: USB session cannot reopen after an uncertain operation: USB write timed out after 500 ms; device outcome uncertain. Reconcile the motor and select a new USB session.",
      "error: Serial session cannot reopen after an uncertain operation: Serial close timed out after 2000 ms; session outcome uncertain. Reconcile the motor and select a new serial session.",
    ]) {
      assert.match(userFacingError(error, "usb"), /Reconnect/, error);
      assert.equal(describeFailure({ kind: "read", error }, "usb", []).linkLost, true, error);
    }
    // The core's own uncertain SET leaves the port usable.
    const batch = "error: batch stopped at operation 1; 0 completed operations remain applied; inspect tx_1.journal.jsonl: setting write failed; outcome may be uncertain, inspect backup and read device";
    assert.equal(describeFailure({ kind: "save", error: batch, settings: ["lamp"] }, "usb", []).linkLost, false);
    assert.equal(describeFailure({ kind: "save", error: "error: A USB write is already pending" }, "usb", []).linkLost, false);
    const flash = describeFailure({ kind: "firmware", error: "DO NOT POWER CYCLE THE MOTOR.\nerror: DCAS block 12 write: serial write; device outcome may be uncertain: USB write timed out after 500 ms; device outcome uncertain" }, "usb", []);
    assert.equal(flash.critical, true);
    assert.equal(flash.linkLost, false);
    assert.match(flash.message, /DO NOT POWER CYCLE/);
  });

  it("names the port when the browser rejects a USB or serial receive", async () => {
    const usb: any = new WebUsbPort({
      transferIn: async () => { throw new DOMException("Failed to execute 'transferIn' on 'USBDevice': A transfer error has occurred.", "NetworkError"); },
      close: async () => {},
    });
    Object.assign(usb, { opened: true, input: { endpointNumber: 1, packetSize: 64 } });
    await usb.pump(usb.generation);
    const serial: any = new WebSerialPort({ getInfo: () => ({ usbVendorId: 0x1e44, usbProductId: 0x7151 }), close: async () => {} });
    const reader = { read: async () => { throw new DOMException("Framing error", "FramingError"); }, cancel: async () => {}, releaseLock() {} };
    Object.assign(serial, { opened: true, resources: { reader, writer: null, pump: null } });
    await serial.pump(serial.resources, serial.generation);
    for (const port of [usb, serial]) {
      assert.equal(port.poisoned, port.error);
      const error = `error: read settings: ${port.error.message}`;
      assert.equal(describeFailure({ kind: "read", error }, "usb", []).linkLost, true, error);
    }
  });

  it("keeps the CLI's bootloader steps when a wired check loses the port", () => {
    const guidance = [
      "The application did not return after the DCAS dry run; nothing was erased or written.",
      "The motor may remain in its DCAS bootloader. Check whether its application answers with:",
      "  cranklink probe",
      "Only if it does not, return it to its application with:",
      "  cranklink --slot 13 firmware probe --target dcas --recovery --reset",
    ].join("\n");
    for (const cause of ["FTDI UART line error 0x10", "USB write timed out after 500 ms; device outcome uncertain", "USB device disconnected; device outcome may be uncertain"]) {
      const error = `${guidance}\nerror: application did not return after DCAS dry run: bootloader reset not acknowledged: ${cause}`;
      const failure = describeFailure({ kind: "firmware", error }, "usb", []);
      assert.match(failure.message, /may remain in its DCAS bootloader/, cause);
      assert.match(failure.message, /cranklink probe\n[^]*--recovery --reset/, cause);
      assert.equal(failure.linkLost, true, cause);
      assert.equal(failure.critical, false, cause);
    }
  });

  it("keeps the interrupted-flash warning and recovery command", () => {
    const error = [
      "CRITICAL: FLASH WRITE INTERRUPTED. Motor may be in an unbootable recovery state.",
      "DO NOT POWER CYCLE THE MOTOR.",
      "To recover, re-run with: cranklink -p usb:0 firmware flash fw.dat --serial 1234 --recover",
      "error: DCAS block 12 write: serial write; device outcome may be uncertain: USB device disconnected; device outcome may be uncertain",
    ].join("\n");
    const failure = describeFailure({ kind: "firmware", error }, "usb", []);
    assert.match(failure.message, /DO NOT POWER CYCLE/);
    assert.match(failure.message, /re-run with: cranklink/);
    assert.match(failure.outcome ?? "", /recovery mode/);
    assert.equal(failure.linkLost, false);
  });

  it("shows an interrupted flash without its progress lines and offers no next step", () => {
    const rule = "=".repeat(80);
    const error = [
      "Programming 8192/65536 bytes (12%)",
      "Programming 16384/65536 bytes (25%)",
      "",
      rule,
      "CRITICAL: FLASH WRITE INTERRUPTED. Motor may be in an unbootable recovery state.",
      "DO NOT POWER CYCLE THE MOTOR.",
      "Journal preserved at: fw_write_1.01-dcas.journal.jsonl",
      "To recover, re-run with:",
      "  cranklink -s 010203040506 firmware flash dcas D.dat --companion M.dat --recovery --write",
      rule,
      "",
      "",
      "error: DCAS block 12 write: USB device disconnected; device outcome may be uncertain",
    ].join("\n");
    const failure = describeFailure({ kind: "firmware", error }, "usb", []);
    assert.equal(failure.message.split("\n")[0], "CRITICAL: FLASH WRITE INTERRUPTED. Motor may be in an unbootable recovery state.");
    assert.doesNotMatch(failure.message, /Programming|===|\n\n\n/);
    assert.match(failure.message, /--recovery --write\n\nerror: DCAS block 12 write/);
    assert.equal(failure.critical, true);
    assert.equal(failure.linkLost, false);

    assert.match(failure.outcome ?? "", /Do not power-cycle it\. The web app cannot recover it: .*SM-PCE02.*cranklink CLI, using the command below/);

    const verify = describeFailure({ kind: "firmware", error: "Programming 65536/65536 bytes (100%)\nerror: verify mismatch" }, "usb", []);
    assert.equal(verify.message, "verify mismatch");
    assert.equal(verify.critical, false);
  });

  it("treats a Bluetooth flash that failed after the write started as critical", () => {
    // Without the CLI's warning (e.g. the worker died first), a started write still counts.
    const error = "Programming 8192/65536 bytes (12%)\nProgramming 16384/65536 bytes (25%)\nerror: Bluetooth disconnected\n[exit 1]";
    for (const transport of ["ble", "sim"] as const) {
      const failure = describeFailure({ kind: "firmware", error }, transport, []);
      assert.equal(failure.critical, true);
      assert.equal(failure.linkLost, false);
      assert.match(failure.outcome ?? "", /Do not power-cycle it\..*cranklink CLI\.$/);
      assert.doesNotMatch(failure.outcome ?? "", /command below/);
      assert.equal(failure.message, "Bluetooth disconnected");
    }
    // Before the write started, or outside a flash, a closed link stays a closed link.
    const check = describeFailure({ kind: "firmware", error: "error: Bluetooth disconnected" }, "ble", []);
    assert.equal(check.critical, false);
    assert.equal(check.linkLost, true);
    assert.equal(describeFailure({ kind: "read", error }, "ble", []).critical, false);
  });

  it("keeps a lost link lost when a later, unrelated operation is rejected", () => {
    const controller = new SessionController();
    (controller as any).update({ phase: "ready", linkLost: true });
    assert.throws(() => controller.stageParameter("lamp", 1));
    assert.equal(controller.getState().linkLost, true);
  });

  it("prioritizes uncertain cleanup over PIN and motor authorization failures", () => {
    for (const original of ["BLE authentication failed: rejected", "drive-unit authentication challenge rejected"]) {
      const message = userFacingError(`error: ${original}\nThis cleanup also failed: BLE PC-link stop was not confirmed\n    at host_call (host.js:1)\n[exit 1]`);
      assert.match(message, /closed before the bike confirmed/);
      assert.doesNotMatch(message, /PIN|authorization|host_call|exit/);
    }
  });
});

describe("Disconnect cleanup reporting", () => {
  function connected(run: () => Promise<{ code: number }>) {
    (globalThis as any).window = globalThis;
    const controller = new SessionController();
    controller.attachTerminal({
      run,
      setPasskeyPrompt: () => {},
      status: () => ({ ready: true, busy: false, usbSelected: false, transport: "ble" }),
      files: { list: () => [], bytes: () => new Uint8Array(), initialize: async () => {}, import: async () => {} },
    });
    (controller as any).update({ phase: "ready", transport: "ble", identity: { serial: "010203040506" }, stagedValues: { "tire-circumference": 2010 } });
    return controller;
  }

  function assertCleared(controller: SessionController) {
    assert.equal(controller.getState().phase, "disconnected");
    assert.equal(controller.getState().identity, null);
    assert.equal(controller.getState().transport, null);
    assert.deepEqual(controller.getState().stagedValues, {});
  }

  it("keeps a thrown cleanup failure visible after clearing the disconnected identity", async () => {
    const controller = connected(async () => { throw new Error("Bluetooth cleanup failed; check the bike."); });
    await assert.rejects(controller.disconnect(), /cleanup failed/);
    assertCleared(controller);
    assert.match(controller.getState().failure!.error, /cleanup failed/);
    assert.match(controller.getState().statusMessage, /cleanup was not confirmed/);
  });

  it("treats a nonzero terminal cleanup result as failure", async () => {
    const controller = connected(async () => {
      (globalThis as any).estepsTerminalPrint("BLE PC-link stop was not confirmed", "stderr");
      return { code: 1 };
    });
    await assert.rejects(controller.disconnect(), /stop was not confirmed/);
    assertCleared(controller);
    assert.match(controller.getState().failure!.error, /stop was not confirmed/);
  });

  it("reports a confirmed disconnect without a stale error", async () => {
    const controller = connected(async () => ({ code: 0 }));
    (controller as any).update({ failure: { kind: "read", error: "old failure" } });
    await controller.disconnect();
    assertCleared(controller);
    assert.equal(controller.getState().failure, null);
    assert.equal(controller.getState().statusMessage, "Disconnected.");
  });
});

describe("Adapter unplugged between commands", () => {
  function connected(transport: "usb" | "serial" | "ble", run: (line: string) => Promise<{ code: number }> = async () => ({ code: 0 })) {
    (globalThis as any).window = globalThis;
    let unplugged: ((error: string) => void) | null = null;
    const controller = new SessionController();
    controller.attachTerminal({
      run,
      setPasskeyPrompt: () => {},
      setAdapterDisconnectHandler: (handler) => { unplugged = handler; },
      status: () => ({ ready: true, busy: false, usbSelected: true, transport }),
      files: { list: () => [], bytes: () => new Uint8Array(), initialize: async () => {}, import: async () => {} },
    });
    (controller as any).update({ phase: "ready", transport, identity: { serial: "010203040506" }, stagedValues: { "tire-circumference": 2010 } });
    return { controller, unplug: (error: string) => unplugged!(error) };
  }

  it("marks the link lost at once and keeps the staged edits", () => {
    for (const [transport, error] of [["usb", "USB device disconnected"], ["serial", "Serial device disconnected"]] as const) {
      const { controller, unplug } = connected(transport);
      unplug(error);
      const state = controller.getState();
      assert.equal(state.linkLost, true);
      assert.deepEqual(state.stagedValues, { "tire-circumference": 2010 });
      const failure = describeFailure(state.failure!, state.transport, []);
      assert.equal(failure.title, "USB adapter disconnected");
      assert.match(failure.message, /USB adapter disconnected.*reconnect/i);
      assert.equal(failure.linkLost, true);
    }
  });

  it("leaves a running command to report the unplug through its own error", async () => {
    let unplug: (error: string) => void = () => {};
    let duringCommand: { linkLost: boolean; failure: unknown } | null = null;
    const session = connected("usb", async () => {
      unplug("USB device disconnected");
      duringCommand = { linkLost: session.controller.getState().linkLost, failure: session.controller.getState().failure };
      return { code: 0 };
    });
    unplug = session.unplug;
    await session.controller.runRawCommand("probe");
    assert.deepEqual(duringCommand, { linkLost: false, failure: null });
    (session.controller as any).update({ phase: "committing" });
    session.unplug("USB device disconnected");
    assert.equal(session.controller.getState().linkLost, false);
  });

  it("ignores a wired unplug while a Bluetooth session is selected", () => {
    const { controller, unplug } = connected("ble");
    unplug("USB device disconnected");
    assert.equal(controller.getState().linkLost, false);
    assert.equal(controller.getState().failure, null);
  });
});

describe("Failure outcomes", () => {
  const schema =enrichSchema(JSON.parse(fs.readFileSync(path.join(import.meta.dirname, "schemas", "ep800-crg.json"), "utf8")), "ep800-crg");
  const batchError = "error: cleanup also failed: BLE PC-link stop was not confirmed: BLE session failed; PC-link stop could not be confirmed; disconnect and inspect bike state: batch stopped at operation 2; 1 completed operations remain applied; inspect tx_1.journal.jsonl: setting write failed; outcome may be uncertain, inspect backup and read device\n[exit 1]";

  it("leads a failed save with what reached the bike", () => {
    const failure = describeFailure({ kind: "save", error: batchError, settings: ["destination", "speed-limit"] }, "ble", schema);
    assert.equal(failure.title, "Save stopped");
    assert.equal(failure.outcome, "1 of 2 changes were applied before the error. Assist Speed Limit may or may not have been written.");
    assert.equal(failure.linkLost, true);
  });

  it("names the simulated link in simulator sessions", () => {
    const single = describeFailure({ kind: "save", error: batchError.replace("operation 2; 1 completed", "operation 1; 0 completed"), settings: ["lamp"] }, "sim", schema);
    assert.match(single.message, /simulated Bluetooth link/);
    assert.equal(single.outcome, "Wired Light Port may or may not have been written.");
  });
});

describe("Setting value formatting", () => {
  const schema = enrichSchema(JSON.parse(fs.readFileSync(path.join(import.meta.dirname, "schemas", "ep800-crg.json"), "utf8")), "ep800-crg");
  it("uses labels and units instead of raw values", () => {
    assert.equal(formatSetting(schema, "lamp", 0), "Off");
    assert.equal(formatSetting(schema, "speed-limit", 25), "25 km/h");
    assert.equal(formatSetting(schema, "display-speed-adjustment", 15), "1.5%");
    assert.equal(formatSetting(schema, "mounting-angle", 12), "12°");
    assert.equal(formatSetting(schema, "destination", 1), "Region 1 / US Class 1 (32 km/h)");
    assert.equal(formatSetting(schema, "start-mode", 0), "Off");
    assert.equal(formatSetting(schema, "speed-limit", null), "—");
  });

  it("asks for the regulated-change acknowledgment for every oem setting", () => {
    for (const file of fs.readdirSync(path.join(import.meta.dirname, "schemas"))) {
      const model = path.basename(file, ".json") as SettingsModel;
      const specs = enrichSchema(JSON.parse(fs.readFileSync(path.join(import.meta.dirname, "schemas", file), "utf8")), model);
      for (const spec of specs.filter((spec) => spec.oem)) {
        assert.equal(isRegulatedSetting(spec, spec.name), true, `${model} ${spec.name}`);
      }
    }
    for (const name of ["tire-circumference", "walk-assist", "rear-teeth"]) {
      assert.notEqual(schema.find((spec) => spec.name === name)?.section, "oem", name);
    }
    for (const name of ["lamp", "speed-limit"]) {
      assert.equal(isRegulatedSetting(schema.find((spec) => spec.name === name), name), false, name);
    }
  });
});
