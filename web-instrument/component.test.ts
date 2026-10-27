import { afterEach, beforeEach, describe, it } from "node:test";
import assert from "node:assert/strict";

import { SessionController } from "./src/core/sessionController.ts";
import type { TerminalHost } from "./src/core/sessionController.ts";
import { WebMCPServer } from "./src/core/webMcpServer.ts";
import type { ComponentUnit, SettingSpec } from "./src/types.ts";
import { describeFailure } from "./src/domain/errors.ts";
import { detectDeviceIdentity } from "./src/domain/motorIdentity.ts";
import { enrichSchema, settingAcceptsValue } from "./src/domain/schema.ts";

const LEVER = "ST-6870-L";
const POWER = "SM-BMR1";
const units: ComponentUnit[] = [
  { slot: 4, model_series: 7, model_unit: 1, model: LEVER, model_candidates: [LEVER], part: 1, model_params: null, raw: [7, 1] },
  { slot: 13, model_series: 2, model_unit: 0, model: POWER, model_candidates: [POWER], part: null, model_params: null, raw: [2, 0] },
];

function spec(name: string, patch: Partial<SettingSpec> = {}): SettingSpec {
  return {
    name, unit: "wire function code", value_type: "number", min_raw: 0, max_raw: 3,
    step_raw: 1, choices: [0, 1, 2, 3], oem: false, nullable: false,
    wire_units_per_input: 1, ...patch,
  };
}

const leverSchema = [
  spec("switch-a-function"), spec("switch-b-function"),
  spec("switch-c-function", { read_only: true }), spec("switch-s-function"),
];
const powerSchema = [
  spec("multi-shift-limit.up", { min_raw: 2, max_raw: 255, choices: [2, 3, 255] }),
  spec("multi-shift-limit.down", { min_raw: 2, max_raw: 255, choices: [2, 3, 255] }),
];

function snapshot(unit: ComponentUnit): Snapshot {
  const lever = unit.model === LEVER;
  return {
    model: unit.model,
    slot: unit.slot,
    identity: {
      slot: unit.slot, model_series: unit.model_series, model_unit: unit.model_unit,
      model_name: unit.model, settings_model: unit.model,
      serial: lever ? "010203040506" : "0A0B0C0D0E0F",
      cargo: unit.model, dcas_version: "3.1.2.3", rx_version: "not applicable",
    },
    values: lever ? {
      "switch-a-function": { value: 2, raw: "2300" },
      "switch-b-function": { value: 3, raw: "2300" },
      "switch-c-function": { value: 1, raw: "0100" },
    } : {
      "multi-shift-limit.up": { value: 2, raw: "02ff" },
      "multi-shift-limit.down": { value: 255, raw: "02ff" },
    },
    unavailable: lever ? { "switch-s-function": "channel not available" } : {},
  };
}

interface Snapshot {
  model: string | null;
  slot: number;
  identity: {
    slot: number; model_series: number; model_unit: number;
    model_name: string | null; settings_model: string | null; serial: string;
    cargo: string | null; dcas_version: string; rx_version: string;
  };
  values: Record<string, { value: number; raw: string }>;
  unavailable: Record<string, string>;
}
type Receipt = { setting: string; before?: number; after?: number; changed?: boolean };

function harness() {
  const controller = new SessionController();
  controller.setBluetoothBike("steps"); // These fixtures exercise explicit component routing, not detection.
  const commands: string[] = [];
  const files = new Map<string, Uint8Array>();
  const replies = new Map<number, Snapshot>(units.map((unit) => [unit.slot, snapshot(unit)]));
  const schemas = new Map<string, unknown>([[LEVER, leverSchema], [POWER, powerSchema]]);
  const config = {
    inventory: structuredClone(units),
    batch: {
      serial: "010203040506", slot: 4, completed: 2,
      results: [
        { setting: "switch-a-function", before: 2, after: 0, changed: true },
        { setting: "switch-b-function", before: 3, after: 1, changed: true },
      ] as Receipt[],
    } as unknown,
    schemaError: "",
  };
  const print = (value: unknown, stream: "stdout" | "stderr" = "stdout") => {
    (globalThis as any).estepsTerminalPrint?.(typeof value === "string" ? value : JSON.stringify(value), stream);
  };
  const terminal: TerminalHost = {
    setPasskeyPrompt() {},
    status: () => ({ ready: true, busy: false, usbSelected: true, transport: "ble" }),
    files: {
      list: () => [...files.keys()].map((path) => ({ path })),
      bytes: (path) => files.get(path) ?? new Uint8Array(),
      import: async (path, bytes) => { files.set(path, bytes); },
      initialize: async () => {},
    },
    run: async (line) => {
      commands.push(line);
      if (line === "connect ble" || line === "disconnect") return { code: 0 };
      if (line === "--ble-master=master units") {
        print({ units: config.inventory, ble_routing: { route: "master", policy: "master", reason: "explicit" } });
        return { code: 0 };
      }
      if (line.startsWith("settings schema")) {
        if (config.schemaError) { print(config.schemaError, "stderr"); return { code: 1 }; }
        const model = line.match(/--model '([^']+)'/)?.[1];
        assert.ok(model && schemas.has(model), `unexpected schema command: ${line}`);
        print(schemas.get(model));
        return { code: 0 };
      }
      if (line.includes("settings read")) {
        const slot = Number(line.match(/--slot (\d+)/)?.[1]);
        assert.ok(replies.has(slot), `unexpected settings target: ${line}`);
        print(replies.get(slot));
        return { code: 0 };
      }
      if (line.includes("settings batch")) {
        print(config.batch);
        return { code: 0 };
      }
      assert.fail(`unexpected backend operation: ${line}`);
    },
  };
  controller.attachTerminal(terminal);
  return { controller, commands, files, replies, schemas, config };
}

