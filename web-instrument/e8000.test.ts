import { describe, it } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import { SessionController } from "./src/core/sessionController.ts";
import { detectMotorIdentity } from "./src/domain/motorIdentity.ts";
import { enrichSchema } from "./src/domain/schema.ts";

const rawSchema = (model: "e8000" | "e8000-pre41") => JSON.parse(fs.readFileSync(new URL(`./schemas/${model}.json`, import.meta.url), "utf8"));

function connected(transport: "usb" | "ble", normal = false) {
  (globalThis as any).window = globalThis;
  const controller = new SessionController();
  const files = new Map<string, Uint8Array>();
  const commands: string[] = [];
  let region = 0;
  const identity = { slot: 13, model_series: 0x12, model_unit: 0, model_name: "DU-E8000", settings_model: "e8000", dcas_version: "4.6.0.0", rx_version: "4.6.0.0", serial: "010203040506" };
  const snapshot = () => ({ identity, values: { destination: { value: region, raw: `01${region === 0 ? "00" : "01"}ffff` } }, unavailable: {} });
  controller.attachTerminal({
    setPasskeyPrompt() {},
    status: () => ({ ready: true, busy: false, usbSelected: transport === "usb", transport }),
    files: { list: () => [...files.keys()].map((path) => ({ path })), bytes: (path) => files.get(path)!,
      import: async (path, bytes) => { files.set(path, bytes); }, initialize: async () => {} },
    run: async (line) => {
      commands.push(line);
      const print = (data: unknown) => (globalThis as any).estepsTerminalPrint(JSON.stringify(data), "stdout");
      if (line.endsWith("settings read")) print(snapshot());
      else {
        const planPath = line.match(/settings batch '([^']+)'/)?.[1];
        assert.ok(planPath, line);
        const plan = JSON.parse(new TextDecoder().decode(files.get(planPath)));
        const results = plan.map((operation: any) => {
          const before = region;
          if (operation.op === "restore") region = JSON.parse(new TextDecoder().decode(files.get(operation.file))).before;
          else { assert.equal(operation.name, "destination"); region = Number(operation.value); }
          return { setting: "destination", before, after: region };
        });
        print({ completed: results.length, results });
      }
      return { code: 0 };
    },
  });
  (controller as any).update({ phase: "ready", transport, bluetoothSystem: "steps", bluetoothMasterSeries: normal ? 0x12 : null,
    identity: { ...detectMotorIdentity(identity), slot: 13, serial: identity.serial, isMotor: true, modelSeries: 0x12, modelUnit: 0, dcasVersion: "4.6.0.0", rxVersion: "4.6.0.0" },
    schema: enrichSchema(rawSchema("e8000"), "e8000"), liveValues: snapshot().values });
  return { controller, files, commands };
}

describe("E8000 region-based assist speed", () => {
  it("selects a firmware-bound schema for E8000 and E8080 without independent speed controls", () => {
    for (const name of ["DU-E8000", "DU-E8080"]) {
      for (const [version, slug] of [["4.0.99.0", "e8000-pre41"], ["4.1.0.0", "e8000"], ["4.6.0.0", "e8000"], ["4.10.0.0", "e8000"]] as const) {
        assert.equal(detectMotorIdentity({ model_series: 0x12, model_unit: 0, model_name: name, settings_model: slug, dcas_version: version }).modelSlug, slug);
        assert.equal(rawSchema(slug).some((spec: any) => spec.name === "destination"), slug === "e8000");
        assert.equal(rawSchema(slug).some((spec: any) => spec.name === "speed-limit"), false);
      }
    }
    assert.throws(() => detectMotorIdentity({ model_series: 0x12 }), /valid drive-unit firmware/);
    assert.throws(() => detectMotorIdentity({ model_series: 0x12, dcas_version: "4.0.0.0", settings_model: "e8000" }), /do not match/);
  });

  it("saves and restores the region alone over USB or a master session", async () => {
    for (const transport of ["usb", "ble"] as const) {
      const h = connected(transport);
      const region = h.controller.getState().schema.find((spec) => spec.name === "destination")!;
      assert.deepEqual(region.choiceLabels, { 0: "Europe · 25 km/h", 1: "United States · 32 km/h" });
      assert.equal(region.label, "Region & assist speed");
      h.controller.stageParameter("destination", 1);
      assert.deepEqual(h.controller.getState().stagedValues, { destination: 1 });
      await h.controller.commitStagedBatch();
      const plan = JSON.parse(new TextDecoder().decode([...h.files.values()][0]));
      assert.deepEqual(plan, [{ op: "set", name: "destination", value: "1" }]);
      assert.equal(h.controller.getState().liveValues.destination.value, 1);
      assert.ok(h.commands.every((command) => !command.includes("--ble-master=normal")));
      h.files.set("before.json", new TextEncoder().encode(JSON.stringify({ format: "cranklink-setting-backup-v1", settings_model: "e8000", serial: "010203040506", setting: "destination", before: 0 })));
      await h.controller.restoreBackupFiles(["before.json"]);
      assert.equal(h.controller.getState().liveValues.destination.value, 0);
    }
  });

  it("refuses master-required saves and restores on a normal retained connection before importing a plan", async () => {
    const h = connected("ble", true);
    h.controller.stageParameter("destination", 1);
    await assert.rejects(h.controller.commitStagedBatch(), /requires Bluetooth master routing.*No changes were sent/);
    assert.deepEqual(h.controller.getState().stagedValues, { destination: 1 });
    assert.equal(h.files.size, 0);
    h.controller.discardAllStaged();
    h.files.set("before.json", new TextEncoder().encode(JSON.stringify({ format: "cranklink-setting-backup-v1", settings_model: "e8000", serial: "010203040506", setting: "destination", before: 0 })));
    await assert.rejects(h.controller.restoreBackupFiles(["before.json"]), /requires Bluetooth master routing/);
    assert.equal(h.files.size, 1);
    assert.deepEqual(h.commands, []);
    assert.throws(() => h.controller.stageParameter("speed-limit", 25), /unavailable or read-only/);
    assert.throws(() => h.controller.stageParameter("destination", 5), /outside the permitted/);
  });
});
