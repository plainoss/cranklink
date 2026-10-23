import { describe, it } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { SessionController } from "./src/core/sessionController.ts";
import { detectDeviceIdentity, detectMotorIdentity, isMotorStock } from "./src/domain/motorIdentity.ts";
import { E6002_DESTINATION_LABELS, destinationSpeedRange, enrichSchema } from "./src/domain/schema.ts";
import { formatSetting } from "./src/domain/settingFormat.ts";
import type { MotorModel } from "./src/types.ts";

const rawSchema = (model: MotorModel) => JSON.parse(fs.readFileSync(path.join(import.meta.dirname, "schemas", `${model}.json`), "utf8"));

describe("E600x motor settings support", () => {
  it("recognizes E6001-family motor stocks and preserves verified variant names", () => {
    for (const [series, name] of [[0x0a, "DU-E6001"], [0x0c, "DU-E6001"], [0x0c, "DU-E6010"], [0x0c, "DU-E6050"]] as const) {
      assert.equal(isMotorStock(series, 0), true);
      assert.equal(isMotorStock(series, 1), false);
      assert.deepEqual(detectDeviceIdentity({ model_series: series, model_unit: 0, model_name: name, settings_model: "e6001" }), {
        modelSlug: "e6001", model: name, isCargo: false, isMotor: true, modelUnit: 0,
      });
    }
    assert.equal(detectMotorIdentity({ model_series: 0x0c }).model, "E6001 family");
    assert.throws(() => detectMotorIdentity({ model_series: 0x0a, settings_model: "e6000" }), /do not match/);
  });

  it("uses the D3.3 boundary for E6002-family schemas and rejects mismatched metadata", () => {
    for (const [version, slug] of [["3.2.15.0", "e6002-pre33"], ["3.3.0.0", "e6002"], ["3.10.0.0", "e6002"], ["4.0.0.0", "e6002"]] as const) {
      for (const name of ["DU-E6002", "DU-E6012"]) {
        const identity = detectDeviceIdentity({ model_series: 0x15, model_unit: 0, model_name: name, settings_model: slug, dcas_version: version });
        assert.equal(identity.modelSlug, slug);
        assert.equal(identity.model, name);
        assert.equal(identity.isMotor, true);
      }
    }
    assert.throws(() => detectMotorIdentity({ model_series: 0x15 }), /valid drive-unit firmware/);
    assert.throws(() => detectMotorIdentity({ model_series: 0x15, dcas_version: "3.3" }), /valid drive-unit firmware/);
    assert.throws(() => detectMotorIdentity({ model_series: 0x15, dcas_version: "3.2.0.0", settings_model: "e6002" }), /do not match/);
    assert.throws(() => detectMotorIdentity({ model_series: 0x15, dcas_version: "3.3.0.0", settings_model: "e6002-pre33" }), /do not match/);
  });

  it("loads schemas through current component discovery and selected-target reads", async () => {
    (globalThis as any).window = globalThis;
    for (const [series, name, version, slug, count] of [
      [0x0a, "DU-E6001", "3.0.0.0", "e6001", 3],
      [0x0c, "DU-E6050", "3.0.0.0", "e6001", 3],
      [0x15, "DU-E6002", "3.2.0.0", "e6002-pre33", 3],
      [0x15, "DU-E6012", "3.3.0.0", "e6002", 4],
    ] as const) {
      const controller = new SessionController();
      controller.setBluetoothBike("steps"); // Historical master families require an explicit choice.
      const commands: string[] = [];
      controller.attachTerminal({
        setPasskeyPrompt: () => {},
        status: () => ({ ready: true, busy: false, usbSelected: false, transport: "ble" }),
        files: { list: () => [], bytes: () => new Uint8Array(), import: async () => {}, initialize: async () => {} },
        run: async (line) => {
          commands.push(line);
          const print = (globalThis as any).estepsTerminalPrint;
          if (line === "connect ble") return { code: 0 };
          if (line === "--ble-master=master units") {
            print(JSON.stringify({ units: [{ slot: 13, model_series: series, model_unit: 0, model: name, model_candidates: [] }] }), "stdout");
          } else if (line === `settings schema --model '${slug}'`) {
            print(JSON.stringify(rawSchema(slug)), "stdout");
          } else {
            assert.equal(line, `--ble-master=master --slot 13 --component '${name}' settings read`);
            print(JSON.stringify({
              identity: { slot: 13, model_series: series, model_unit: 0, model_name: name, settings_model: slug, dcas_version: version, serial: "010203040506" },
              values: {}, unavailable: {},
            }), "stdout");
          }
          return { code: 0 };
        },
      });
      const identity = await controller.connectAndSync("ble");
      assert.ok(identity, controller.getState().failure?.error ?? "Motor identity was not loaded");
      assert.equal(identity.modelSlug, slug);
      assert.equal(identity.model, name);
      assert.equal(identity.isMotor, true);
      assert.equal(controller.getState().schema.length, count);
      assert.ok(commands.includes(`settings schema --model '${slug}'`));
    }
  });

  it("formats E6002 markets consistently without claiming speed cutoffs", () => {
    const schema = enrichSchema(rawSchema("e6002"), "e6002");
    assert.deepEqual(schema.find((setting) => setting.name === "destination")!.choiceLabels, E6002_DESTINATION_LABELS);
    assert.doesNotMatch(schema.find((setting) => setting.name === "destination")!.description, /speed|cutoff/i);
    assert.deepEqual([0, 1, 4].map((value) => formatSetting(schema, "destination", value)), ["Europe (EU)", "United States (US)", "Korea"]);
    assert.doesNotMatch(Object.values(E6002_DESTINATION_LABELS).join(" "), /Australia|25|32|km\/h/);
    assert.equal(schema.some((setting) => setting.name === "speed-limit" || setting.section === "profiles"), false);
  });

  it("stages and reverts an E6002 region without creating a speed write", () => {
    const controller = new SessionController();
    (controller as any).update({
      phase: "ready", transport: "usb", identity: { slot: 13, modelSlug: "e6002", isMotor: true, modelUnit: 0, serial: "010203040506" },
      schema: enrichSchema(rawSchema("e6002"), "e6002"),
      liveValues: { destination: { value: 0, raw: "" }, "speed-limit": { value: 45, raw: "" } },
    });
    for (const destination of [1, 4]) {
      controller.stageParameter("destination", destination);
      assert.deepEqual(controller.getState().stagedValues, { destination });
      assert.equal(destinationSpeedRange("e6002", destination), undefined);
    }
    assert.throws(() => controller.stageParameter("destination", 3), /outside the permitted/);
    controller.revertParameter("destination");
    assert.deepEqual(controller.getState().stagedValues, {});
    assert.throws(() => controller.stageParameter("speed-limit", 25), /unavailable or read-only/);
  });

  it("keeps older E600x schemas to the three common hardware settings", () => {
    for (const model of ["e6001", "e6002-pre33"] as const) {
      const schema = enrichSchema(rawSchema(model), model);
      assert.equal(schema.length, 3);
      assert.equal(schema.some((setting) => ["destination", "speed-limit"].includes(setting.name) || setting.section === "profiles"), false);
    }
  });
});