const originalWindow = (globalThis as any).window;
const originalPrint = (globalThis as any).estepsTerminalPrint;
const originalFetch = globalThis.fetch;
beforeEach(() => {
  (globalThis as any).window = globalThis;
  delete (globalThis as any).estepsTerminalPrint;
});
afterEach(() => {
  (globalThis as any).window = originalWindow;
  (globalThis as any).estepsTerminalPrint = originalPrint;
  globalThis.fetch = originalFetch;
});

describe("Catalogued component discovery and schema binding", () => {
  it("selects current inventory targets with their exact dynamic Rust schema and no motor fallback", async () => {
    const fetched: string[] = [];
    globalThis.fetch = (async (url: unknown) => { fetched.push(String(url)); throw new Error("unexpected static schema fetch"); }) as typeof fetch;
    const h = harness();
    await h.controller.connectAndSync("ble");
    assert.equal(h.controller.getState().identity?.model, LEVER);
    assert.equal(h.controller.getState().identity?.isMotor, false);
    assert.deepEqual(h.controller.getState().units, units);
    assert.deepEqual(h.controller.getState().schema.map((setting) => setting.name), leverSchema.map((setting) => setting.name));
    assert.match(h.commands[2], /--slot 4 --component 'ST-6870-L' settings read$/);

    await h.controller.selectComponent(13);
    assert.equal(h.controller.getState().identity?.modelSlug, POWER);
    assert.deepEqual(h.controller.getState().schema.map((setting) => setting.name), powerSchema.map((setting) => setting.name));
    assert.deepEqual(Object.keys(h.controller.getState().liveValues), ["multi-shift-limit.up", "multi-shift-limit.down"]);
    assert.ok(h.commands.includes(`settings schema --model '${LEVER}'`));
    assert.ok(h.commands.includes(`settings schema --model '${POWER}'`));
    assert.ok(h.commands.some((line) => /--slot 13 --component 'SM-BMR1' settings read$/.test(line)));
    assert.ok(!h.commands.some((line) => line.includes("--ble-unit-auth") || line.includes("ep800")));
    assert.deepEqual(fetched, []);
  });

  it("accepts catalogued names containing a slash, such as SW-M8150-R/IR", async () => {
    const h = harness();
    h.config.inventory = [...units, { slot: 6, model_series: 0x2e, model_unit: 1, model: "SW-M8150-R/IR", model_candidates: ["SW-M8150-R/IR"] }];
    await h.controller.connectAndSync("ble");
    assert.equal(h.controller.getState().identity?.model, LEVER);
    assert.equal(h.controller.getState().units.length, units.length + 1);
    assert.deepEqual(detectDeviceIdentity({ model_series: 0x2e, model_unit: 1, model_name: "SW-M8150-R/IR", settings_model: "SW-M8150-R/IR" }).model, "SW-M8150-R/IR");
    assert.throws(() => detectDeviceIdentity({ model_series: 0x2e, model_unit: 1, model_name: "SW-M8150-R'IR", settings_model: "SW-M8150-R'IR" }), /do not match/);
  });

  it("keeps unknown stock visible without choosing it as a settings target", async () => {
    const h = harness();
    h.config.inventory = [{ slot: 9, model_series: 0x99, model_unit: 4, model: null, model_candidates: [] }];
    assert.equal(await h.controller.connectAndSync("ble"), null);
    assert.equal(h.controller.getState().phase, "ready");
    assert.equal(h.controller.getState().units[0].slot, 9);
    await assert.rejects(h.controller.selectComponent(9), /catalogued component/);
    assert.ok(!h.commands.some((line) => line.includes("settings")));
  });

  it("retains the inventory and refuses a schema error instead of substituting motor controls", async () => {
    const h = harness();
    h.config.schemaError = "Component schema unavailable";
    assert.equal(await h.controller.connectAndSync("ble"), null);
    assert.equal(h.controller.getState().phase, "ready");
    assert.equal(h.controller.getState().identity, null);
    assert.deepEqual(h.controller.getState().schema, []);
    assert.deepEqual(h.controller.getState().units, units);
    assert.match(h.controller.getState().failure?.error ?? "", /Component schema unavailable/);
  });

  it("discards a read of the previous component that lands after a target switch", async () => {
    const h = harness();
    const motor = { slot: 13, model_series: 0x23, model_unit: 0, model: "DU-EP800-CRG", model_candidates: ["DU-EP800-CRG"] };
    h.config.inventory = [units[0], motor];
    h.replies.set(13, {
      model: "DU-EP800/DU-EP800-CRG", slot: 13,
      identity: { ...snapshot(motor).identity, serial: "010203040506", cargo: "00-CRG", settings_model: "ep800-crg" },
      values: { lamp: { value: 1, raw: "01" } }, unavailable: {},
    });
    // Motor schemas come from static files, so the motor selection finishes
    // before a lever read queued behind it.
    globalThis.fetch = (async () => ({ ok: true, json: async () => [spec("lamp", { min_raw: 0, max_raw: 1, choices: [0, 1] })] })) as unknown as typeof fetch;
    await h.controller.connectAndSync("ble");
    await h.controller.selectComponent(4);
    const selecting = h.controller.selectComponent(13);
    const staleRead = h.controller.readAllParameters();
    await selecting;
    await assert.rejects(staleRead, /component changed during the read/);
    assert.equal(h.controller.getState().identity?.slot, 13);
    assert.deepEqual(Object.keys(h.controller.getState().liveValues), ["lamp"]);
    assert.equal(h.controller.getState().failure, null);
  });

  it("rejects an identity/schema mismatch before replacing the selected component", async () => {
    const h = harness();
    await h.controller.connectAndSync("ble");
    h.replies.get(13)!.identity.settings_model = LEVER;
    await assert.rejects(h.controller.selectComponent(13), /identity and settings schema do not match/);
    assert.equal(h.controller.getState().identity?.model, LEVER);
    assert.equal(h.controller.getState().liveValues["switch-a-function"].value, 2);
    assert.ok(!h.commands.includes(`settings schema --model '${POWER}'`));
  });

  it("rejects changed stock series or unit on a later read even when slot, serial and schema name match", async () => {
    const h = harness();
    await h.controller.connectAndSync("ble");
    const live = structuredClone(h.controller.getState().liveValues);
    const reply = h.replies.get(4)!;
    reply.values["switch-a-function"]!.value = 0;
    for (const [field, changed] of [["model_series", 8], ["model_unit", 2]] as const) {
      const original = reply.identity[field];
      reply.identity[field] = changed;
      await assert.rejects(h.controller.readAllParameters(), /different component/);
      assert.deepEqual(h.controller.getState().liveValues, live);
      assert.equal(h.controller.getState().identity?.modelSeries, 7);
      assert.equal(h.controller.getState().identity?.modelUnit, 1);
      reply.identity[field] = original;
    }
  });

  it("distinguishes displays sharing a motor series and requires exact component schema identity", () => {
    assert.deepEqual(detectDeviceIdentity({ model_series: 0x23, model_unit: 2, model_name: "SC-EM800", settings_model: "SC-EM800" }), {
      model: "SC-EM800", modelSlug: "SC-EM800", isCargo: false, isMotor: false, modelUnit: 2,
    });
    assert.throws(() => detectDeviceIdentity({ model_series: 0x23, model_unit: 2, model_name: "SC-EM800", settings_model: "ep800-crg" }), /do not match/);
    assert.throws(() => detectDeviceIdentity({ model_series: 0x99, model_unit: 1 }), /do not match/);
    const limit = enrichSchema(powerSchema, POWER)[0];
    assert.equal(settingAcceptsValue(limit, 255), true);
    assert.equal(settingAcceptsValue(limit, 4), false);
  });
});

