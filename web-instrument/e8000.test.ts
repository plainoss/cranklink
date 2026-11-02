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
  let circumference = 2100;
  const identity = { slot: 13, model_series: 0x12, model_unit: 0, model_name: "DU-E8000", settings_model: "e8000", dcas_version: "4.6.0.0", rx_version: "4.6.0.0", serial: "010203040506" };
  const snapshot = () => ({ identity, values: { destination: { value: region, raw: `01${region === 0 ? "00" : "01"}ffff` }, "tire-circumference": { value: circumference, raw: "0000" } }, unavailable: {} });
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
          const name = operation.op === "restore" ? JSON.parse(new TextDecoder().decode(files.get(operation.file))).setting : operation.name;
          assert.ok(["destination", "tire-circumference"].includes(name));
          const before = name === "destination" ? region : circumference;
          const after = operation.op === "restore" ? JSON.parse(new TextDecoder().decode(files.get(operation.file))).before : Number(operation.value);
          if (name === "destination") region = after; else circumference = after;
          return { setting: name, before, after };
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

  it("allows E8000 region and wheel saves/restores while retaining normal routing automatically", async () => {
    for (const name of ["destination", "tire-circumference"]) {
      const h = connected("ble", true);
      const value = name === "destination" ? 1 : 2200;
      h.controller.stageParameter(name, value);
      await h.controller.commitStagedBatch();
      assert.equal(h.controller.getState().liveValues[name].value, value);
      h.files.set("before.json", new TextEncoder().encode(JSON.stringify({ format: "cranklink-setting-backup-v1", settings_model: "e8000", serial: "010203040506", setting: name, before: name === "destination" ? 0 : 2100 })));
      await h.controller.restoreBackupFiles(["before.json"]);
      assert.equal(h.controller.getState().liveValues[name].value, name === "destination" ? 0 : 2100);
      assert.ok(h.commands.every((command) => command.includes("--ble-master=normal")));
      assert.ok(h.commands.every((command) => !command.includes("--ble-master=master")));
      assert.ok(h.commands.filter((command) => command.includes("settings batch")).every((command) => command.includes("--backup-prefix")));
    }
  });

  it("refuses master-required settings on normal routing with friendly USB PCE message", () => {
    const h = connected("ble", true);
    assert.throws(() => (h.controller as any).requireWriteRouting("save", ["destination", "lamp"]),
      /requires a USB connection on this bike/);
    assert.throws(() => (h.controller as any).requireWriteRouting("restore", ["lighting-time"]),
      /requires a USB connection on this bike/);
    (h.controller as any).update({ identity: { ...h.controller.getState().identity!, modelSlug: "ep800-crg" } });
    assert.throws(() => (h.controller as any).requireWriteRouting("save", ["destination"]),
      /requires a USB connection on this bike/);
    assert.throws(() => (h.controller as any).requireWriteRouting("save", ["tire-circumference"]),
      /requires a USB connection on this bike/);

    // e8000-pre41 permits tire-circumference on normal routing, but blocks lamp
    (h.controller as any).update({ identity: { ...h.controller.getState().identity!, modelSlug: "e8000-pre41" } });
    assert.doesNotThrow(() => (h.controller as any).requireWriteRouting("save", ["tire-circumference"]));
    assert.throws(() => (h.controller as any).requireWriteRouting("save", ["lamp"]),
      /requires a USB connection on this bike/);
  });

  it("selects normal discovery and reads automatically when an E8000 gateway is detected", async () => {
    (globalThis as any).window = globalThis;
    const controller = new SessionController();
    const commands: string[] = [];
    const identity = { slot: 0, model_series: 0x12, model_unit: 0, model_name: "DU-E8000", settings_model: "e8000", dcas_version: "4.6.0.0", rx_version: "4.6.0.0", serial: "010203040506" };
    controller.attachTerminal({
      setPasskeyPrompt() {},
      status: () => ({ ready: true, busy: false, usbSelected: false, transport: "ble" }),
      files: { list: () => [], bytes: () => new Uint8Array(), import: async () => {}, initialize: async () => {} },
      run: async (line) => {
        commands.push(line);
        const print = (data: unknown) => (globalThis as any).estepsTerminalPrint(JSON.stringify(data), "stdout");
        if (line === "connect ble") return { code: 0 };
        if (line === "ble detect-system") print({ system: "steps", master_series: 0x12, service_started: false });
        else if (line === "--ble-master=normal units") print({ units: [{ slot: 0, model_series: 0x12, model_unit: 0, model: "DU-E8000", model_candidates: ["DU-E8000"] }] });
        else if (line.startsWith("settings schema")) print(rawSchema("e8000"));
        else {
          assert.match(line, /^--ble-master=normal --slot 0 --component 'DU-E8000' settings read$/);
          print({ identity, values: { destination: { value: 0, raw: "0100ffff" } }, unavailable: {} });
        }
        return { code: 0 };
      },
    });
    const selected = await controller.connectAndSync("ble");
    assert.ok(selected, JSON.stringify(controller.getState().failure));
    assert.equal(selected.modelSlug, "e8000");
    assert.equal(controller.getState().phase, "ready");
    assert.ok(commands.includes("--ble-master=normal units"));
    assert.ok(commands.some((line) => line.endsWith("settings read")));
    assert.ok(commands.every((line) => !line.includes("--ble-master=master")));
  });

  it("exposes legacy assist tuning controls for DU-E8000 and DU-E6100", () => {
    const e8000Schema = enrichSchema(rawSchema("e8000"), "e8000");
    const pattern8000 = e8000Schema.find((s) => s.name === "assist-pattern")!;
    assert.ok(pattern8000);
    assert.equal(pattern8000.label, "Assist Mode Preset");
    assert.deepEqual(pattern8000.choiceLabels, { 2: "Dynamic", 3: "Explorer", 4: "Custom" });

    const boost = e8000Schema.find((s) => s.name === "boost-level")!;
    assert.ok(boost);
    assert.equal(boost.label, "Boost Assist Level");
    assert.deepEqual(boost.choiceLabels, { 0: "Low", 1: "Medium", 2: "High" });

    const trail = e8000Schema.find((s) => s.name === "trail-level")!;
    assert.ok(trail);
    assert.equal(trail.label, "Trail Assist Level");
    assert.deepEqual(trail.choiceLabels, { 0: "Low", 1: "Medium", 2: "High" });

    const e6100Raw = JSON.parse(fs.readFileSync(new URL("./schemas/e6100.json", import.meta.url), "utf8"));
    const e6100Schema = enrichSchema(e6100Raw, "e6100");
    const pattern6100 = e6100Schema.find((s) => s.name === "assist-pattern")!;
    assert.ok(pattern6100);
    assert.equal(pattern6100.label, "Assist Pattern");
    assert.deepEqual(pattern6100.choiceLabels, { 0: "Comfort", 1: "Sportive" });
    assert.equal(e6100Schema.some((s) => s.name === "boost-level"), false);
  });

  it("coordinates legacy assist presets and boost/trail levels in session controller", () => {
    const h = connected("usb");
    const state = h.controller.getState();
    (h.controller as any).update({
      liveValues: {
        ...state.liveValues,
        "assist-pattern": { value: 2, raw: "02" },
        "boost-level": { value: 2, raw: "02" },
        "trail-level": { value: 0, raw: "00" },
      },
    });

    // Staging Explorer (3) stages pattern=3, boost=1 (Med), trail=1 (Med)
    h.controller.stageParameter("assist-pattern", 3);
    assert.deepEqual(h.controller.getState().stagedValues, {
      "assist-pattern": 3,
      "boost-level": 1,
      "trail-level": 1,
    });

    // Changing Boost level to 0 auto-stages Custom (4) alongside Boost=0
    h.controller.stageParameter("boost-level", 0);
    assert.equal(h.controller.getState().stagedValues["boost-level"], 0);
    assert.equal(h.controller.getState().stagedValues["trail-level"], 1);
    assert.equal(h.controller.getState().stagedValues["assist-pattern"], 4);

    // Staging Dynamic (2) re-sets Boost=2 and Trail=0; pattern=2 matches live so pattern un-stages
    h.controller.stageParameter("assist-pattern", 2);
    assert.equal(h.controller.getState().stagedValues["boost-level"], undefined);
    assert.equal(h.controller.getState().stagedValues["trail-level"], undefined);
    assert.equal(h.controller.getState().stagedValues["assist-pattern"], undefined);
  });
});
