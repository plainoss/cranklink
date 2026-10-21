import { describe, it } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";

import { enrichSchema, TIRE_PRESETS, settingAcceptsValue, clampSettingValue } from "./src/domain/schema.ts";
import {
  classifyFirmwareRole,
  evaluateCompanionPair,
  buildSynchronizedFlashPlan,
} from "./src/domain/firmwarePackage.ts";
import { describeFailure, userFacingError } from "./src/domain/errors.ts";
import { formatSetting, formatValue } from "./src/domain/settingFormat.ts";
import { detectMotorIdentity, friendlyMotorName } from "./src/domain/motorIdentity.ts";
import { SessionController } from "./src/core/sessionController.ts";
import { DeviceSimulator } from "./simulator.js";

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
    assert.match(userFacingError("error: drive-unit authentication is not verified for series 23 D4.1.9.0"), /isn’t supported/);
    const cleanup = userFacingError("error: cleanup also failed: BLE PC-link stop was not confirmed: BLE response timeout or notification overflow; reconnect before another command\n[exit 1]");
    assert.equal(cleanup, "The Bluetooth link closed before the bike confirmed it was released. Turn on the bike’s connection mode, then reconnect to read its current settings.");
    assert.doesNotMatch(cleanup, /overflow|PC-link|exit/);
    assert.equal(userFacingError("error: Invalid value\n    at host_call (host.js:1)\n[exit 1]"), "Invalid value");
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

describe("Failure outcomes", () => {
  const schema = enrichSchema(JSON.parse(fs.readFileSync(path.join(import.meta.dirname, "schemas", "ep800-crg.json"), "utf8")), "ep800-crg");
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
});