describe("Component staging and restore boundaries", () => {
  it("rejects read-only, unknown, unavailable and invalid values without staging or backend writes", async () => {
    const h = harness();
    await h.controller.connectAndSync("ble");
    const commands = [...h.commands];
    for (const [name, value] of [
      ["switch-c-function", 0], ["lamp", 1], ["switch-s-function", 2],
      ["switch-a-function", 4], ["switch-a-function", 1.5],
      ["switch-a-function", NaN], ["switch-a-function", Infinity], ["switch-a-function", ""],
    ] as Array<[string, number | string]>) {
      assert.throws(() => h.controller.stageParameter(name, value), /unavailable|read-only|range|choices/);
      assert.equal(h.controller.getState().failure?.kind, "stage");
      assert.deepEqual(h.controller.getState().stagedValues, {});
    }
    h.controller.stageParameter("switch-a-function", "3");
    assert.deepEqual(h.controller.getState().stagedValues, { "switch-a-function": 3 });
    h.controller.stageParameter("switch-a-function", 2);
    assert.deepEqual(h.controller.getState().stagedValues, {});
    assert.deepEqual(h.commands, commands);
  });

  it("blocks target and inventory changes until staged edits are saved or discarded", async () => {
    const h = harness();
    await h.controller.connectAndSync("ble");
    h.controller.stageParameter("switch-a-function", 0);
    const commands = [...h.commands];
    await assert.rejects(h.controller.selectComponent(13), /Save or discard staged changes/);
    await assert.rejects(h.controller.refreshUnits(), /Save or discard staged changes/);
    assert.equal(h.controller.getState().identity?.model, LEVER);
    assert.deepEqual(h.commands, commands);
    h.controller.discardAllStaged();
    await h.controller.selectComponent(13);
    assert.equal(h.controller.getState().identity?.model, POWER);
  });

  for (const [label, patch] of [
    ["wrong serial", { serial: "FFFFFFFFFFFF" }],
    ["wrong model", { settings_model: POWER }],
    ["missing component model", { settings_model: undefined }],
  ] as const) {
    it(`rejects a restore with ${label} before issuing a backend write`, async () => {
      const h = harness();
      await h.controller.connectAndSync("ble");
      h.files.set("backup.json", new TextEncoder().encode(JSON.stringify({
        format: "cranklink-setting-backup-v1", serial: "010203040506", settings_model: LEVER,
        setting: "switch-a-function", before: 0, raw_before: "0300", ...patch,
      })));
      const commands = [...h.commands];
      await assert.rejects(h.controller.restoreBackupFiles(["backup.json"]), /model or serial does not match/);
      assert.deepEqual(h.commands, commands);
      assert.equal(h.controller.getState().failure?.kind, "restore");
      assert.equal(h.controller.getState().liveValues["switch-a-function"].value, 2);
    });
  }

  it("restores one save's backups in their plan order through a single batch", async () => {
    const h = harness();
    await h.controller.connectAndSync("ble");
    const files = ["tx_1.001.backup.json", "tx_1.002.backup.json"];
    for (const [file, setting, before] of [[files[0], "switch-a-function", 2], [files[1], "switch-b-function", 3]] as const) {
      h.files.set(file, new TextEncoder().encode(JSON.stringify({
        format: "cranklink-setting-backup-v1", serial: "010203040506", settings_model: LEVER, setting, before,
      })));
    }
    const start = h.commands.length;
    // This session's file list keeps write order: lowering a region backs up
    // the speed (.002) before the destination (.001).
    await h.controller.restoreBackupFiles([...files].reverse());
    // One file at a time, the core refuses to undo a region raise.
    const writes = h.commands.slice(start).filter((line) => /settings (restore|batch)/.test(line));
    assert.equal(writes.length, 1);
    const plan = writes[0].match(/settings batch '?([^' ]+)'? --backup-prefix '?(pre_restore_\d+)'?$/);
    assert.ok(plan, writes[0]);
    assert.equal(plan[1], `${plan[2]}.plan.json`);
    assert.deepEqual(JSON.parse(new TextDecoder().decode(h.files.get(plan[1]))), files.map((file) => ({ op: "restore", file })));
    assert.equal(h.controller.getState().phase, "ready");
  });

  it("restores a save's destination before a limit it planned first", async () => {
    const h = harness();
    await h.controller.connectAndSync("ble");
    // A CLI batch may lower 1 / 32 km/h to 3 / 25 km/h as [speed-limit, destination].
    // Only the setting names matter to this ordering.
    const files = ["tx_1.001.backup.json", "tx_1.002.backup.json"];
    for (const [file, setting, before] of [[files[0], "speed-limit", 32], [files[1], "destination", 1]] as const) {
      h.files.set(file, new TextEncoder().encode(JSON.stringify({
        format: "cranklink-setting-backup-v1", serial: "010203040506", settings_model: LEVER, setting, before,
      })));
    }
    const start = h.commands.length;
    await h.controller.restoreBackupFiles(files);
    const plan = h.commands.slice(start).find((line) => /settings batch/.test(line))?.match(/settings batch '?([^' ]+)'?/);
    assert.ok(plan);
    assert.deepEqual(JSON.parse(new TextDecoder().decode(h.files.get(plan[1]))), [files[1], files[0]].map((file) => ({ op: "restore", file })));
  });

  it("records a refused restore while edits are staged or the backup is unreadable", async () => {
    const h = harness();
    await h.controller.connectAndSync("ble");
    h.files.set("backup.json", new TextEncoder().encode(JSON.stringify({
      format: "cranklink-setting-backup-v1", serial: "010203040506", settings_model: LEVER,
      setting: "switch-a-function", before: 0, raw_before: "0300",
    })));
    h.files.set("broken.json", new TextEncoder().encode("{"));
    h.controller.stageParameter("switch-b-function", 1);
    const commands = [...h.commands];
    await assert.rejects(h.controller.restoreBackupFiles(["backup.json"]), /Save or discard staged changes/);
    assert.equal(h.controller.getState().failure?.kind, "restore");
    h.controller.discardAllStaged();
    h.controller.dismissError();
    await assert.rejects(h.controller.restoreBackupFiles(["backup.json", "broken.json"]), /broken\.json is not valid JSON/);
    assert.equal(h.controller.getState().failure?.kind, "restore");
    // Like files.js, a missing file throws; it must not read as invalid JSON.
    const store = (h.controller as any).terminal.files;
    const bytes = store.bytes;
    store.bytes = (path: string) => {
      if (!h.files.has(path)) throw new Error(`File not found: ${path}`);
      return bytes(path);
    };
    await assert.rejects(h.controller.restoreBackupFiles(["gone.json"]), /gone\.json could not be read: File not found/);
    assert.deepEqual(h.commands, commands);
  });

  it("reconnects the previously selected component and restores edits only after its identity matches", async () => {
    const h = harness();
    await h.controller.connectAndSync("ble");
    await h.controller.selectComponent(13);
    h.controller.stageParameter("multi-shift-limit.up", 3);
    const reconnectStart = h.commands.length;
    await h.controller.reconnect();
    assert.equal(h.controller.getState().identity?.model, POWER);
    assert.equal(h.controller.getState().identity?.serial, "0A0B0C0D0E0F");
    assert.deepEqual(h.controller.getState().stagedValues, { "multi-shift-limit.up": 3 });
    assert.equal(h.controller.getState().liveValues["multi-shift-limit.up"].value, 2);
    assert.deepEqual(h.commands.slice(reconnectStart, reconnectStart + 2), ["disconnect", "connect ble"]);
    assert.ok(h.commands.slice(reconnectStart).some((line) => line.includes("--slot 13 --component 'SM-BMR1'")));
  });

  it("does not carry staged edits onto a replacement component with a different serial", async () => {
    const h = harness();
    await h.controller.connectAndSync("ble");
    await h.controller.selectComponent(13);
    h.controller.stageParameter("multi-shift-limit.up", 3);
    h.replies.get(13)!.identity.serial = "111213141516";
    await h.controller.reconnect();
    assert.equal(h.controller.getState().identity?.model, POWER);
    assert.equal(h.controller.getState().identity?.serial, "111213141516");
    assert.deepEqual(h.controller.getState().stagedValues, {});
    assert.match(h.controller.getState().failure?.error ?? "", /unsaved changes were discarded/);
  });

  it("keeps the read failure that ended the search and says the edits are gone", async () => {
    const h = harness();
    await h.controller.connectAndSync("ble");
    h.controller.stageParameter("switch-a-function", 0);
    h.replies.get(4)!.identity.serial = "zz";
    await h.controller.reconnect();
    const state = h.controller.getState();
    assert.deepEqual(state.stagedValues, {});
    assert.match(state.failure?.error ?? "", /lacks a verified serial identity/);
    assert.equal(describeFailure(state.failure!, state.transport, []).outcome, "Your unsaved changes were discarded.");
  });

  /** Fails `connect ble` while `failing` is true, as a cancelled browser chooser does. */
  function cancellableChooser(h: ReturnType<typeof harness>) {
    const chooser = { failing: true };
    const terminal = (h.controller as any).terminal;
    const run = terminal.run;
    terminal.run = async (line: string) => {
      if (line === "connect ble" && chooser.failing) {
        (globalThis as any).estepsTerminalPrint?.("No device selected.", "stderr");
        return { code: 1 };
      }
      return run(line);
    };
    return chooser;
  }

  it("keeps the edits when the reconnect cannot connect and restores them at the next connect to that component", async () => {
    const h = harness();
    await h.controller.connectAndSync("ble");
    await h.controller.selectComponent(13);
    h.controller.stageParameter("multi-shift-limit.up", 3);
    const chooser = cancellableChooser(h);
    await assert.rejects(h.controller.reconnect(), /No device selected/);
    // The connect screen's own button fails the same way, and still keeps them.
    await assert.rejects(h.controller.connectAndSync("ble"), /No device selected/);
    let state = h.controller.getState();
    assert.deepEqual(state.stagedValues, {});
    assert.equal(state.failure?.kind, "connect");
    assert.equal(describeFailure(state.failure!, state.transport, []).outcome,
      "Your unsaved changes are kept: connect to SM-BMR1 (serial 0A0B0C0D0E0F) again to restore them. Connecting to another component discards them.");
    chooser.failing = false;
    await h.controller.connectAndSync("ble");
    state = h.controller.getState();
    assert.equal(state.identity?.serial, "0A0B0C0D0E0F");
    assert.deepEqual(state.stagedValues, { "multi-shift-limit.up": 3 });
    assert.equal(state.failure, null);
  });

  it("discards held edits when the next connect reaches another component", async () => {
    const h = harness();
    await h.controller.connectAndSync("ble");
    await h.controller.selectComponent(13);
    h.controller.stageParameter("multi-shift-limit.up", 3);
    const chooser = cancellableChooser(h);
    await assert.rejects(h.controller.reconnect(), /No device selected/);
    chooser.failing = false;
    // Another bike, whose SM-BMR1 has another serial.
    h.replies.get(13)!.identity.serial = "111213141516";
    const connectStart = h.commands.length;
    await h.controller.connectAndSync("ble");
    const state = h.controller.getState();
    // The search reads that SM-BMR1, then the component the connect chose is selected again.
    const reads = h.commands.slice(connectStart).filter((line) => line.includes("settings read"));
    assert.deepEqual(reads.map((line) => Number(line.match(/--slot (\d+)/)?.[1])), [4, 13, 4]);
    assert.equal(state.identity?.model, LEVER);
    assert.deepEqual(state.stagedValues, {});
    assert.equal(state.failure?.error, "SM-BMR1 (serial 0A0B0C0D0E0F) did not answer this connection, so your unsaved changes were discarded.");
    // Nothing stays held for a later connect.
    h.replies.get(13)!.identity.serial = "0A0B0C0D0E0F";
    await h.controller.disconnect();
    await h.controller.connectAndSync("ble");
    assert.equal(h.controller.getState().identity?.model, LEVER);
    assert.deepEqual(h.controller.getState().stagedValues, {});
  });

  it("restores held edits before it shows an adapter unplugged during the reconnect", async () => {
    const h = harness();
    const terminal = (h.controller as any).terminal;
    const run = terminal.run;
    let unplugAt = "";
    // The harness answers for Bluetooth; the commands are the same over USB.
    terminal.run = async (line: string) => {
      const reply = await run(line.replace(/^connect usb$/, "connect ble").replace(/^units$/, "--ble-master=master units"));
      if (unplugAt && line.includes(unplugAt)) h.controller.adapterDisconnected("USB device disconnected");
      return reply;
    };
    await h.controller.connectAndSync("usb");
    await h.controller.selectComponent(13);
    h.controller.stageParameter("multi-shift-limit.up", 3);
    // The reconnect's search reads the component, then the adapter is pulled.
    unplugAt = "--slot 13 ";
    await h.controller.reconnect();
    const state = h.controller.getState();
    assert.equal(state.identity?.serial, "0A0B0C0D0E0F");
    assert.deepEqual(state.stagedValues, { "multi-shift-limit.up": 3 });
    assert.equal(state.failure?.kind, "adapter");
    assert.equal(state.linkLost, true);
  });

  it("restores no edits when one of them can no longer be staged", async () => {
    const h = harness();
    await h.controller.connectAndSync("ble");
    h.controller.stageParameter("switch-a-function", 3);
    h.controller.stageParameter("switch-b-function", 2);
    const lever = h.replies.get(4)!;
    delete lever.values["switch-b-function"];
    lever.unavailable["switch-b-function"] = "channel not available";
    await h.controller.reconnect();
    assert.equal(h.controller.getState().identity?.serial, "010203040506");
    assert.deepEqual(h.controller.getState().stagedValues, {});
    assert.match(h.controller.getState().failure?.error ?? "", /discarded after reconnecting: .*could not be staged again/);
  });

  it("keeps looking for the previous serial when one candidate cannot be read", async () => {
    const h = harness();
    const lever13 = { ...units[0], slot: 13 };
    const lever19 = { ...units[0], slot: 19 };
    h.config.inventory = [units[0], lever13];
    h.replies.set(13, snapshot(lever13));
    h.replies.get(13)!.identity.serial = "0A0B0C0D0E0F";
    await h.controller.connectAndSync("ble");
    await h.controller.selectComponent(13);
    h.controller.stageParameter("switch-a-function", 0);
    // The previous slot now answers with another identity; the lever moved to slot 19.
    h.config.inventory = [units[0], lever13, lever19];
    h.replies.get(13)!.identity.model_unit = 9;
    h.replies.set(19, snapshot(lever19));
    h.replies.get(19)!.identity.serial = "0A0B0C0D0E0F";
    await h.controller.reconnect();
    assert.equal(h.controller.getState().identity?.slot, 19);
    assert.deepEqual(h.controller.getState().stagedValues, { "switch-a-function": 0 });
    assert.equal(h.controller.getState().failure, null);
  });

  it("reselects the previous serial when another component has the same model", async () => {
    const h = harness();
    const secondLever = { ...units[0], slot: 13 };
    h.config.inventory = [units[0], secondLever];
    h.replies.set(13, snapshot(secondLever));
    h.replies.get(13)!.identity.serial = "0A0B0C0D0E0F";
    await h.controller.connectAndSync("ble");
    await h.controller.selectComponent(13);
    h.controller.stageParameter("switch-a-function", 0);
    await h.controller.reconnect();
    assert.equal(h.controller.getState().identity?.slot, 13);
    assert.equal(h.controller.getState().identity?.serial, "0A0B0C0D0E0F");
    assert.deepEqual(h.controller.getState().stagedValues, { "switch-a-function": 0 });
  });

  it("finds the previous serial after matching components move to different slots", async () => {
    const h = harness();
    const secondLever = { ...units[0], slot: 19 };
    h.config.inventory = [units[0], units[1], secondLever];
    h.replies.set(19, snapshot(secondLever));
    h.replies.get(19)!.identity.serial = "111213141516";
    await h.controller.connectAndSync("ble");
    await h.controller.selectComponent(19);
    h.controller.stageParameter("switch-a-function", 0);
    // A power unit is selected first; the previous slot now belongs to the
    // other lever, so reconnect must verify both same-model candidates.
    h.config.inventory = [units[1], units[0], secondLever];
    h.replies.get(19)!.identity.serial = "010203040506";
    h.replies.get(4)!.identity.serial = "111213141516";
    await h.controller.reconnect();
    assert.equal(h.controller.getState().identity?.slot, 4);
    assert.equal(h.controller.getState().identity?.serial, "111213141516");
    assert.deepEqual(h.controller.getState().stagedValues, { "switch-a-function": 0 });
  });
});

describe("Agent write review boundaries", () => {
  function serverFor(controller: SessionController) {
    const browserWindow = (globalThis as any).window;
    delete (globalThis as any).window;
    try { return new WebMCPServer(controller); }
    finally { (globalThis as any).window = browserWindow; }
  }
  const request = (server: WebMCPServer) => server.handleRequest("tools/call", {
    name: "cranklink_set_setting", arguments: { name: "switch-b-function", value: 1 },
  });
  const batchRequest = (server: WebMCPServer, operations: Array<{ name: string; value: number }>) => server.handleRequest("tools/call", {
    name: "cranklink_batch_settings", arguments: { operations: operations.map((op) => ({ op: "set", ...op })) },
  });
  function motorHarness() {
    const h = harness();
    const motor = { slot: 13, model_series: 0x23, model_unit: 0, model: "DU-EP800-CRG", model_candidates: ["DU-EP800-CRG"] };
    h.config.inventory = [motor];
    h.replies.set(13, {
      model: "DU-EP800/DU-EP800-CRG", slot: 13,
      identity: { ...snapshot(motor).identity, serial: "010203040506", cargo: "00-CRG", settings_model: "ep800-crg" },
      values: { destination: { value: 3, raw: "0103ffff" }, "speed-limit": { value: 25, raw: "c409ffff" } }, unavailable: {},
    });
    h.schemas.set("ep800-crg", [
      spec("destination", { min_raw: 0, max_raw: 4, choices: [0, 1, 3, 4] }),
      spec("speed-limit", { min_raw: 1500, max_raw: 3200, step_raw: 100, choices: [], wire_units_per_input: 100 }),
    ]);
    return h;
  }

  it("answers WebMCP messages only from this page's origin", async () => {
    const h = harness();
    let listener: ((event: any) => Promise<void>) | null = null;
    const page = { addEventListener: (_: string, fn: any) => { listener = fn; }, location: { origin: "https://app.example" } };
    const browserWindow = (globalThis as any).window;
    (globalThis as any).window = page;
    try { new WebMCPServer(h.controller); }
    finally { (globalThis as any).window = browserWindow; }
    const replies: Array<[unknown, string]> = [];
    const source = { postMessage: (message: unknown, origin: string) => { replies.push([message, origin]); } };
    const data = { jsonrpc: "2.0", id: 1, method: "tools/list" };
    (globalThis as any).window = page;
    try {
      await listener!({ data, origin: "https://evil.example", source });
      assert.deepEqual(replies, []);
      await listener!({ data, origin: "https://app.example", source });
      assert.equal(replies.length, 1);
      assert.equal(replies[0][1], "https://app.example");
    } finally { (globalThis as any).window = browserWindow; }
  });

  it("names the target in the review and drops a pending review on disconnect", async () => {
    const h = harness();
    await h.controller.connectAndSync("ble");
    const pending = request(serverFor(h.controller));
    const identity = h.controller.getState().identity!;
    assert.deepEqual(h.controller.getState().mcpApproval?.target, { model: LEVER, slot: 4, serial: identity.serial });
    const disconnecting = h.controller.disconnect();
    // Authorizing, or a new request, while the disconnect runs must not queue a write behind it.
    h.controller.respondMcpApproval(true);
    const late = request(serverFor(h.controller));
    await disconnecting;
    await assert.rejects(pending, /rejected by operator/);
    await assert.rejects(late);
    assert.equal(h.controller.getState().mcpApproval, null);
    assert.ok(!h.commands.some((line) => line.includes("settings batch")));
  });

  it("refuses device tools on a lost link before asking the operator", async () => {
    const h = harness();
    await h.controller.connectAndSync("ble");
    (h.controller as any).update({ linkLost: true });
    const server = serverFor(h.controller);
    const commands = [...h.commands];
    for (const name of ["cranklink_read_settings", "cranklink_create_backup"]) {
      await assert.rejects(server.handleRequest("tools/call", { name, arguments: {} }), /connected, idle component/);
    }
    await assert.rejects(request(server), /connected, idle component/);
    assert.equal(h.controller.getState().mcpApproval, null);
    assert.equal(h.controller.getState().linkLost, true);
    assert.deepEqual(h.commands, commands);
  });

  it("refuses an agent write while other changes are staged", async () => {
    const h = harness();
    await h.controller.connectAndSync("ble");
    h.controller.stageParameter("switch-a-function", 0);
    const commands = [...h.commands];
    await assert.rejects(request(serverFor(h.controller)), /Save or discard staged changes/);
    assert.equal(h.controller.getState().mcpApproval, null);
    assert.deepEqual(h.controller.getState().stagedValues, { "switch-a-function": 0 });
    assert.deepEqual(h.commands, commands);
  });

  it("does not commit unrelated edits introduced while the operator reviews an agent write", async () => {
    const h = harness();
    await h.controller.connectAndSync("ble");
    const pending = request(serverFor(h.controller));
    assert.deepEqual(h.controller.getState().mcpApproval?.diff.map((entry) => [entry.name, entry.regulated]), [["switch-b-function", false]]);
    h.controller.stageParameter("switch-a-function", 0);
    h.controller.respondMcpApproval(true);
    await assert.rejects(pending, /Staged changes appeared during review/);
    assert.deepEqual(h.controller.getState().stagedValues, { "switch-a-function": 0 });
    assert.equal(h.controller.getState().liveValues["switch-b-function"].value, 3);
    assert.ok(!h.commands.some((line) => line.includes("settings batch")));
  });

  it("keeps one review active and commits exactly its approved operations", async () => {
    const h = harness();
    await h.controller.connectAndSync("ble");
    h.config.batch = { completed: 1, results: [{ setting: "switch-b-function", after: 1 }] };
    const server = serverFor(h.controller);
    const pending = request(server);
    const reviewId = h.controller.getState().mcpApproval?.id;
    await assert.rejects(request(server), /pending change review/);
    assert.equal(h.controller.getState().mcpApproval?.id, reviewId);
    h.controller.respondMcpApproval(true);
    await pending;
    const planFile = [...h.files.keys()].find((name) => name.endsWith(".plan.json"))!;
    assert.deepEqual(JSON.parse(new TextDecoder().decode(h.files.get(planFile))), [
      { op: "set", name: "switch-b-function", value: "1" },
    ]);
  });

  it("rejects duplicate requested settings before creating a review", async () => {
    const h = harness();
    await h.controller.connectAndSync("ble");
    await assert.rejects(batchRequest(serverFor(h.controller), [
      { name: "switch-a-function", value: 0 }, { name: "switch-a-function", value: 1 },
    ]), /only once/);
    assert.equal(h.controller.getState().mcpApproval, null);
    assert.deepEqual(h.controller.getState().stagedValues, {});
  });

  it("clears the agent's temporary plan if a later requested value fails validation", async () => {
    const h = harness();
    await h.controller.connectAndSync("ble");
    const pending = batchRequest(serverFor(h.controller), [
      { name: "switch-a-function", value: 0 }, { name: "switch-b-function", value: 9 },
    ]);
    h.controller.respondMcpApproval(true);
    await assert.rejects(pending, /range or choices/);
    assert.deepEqual(h.controller.getState().stagedValues, {});
    assert.ok(!h.commands.some((line) => line.includes("settings batch")));
  });

  for (const [label, operations] of [
    ["a speed change adds an unreviewed destination", [{ name: "speed-limit", value: 32 }]],
    ["derived destination differs from the reviewed value", [{ name: "destination", value: 3 }, { name: "speed-limit", value: 32 }]],
    ["an unsupported speed is not staged", [{ name: "speed-limit", value: 100 }]],
  ] as Array<[string, Array<{ name: string; value: number }> ]>) {
    it(`rejects and clears an agent plan when ${label}`, async () => {
      const h = motorHarness();
      await h.controller.connectAndSync("ble");
      const pending = batchRequest(serverFor(h.controller), operations);
      h.controller.respondMcpApproval(true);
      await assert.rejects(pending, /Include the required destination and speed settings explicitly in a new review/);
      assert.deepEqual(h.controller.getState().stagedValues, {});
      assert.equal(h.controller.getState().liveValues.destination.value, 3);
      assert.equal(h.controller.getState().liveValues["speed-limit"].value, 25);
      assert.ok(!h.commands.some((line) => line.includes("settings batch")));
    });
  }

  it("commits a destination and speed pair only when both final values were reviewed", async () => {
    const h = motorHarness();
    await h.controller.connectAndSync("ble");
    h.config.batch = { completed: 2, results: [{ setting: "destination", after: 1 }, { setting: "speed-limit", after: 32 }] };
    const pending = batchRequest(serverFor(h.controller), [
      { name: "destination", value: 1 }, { name: "speed-limit", value: 32 },
    ]);
    assert.deepEqual(h.controller.getState().mcpApproval?.diff.map((entry) => [entry.name, entry.regulated]), [["destination", true], ["speed-limit", false]]);
    h.controller.respondMcpApproval(true);
    await pending;
    assert.deepEqual(h.controller.getState().stagedValues, {});
    assert.equal(h.controller.getState().liveValues.destination.value, 1);
    assert.equal(h.controller.getState().liveValues["speed-limit"].value, 32);
    const planFile = [...h.files.keys()].find((name) => name.endsWith(".plan.json"))!;
    assert.deepEqual(JSON.parse(new TextDecoder().decode(h.files.get(planFile))), [
      { op: "set", name: "destination", value: "1" },
      { op: "set", name: "speed-limit", value: "32" },
    ]);
  });
});

describe("Verified component batch receipts", () => {
  const valid = [
    { setting: "switch-a-function", before: 2, after: 0, changed: true },
    { setting: "switch-b-function", before: 3, after: 1, changed: true },
  ];
  const malformed = [
    ["partial completion", { completed: 1, results: valid.slice(0, 1) }],
    ["missing completion", { results: valid }],
    ["missing receipt", { completed: 2, results: valid.slice(0, 1) }],
    ["wrong order", { completed: 2, results: [...valid].reverse() }],
    ["duplicate receipt", { completed: 2, results: [valid[0], valid[0]] }],
    ["missing readback", { completed: 2, results: [valid[0], { setting: "switch-b-function" }] }],
    ["mismatched readback", { completed: 2, results: [valid[0], { ...valid[1], after: 2 }] }],
    ["null readback", { completed: 2, results: [valid[0], { ...valid[1], after: null }] }],
    ["non-array receipts", { completed: 2, results: {} }],
    ["invalid JSON", "Write returned no JSON receipt"],
  ] as const;

  for (const [label, receipt] of malformed) {
    it(`retains staged edits and live readings after ${label}`, async () => {
      const h = harness();
      await h.controller.connectAndSync("ble");
      h.controller.stageParameter("switch-a-function", 0);
      h.controller.stageParameter("switch-b-function", 1);
      h.config.batch = receipt;
      const live = structuredClone(h.controller.getState().liveValues);
      const staged = structuredClone(h.controller.getState().stagedValues);
      await assert.rejects(h.controller.commitStagedBatch(), /complete set of verified setting receipts/);
      assert.deepEqual(h.controller.getState().liveValues, live);
      assert.deepEqual(h.controller.getState().stagedValues, staged);
      assert.equal(h.controller.getState().phase, "ready");
      assert.equal(h.controller.getState().failure?.kind, "save");
      assert.equal(h.commands.filter((line) => line.includes("settings batch")).length, 1);
    });
  }

  it("records a refused save so the banner can explain it", async () => {
    const h = harness();
    await h.controller.connectAndSync("ble");
    h.controller.stageParameter("switch-a-function", 0);
    const commands = [...h.commands];
    for (const [patch, error] of [
      [{ linkLost: true }, /Reconnect to an idle component before saving/],
      [{ unavailableValues: { "switch-a-function": "channel not available" } }, /switch-a-function is unavailable/],
    ] as const) {
      (h.controller as any).update({ failure: null, linkLost: false, unavailableValues: {}, ...patch });
      await assert.rejects(h.controller.commitStagedBatch(), error);
      assert.equal(h.controller.getState().failure?.kind, "save");
      assert.match(h.controller.getState().failure?.error ?? "", error);
    }
    assert.deepEqual(h.controller.getState().stagedValues, { "switch-a-function": 0 });
    assert.deepEqual(h.commands, commands);
  });

  it("accepts a null readback for a nullable date saved as unset", async () => {
    const h = harness();
    h.schemas.set(LEVER, [...leverSchema, spec("maintenance-date", { value_type: "date", nullable: true, unit: "", choices: [] })]);
    (h.replies.get(4)!.values as any)["maintenance-date"] = { value: "2025-01-01", raw: "19010100" };
    await h.controller.connectAndSync("ble");
    h.controller.stageParameter("maintenance-date", "unset");
    h.config.batch = { completed: 1, results: [{ setting: "maintenance-date", before: "2025-01-01", after: null, changed: true }] };
    await h.controller.commitStagedBatch();
    assert.equal(h.controller.getState().liveValues["maintenance-date"].value, null);
    assert.deepEqual(h.controller.getState().stagedValues, {});
    assert.equal(h.controller.getState().failure, null);
  });

  it("applies only a complete ordered readback and binds its write plan to model, slot and serial", async () => {
    const h = harness();
    await h.controller.connectAndSync("ble");
    h.controller.stageParameter("switch-a-function", 0);
    h.controller.stageParameter("switch-b-function", 1);
    await h.controller.commitStagedBatch();
    assert.deepEqual(h.controller.getState().stagedValues, {});
    assert.equal(h.controller.getState().liveValues["switch-a-function"].value, 0);
    assert.equal(h.controller.getState().liveValues["switch-b-function"].value, 1);
    const batchCommand = h.commands.find((line) => line.includes("settings batch"))!;
    assert.match(batchCommand, /--slot 4 --component 'ST-6870-L' --expect-serial 010203040506 settings batch/);
    const planFile = [...h.files.keys()].find((name) => name.endsWith(".plan.json"))!;
    assert.deepEqual(JSON.parse(new TextDecoder().decode(h.files.get(planFile))), [
      { op: "set", name: "switch-a-function", value: "0" },
      { op: "set", name: "switch-b-function", value: "1" },
    ]);
  });
});
