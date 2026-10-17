/**
 * sessionController.ts - Deterministic Finite State Machine & Session Controller.
 *
 * Unifies transport selection, automatic probing, schema binding, full register
 * reading, inline staging, single-session batch commits (with automatic pre-write
 * backups), and synchronized dual-MCU firmware operations.
 */

import type {
  InstrumentState,
  TransportType,
  SimulatedModel,
  SimulatorOptions,
  DeviceIdentity,
  SettingEntry,
  JournalSnapshot,
  InspectedFirmwareFile,
  WorkspaceSection,
  SettingSectionId,
} from "../types.ts";
import { enrichSchema, formatSettingValue, destinationForSpeed, destinationSpeedRange } from "../domain/schema.ts";
import {
  classifyFirmwareRole,
  evaluateCompanionPair,
  buildSynchronizedFlashPlan,
} from "../domain/firmwarePackage.ts";
import {
  detectPlatform,
  requestScreenWakeLock,
  releaseScreenWakeLock,
} from "./platform.ts";

export interface TerminalHost {
  run: (line: string) => Promise<{ code: number }>;
  files: {
    list: () => Array<{ path: string; size?: number }>;
    bytes: (path: string) => Uint8Array;
    import: (path: string, bytes: Uint8Array) => Promise<void>;
    initialize: () => Promise<void>;
  };
  status: () => {
    ready: boolean;
    busy: boolean;
    usbSelected: boolean;
    transport: string | null;
  };
  simulator?: any;
  configureSimulator?: (opts: any) => void;
}

export class SessionController {
  private state: InstrumentState;
  private listeners: Set<(state: InstrumentState) => void> = new Set();
  private terminal: TerminalHost | null = null;
  private logCounter = 0;
  private activeCapture: { stdout: string; stderr: string } | null = null;
  private execQueue: Promise<any> = Promise.resolve();
  private syncInProgress: Promise<DeviceIdentity> | null = null;
  private speedDestinationBeforeAuto: { staged: number | string | undefined } | null = null;

  constructor() {
    this.state = {
      phase: "disconnected",
      statusMessage: "Select a physical adapter or start the simulator.",
      errorBanner: null,
      transport: null,
      simulatorOptions: {
        model: "ep800-crg",
        initialPreset: "eu_default",
        fault: "none",
      },
      identity: null,
      schema: [],
      liveValues: {},
      unavailableValues: {},
      stagedValues: {},
      activeSection: "parameters",
      activeParameterGroup: "geometry",
      activeProfileTab: 1,
      searchFilter: "",
      firmware: {
        dcasFile: null,
        rxFile: null,
        companionValid: false,
        companionMessage: "Select a matched DCAS (-D) and RX (-M) firmware pair to begin.",
        dryRunPassed: false,
        dryRunSerial: null,
        lastFlashReceipt: null,
      },
      snapshots: [],
      logs: [],
      consoleExpanded: false,
      mcpApproval: null,
    };
  }

  public getState(): InstrumentState {
    return this.state;
  }

  public subscribe(listener: (state: InstrumentState) => void): () => void {
    this.listeners.add(listener);
    listener(this.state);
    return () => this.listeners.delete(listener);
  }

  private update(patch: Partial<InstrumentState>): void {
    this.state = { ...this.state, ...patch };
    if (patch.phase === "flashing" || patch.phase === "committing") {
      requestScreenWakeLock().catch(() => {});
    } else if (patch.phase === "ready" || patch.phase === "error" || patch.phase === "disconnected") {
      releaseScreenWakeLock();
    }
    for (const listener of this.listeners) {
      listener(this.state);
    }
  }

  public attachTerminal(term: TerminalHost): void {
    this.terminal = term;
    if (typeof window !== "undefined") {
      const prevPrint = (window as any).estepsTerminalPrint;
      (window as any).estepsTerminalPrint = (text: string, stream: "stdout" | "stderr" | "notice" = "stdout") => {
        if (prevPrint) prevPrint(text, stream);
        if (this.activeCapture && stream !== "notice") {
          this.activeCapture[stream] += text;
        }
        this.appendLog(stream, text);
      };
    }
    this.refreshSnapshots();
  }

  public appendLog(stream: "stdout" | "stderr" | "notice" | "cmd", text: string): void {
    const trimmed = text.replace(/\n$/, "");
    if (!trimmed) return;
    const entry = {
      id: ++this.logCounter,
      timestamp: new Date().toISOString().slice(11, 19),
      stream,
      text: trimmed,
    };
    const nextLogs = [...this.state.logs.slice(-299), entry];
    this.update({
      logs: nextLogs,
      ...(stream === "notice" && ["connecting", "probing"].includes(this.state.phase)
        ? { statusMessage: trimmed }
        : {}),
    });
  }

  private async exec(
    commandLine: string,
  ): Promise<{ code: number; stdout: string; stderr: string }> {
    if (!this.terminal) {
      throw new Error("WASM host is not initialized.");
    }
    const runTask = async () => {
      this.appendLog("cmd", `cranklink> ${commandLine}`);
      const capture = { stdout: "", stderr: "" };
      this.activeCapture = capture;
      try {
        const res = await this.terminal!.run(commandLine);
        // Strip echoed prompt line from stdout
        const cleanStdout = capture.stdout
          .replace(/^(cranklink|esteps)> [^\n]*\n/, "")
          .trim();
        return {
          code: res.code,
          stdout: cleanStdout,
          stderr: capture.stderr.trim(),
        };
      } finally {
        this.activeCapture = null;
      }
    };

    const nextPromise = this.execQueue.then(runTask, runTask);
    this.execQueue = nextPromise.catch(() => {});
    return nextPromise;
  }

  /**
   * Consolidated Zero-Click Handshake:
   * 1. Opens physical link or simulator
   * 2. Runs `probe` and parses actual JSON identity
   * 3. Loads the verified model schema (`ep800-crg`, `ep600`, or `ep801`)
   * 4. Runs `settings read` (with automatic unit auth on modern BLE/Sim models)
   *    and populates all live parameter values
   */
  public async connectAndSync(
    transport: TransportType,
    simOpts?: Partial<SimulatorOptions>,
  ): Promise<DeviceIdentity> {
    const task = this.doConnectAndSync(transport, simOpts);
    this.syncInProgress = task;
    try {
      return await task;
    } finally {
      if (this.syncInProgress === task) {
        this.syncInProgress = null;
      }
    }
  }

  private async doConnectAndSync(
    transport: TransportType,
    simOpts?: Partial<SimulatorOptions>,
  ): Promise<DeviceIdentity> {
    const mergedSim: SimulatorOptions = {
      ...this.state.simulatorOptions,
      ...simOpts,
    };

    this.update({
      phase: "connecting",
      errorBanner: null,
      simulatorOptions: mergedSim,
      statusMessage:
        transport === "sim"
          ? `Starting simulator (${mergedSim.model.toUpperCase()})...`
          : `Opening ${transport.toUpperCase()} adapter...`,
    });

    try {
      if (transport === "sim") {
        this.terminal?.configureSimulator?.(mergedSim);
        const conn = await this.exec(`connect sim ${mergedSim.model}`);
        if (conn.code !== 0) {
          throw new Error(conn.stderr || `Simulator connect failed (${conn.code})`);
        }
        if (mergedSim.fault && this.terminal?.simulator?.setFault) {
          this.terminal.simulator.setFault(mergedSim.fault);
        }
      } else {
        let conn = await this.exec(`connect ${transport}`);
        if (conn.code !== 0 && transport === "usb" && typeof navigator !== "undefined") {
          const plat = detectPlatform();
          if (plat.isWindows) {
            this.appendLog("notice", "WebUSB unavailable or claimed by driver. Attempting WebSerial fallback...");
            conn = await this.exec("connect serial");
          }
        }
        if (conn.code !== 0) {
          throw new Error(conn.stderr || `Connection failed (${conn.code})`);
        }
      }

      this.update({
        transport,
        phase: "probing",
        statusMessage: "Identifying drive unit and firmware versions...",
      });

      const identity = await this.probeIdentity(transport);

      this.update({
        identity,
        phase: "reading",
        statusMessage: `Loading ${identity.model} schema & reading parameters...`,
      });

      const schema = await this.loadSchema(identity.modelSlug);
      this.update({ schema });

      // On real BLE hardware (e.g. SC-EM800), the physical gateway shuts down and re-advertises
      // after the probe session's PC-link stop. A short pause guarantees radio readiness.
      if (transport === "ble") {
        await new Promise((r) => setTimeout(r, 1200));
      }

      await this.readAllParameters(transport, identity);
      this.refreshSnapshots();

      if (this.state.phase === "reading" || this.state.phase === "probing") {
        this.update({
          phase: "ready",
          statusMessage: `Synced with ${identity.model} (${Object.keys(this.state.liveValues).length} parameters read).`,
        });
      }

      return identity;
    } catch (err: any) {
      const msg = err?.message || String(err);
      this.update({
        phase: "error",
        errorBanner: msg,
        statusMessage: `Handshake failed: ${msg}`,
      });
      throw err;
    }
  }


  private async probeIdentity(transport: TransportType): Promise<DeviceIdentity> {
    const probeRes = await this.exec("probe");
    if (probeRes.code !== 0) {
      throw new Error(probeRes.stderr || `Probe failed (${probeRes.code})`);
    }

    const parsed = this.extractLastJsonObject(probeRes.stdout);
    if (!parsed) {
      throw new Error("Could not parse JSON identity from probe output.");
    }

    const series: number | undefined = parsed.model_series;
    const isCargo = parsed.cargo === true || parsed.cargo === "00-CRG";

    let modelSlug: SimulatedModel = "ep800-crg";
    let modelDisplay = "DU-EP800-CRG";

    if (series === 0x30 || (transport === "sim" && this.state.simulatorOptions.model === "ep600")) {
      modelSlug = "ep600";
      modelDisplay = "DU-EP600";
    } else if (series === 0x35 || (transport === "sim" && this.state.simulatorOptions.model === "ep801")) {
      modelSlug = "ep801";
      modelDisplay = "DU-EP801";
    } else if (!isCargo) {
      modelDisplay = "DU-EP800";
    }

    return {
      slot: parsed.slot ?? 13,
      model: modelDisplay,
      modelSlug,
      serial: String(parsed.serial || "010203040506").toUpperCase(),
      dcasVersion: String(parsed.dcas_version || "—"),
      rxVersion: String(parsed.rx_version || "—"),
      isCargo,
      modelSeries: series,
    };
  }

  private async loadSchema(modelSlug: SimulatedModel) {
    try {
      const res = await fetch(`./schemas/${modelSlug}.json`);
      if (res.ok) {
        const rawSpecs = await res.json();
        return enrichSchema(rawSpecs, modelSlug);
      }
    } catch {
      // Fallback to WASM CLI schema command if static fetch fails
    }
    const schemaRes = await this.exec(`settings schema --model ${modelSlug}`);
    if (schemaRes.code === 0) {
      const arr = JSON.parse(schemaRes.stdout);
      return enrichSchema(arr, modelSlug);
    }
    return [];
  }

  public async readAllParameters(
    transportOverride?: TransportType | null,
    identityOverride?: DeviceIdentity | null,
  ): Promise<Record<string, SettingEntry>> {
    const transport = transportOverride ?? this.state.transport;
    const identity = identityOverride ?? this.state.identity;
    if (!transport || !identity) {
      throw new Error("Connect to a drive unit before reading parameters.");
    }

    const wasReady = this.state.phase === "ready";
    if (wasReady) {
      this.update({
        phase: "reading",
        errorBanner: null,
        statusMessage: "Reading motor parameters...",
      });
    }

    try {
      const prefixFlags = this.buildSessionFlags(transport, identity, false);
      const readRes = await this.exec(`${prefixFlags}settings read`);
      if (readRes.code !== 0) {
        throw new Error(readRes.stderr || `Settings read failed (${readRes.code})`);
      }

      const snapshot = this.extractLastJsonObject(readRes.stdout);
      const liveValues: Record<string, SettingEntry> = {};
      const unavailableValues: Record<string, string> = {};

      if (snapshot && snapshot.values) {
        for (const [key, entry] of Object.entries<any>(snapshot.values)) {
          liveValues[key] = {
            value: entry?.value ?? null,
            raw: entry?.raw ?? "",
          };
        }
      }
      if (snapshot && snapshot.unavailable) {
        for (const [key, reason] of Object.entries<any>(snapshot.unavailable)) {
          unavailableValues[key] = String(reason);
        }
      }

      if (this.state.phase === "reading") {
        this.update({
          liveValues,
          unavailableValues,
          phase: "ready",
          statusMessage: `Synced with ${identity.model} (${Object.keys(liveValues).length} parameters read).`,
        });
      } else {
        this.update({
          liveValues,
          unavailableValues,
        });
      }
      return liveValues;
    } catch (err: any) {
      const msg = err?.message || String(err);
      this.update({
        phase: "error",
        errorBanner: msg,
        statusMessage: `Read error: ${msg}`,
      });
      throw err;
    }
  }

  private buildSessionFlags(
    transport: TransportType,
    identity: DeviceIdentity,
    requiresUnitAuth: boolean = false,
  ): string {
    const cleanSerial = identity.serial.replace(/[-:\s]/g, "");
    const isModern = identity.modelSlug === "ep600" || identity.modelSlug === "ep801";
    if (isModern && (transport === "ble" || transport === "sim" || requiresUnitAuth)) {
      return `--expect-serial ${cleanSerial} --ble-unit-auth `;
    }
    return `--expect-serial ${cleanSerial} `;
  }

  public async disconnect(): Promise<void> {
    try {
      if (this.terminal) {
        await this.exec("disconnect");
      }
    } finally {
      this.speedDestinationBeforeAuto = null;
      this.update({
        phase: "disconnected",
        transport: null,
        identity: null,
        liveValues: {},
        unavailableValues: {},
        stagedValues: {},
        errorBanner: null,
        statusMessage: "Disconnected.",
      });
    }
  }

  /**
   * Stage or unstage a parameter modification.
   * If the new value equals the live value on the motor, it automatically unstages.
   */
  public stageParameter(name: string, value: number | string): void {
    const nextStaged = { ...this.state.stagedValues };
    const stage = (key: string, next: number | string) => {
      const live = this.state.liveValues[key]?.value;
      if (live !== undefined && String(live) === String(next)) delete nextStaged[key];
      else nextStaged[key] = next;
    };
    const model = this.state.identity?.modelSlug;
    if (name === "speed-limit") {
      const priorAuto = this.speedDestinationBeforeAuto;
      // Recompute from the user's destination choice, so reverting speed also
      // removes a destination change that was only needed by the faster speed.
      if (this.speedDestinationBeforeAuto) {
        const previous = this.speedDestinationBeforeAuto.staged;
        if (previous === undefined) delete nextStaged.destination;
        else nextStaged.destination = previous;
        this.speedDestinationBeforeAuto = null;
      }
      const current = Number(nextStaged.destination ?? this.state.liveValues.destination?.value);
      const choices = this.state.schema.find((s) => s.name === "destination")?.choices;
      const compatible = destinationForSpeed(model, Number(value), current, choices);
      if (compatible === undefined) {
        this.speedDestinationBeforeAuto = priorAuto;
        this.update({ errorBanner: `No supported destination permits ${value} km/h.` });
        return;
      }
      if (compatible !== current) {
        this.speedDestinationBeforeAuto = { staged: nextStaged.destination };
        stage("destination", compatible);
      }
    } else if (name === "destination") {
      this.speedDestinationBeforeAuto = null;
      const range = destinationSpeedRange(model, Number(value));
      const speed = Number(nextStaged["speed-limit"] ?? this.state.liveValues["speed-limit"]?.value);
      if (range && Number.isFinite(speed)) {
        stage("speed-limit", Math.min(range.max, Math.max(range.min, speed)));
      }
    }
    stage(name, value);
    this.update({ stagedValues: nextStaged, errorBanner: null });
  }

  public revertParameter(name: string): void {
    const live = this.state.liveValues[name]?.value;
    if ((name === "speed-limit" || name === "destination") && live !== undefined && live !== null) {
      this.stageParameter(name, live);
      return;
    }
    const nextStaged = { ...this.state.stagedValues };
    delete nextStaged[name];
    this.update({ stagedValues: nextStaged });
  }

  public discardAllStaged(): void {
    this.speedDestinationBeforeAuto = null;
    this.update({ stagedValues: {}, errorBanner: null });
  }

  public applyPresetMacro(preset: "eu_commute" | "trail_85nm" | "cargo_utility"): void {
    const nextStaged = { ...this.state.stagedValues };
    const stageIfPresent = (name: string, val: number | string) => {
      if (this.state.schema.length === 0 || this.state.schema.some((s) => s.name === name)) {
        const live = this.state.liveValues[name]?.value;
        if (String(live) === String(val)) {
          delete nextStaged[name];
        } else {
          nextStaged[name] = val;
        }
      }
    };

    if (preset === "eu_commute") {
      this.speedDestinationBeforeAuto = null;
      stageIfPresent("destination", this.state.identity?.modelSlug === "ep800-crg" ? 3 : 3);
      stageIfPresent("speed-limit", 25);
      stageIfPresent("tire-circumference", 2280);
      stageIfPresent("lamp", 1);
      stageIfPresent("walk-assist", 2);
    } else if (preset === "trail_85nm") {
      stageIfPresent("profile1.boost.torque", 85);
      stageIfPresent("profile1.trail.torque", 63);
      stageIfPresent("profile1.eco.torque", 34);
    } else if (preset === "cargo_utility") {
      stageIfPresent("tire-circumference", 2150);
      stageIfPresent("lamp", 1);
      stageIfPresent("walk-assist", 2);
      stageIfPresent("profile1.trail.torque", 70);
    }

    this.update({ stagedValues: nextStaged });
  }

  /**
   * Executes all staged parameter changes in a single atomic WASM `settings batch` session.
   * Automatically creates pre-state backup snapshots and journals every operation.
   */
  public async commitStagedBatch(): Promise<any> {
    if (this.syncInProgress) {
      await this.syncInProgress.catch(() => {});
    }
    const { transport, identity, stagedValues } = this.state;
    if (!transport || !identity) {
      throw new Error("No active drive unit session.");
    }
    const entries = Object.entries(stagedValues);
    if (entries.length === 0) return null;

    this.update({
      phase: "committing",
      errorBanner: null,
      statusMessage: `Writing ${entries.length} parameter${entries.length > 1 ? "s" : ""} with automatic backup...`,
    });

    try {
      // Ensure destination is ordered before speed-limit so ceiling checks use the updated destination
      const sortedEntries = [...entries].sort(([a], [b]) => {
        if (a === "destination") return -1;
        if (b === "destination") return 1;
        return 0;
      });

      const operations = sortedEntries.map(([name, value]) => ({
        op: "set" as const,
        name,
        value: String(value),
      }));

      const stamp = new Date().toISOString().replace(/[-:T.Z]/g, "").slice(0, 14) + "-" + Math.random().toString(36).slice(2, 6);
      const planFile = `tx_${stamp}.plan.json`;
      const backupPrefix = `tx_${stamp}`;

      await this.terminal!.files.import(
        planFile,
        new TextEncoder().encode(JSON.stringify(operations, null, 2)),
      );

      const flags = this.buildSessionFlags(transport, identity, true);
      const batchRes = await this.exec(
        `${flags}settings batch '${planFile}' --backup-prefix '${backupPrefix}'`,
      );

      if (batchRes.code !== 0) {
        throw new Error(batchRes.stderr || `Batch write failed (${batchRes.code})`);
      }

      const resultJson = this.extractLastJsonObject(batchRes.stdout);

      // Update liveValues from verified readbacks
      const nextLive = { ...this.state.liveValues };
      if (resultJson?.results && Array.isArray(resultJson.results)) {
        for (const r of resultJson.results) {
          if (r && r.setting !== undefined && r.after !== undefined) {
            nextLive[r.setting] = {
              value: r.after,
              raw: nextLive[r.setting]?.raw || "",
            };
          }
        }
      } else {
        for (const [k, v] of sortedEntries) {
          nextLive[k] = { value: v, raw: nextLive[k]?.raw || "" };
        }
      }

      this.refreshSnapshots();
      this.speedDestinationBeforeAuto = null;
      this.update({
        phase: "ready",
        liveValues: nextLive,
        stagedValues: {},
        statusMessage: `Committed ${entries.length} change${entries.length > 1 ? "s" : ""} (${backupPrefix}).`,
      });

      return resultJson;
    } catch (err: any) {
      const msg = err?.message || String(err);
      this.refreshSnapshots();
      this.update({
        phase: "ready",
        errorBanner: msg,
        statusMessage: `Transaction halted: ${msg}`,
      });
      throw err;
    }
  }

  public async captureManualSnapshot(): Promise<string> {
    if (this.syncInProgress) {
      await this.syncInProgress.catch(() => {});
    }
    const { transport, identity } = this.state;
    if (!transport || !identity) {
      throw new Error("Connect to a drive unit to capture a snapshot.");
    }
    this.update({
      phase: "reading",
      errorBanner: null,
      statusMessage: "Capturing full configuration snapshot...",
    });
    try {
      const stamp = new Date().toISOString().replace(/[-:T.Z]/g, "").slice(0, 14);
      const filename = `snapshot_${identity.serial}_${stamp}.json`;
      const flags = this.buildSessionFlags(transport, identity, false);
      const res = await this.exec(`${flags}settings read --output '${filename}'`);
      if (res.code !== 0) {
        throw new Error(res.stderr || `Snapshot failed (${res.code})`);
      }
      this.refreshSnapshots();
      this.update({
        phase: "ready",
        statusMessage: `Saved full snapshot ${filename}.`,
      });
      return filename;
    } catch (err: any) {
      const msg = err?.message || String(err);
      this.update({
        phase: "error",
        errorBanner: msg,
        statusMessage: msg,
      });
      throw err;
    }
  }

  public async restoreBackupFile(filename: string): Promise<void> {
    if (this.syncInProgress) {
      await this.syncInProgress.catch(() => {});
    }
    const { transport, identity } = this.state;
    if (!transport || !identity) {
      throw new Error("Connect to the target drive unit before restoring a backup.");
    }
    this.update({
      phase: "committing",
      errorBanner: null,
      statusMessage: `Restoring ${filename}...`,
    });
    try {
      const stamp = Date.now();
      const rollbackFile = `pre_restore_${stamp}.backup.json`;
      const flags = this.buildSessionFlags(transport, identity, true);
      const res = await this.exec(
        `${flags}settings restore '${filename}' --backup '${rollbackFile}'`,
      );
      if (res.code !== 0) {
        throw new Error(res.stderr || `Restore failed (${res.code})`);
      }
      await this.readAllParameters(transport, identity);
      this.refreshSnapshots();
      this.update({
        phase: "ready",
        statusMessage: `Restored ${filename} (previous value backed up to ${rollbackFile}).`,
      });
    } catch (err: any) {
      const msg = err?.message || String(err);
      this.update({
        phase: "error",
        errorBanner: msg,
        statusMessage: msg,
      });
      throw err;
    }
  }

  public refreshSnapshots(): void {
    if (!this.terminal?.files) return;
    const rawList = this.terminal.files.list() || [];
    const snapshots: JournalSnapshot[] = [];
    const decoder = new TextDecoder();

    for (const item of rawList) {
      const path = typeof item === "string" ? item : item.path;
      if (!path || path.endsWith(".plan.json")) continue;

      let bytes: Uint8Array<ArrayBufferLike> = new Uint8Array();
      try {
        bytes = this.terminal.files.bytes(path);
      } catch {
        continue;
      }

      let kind: JournalSnapshot["kind"] = "journal";
      let serial: string | undefined;
      let summary = `${(bytes.length / 1024).toFixed(1)} KiB`;

      if (path.endsWith(".dat") || path.endsWith(".bin")) {
        kind = "firmware";
        summary = `Firmware image (${(bytes.length / 1024).toFixed(1)} KiB)`;
      } else if (path.endsWith(".json")) {
        try {
          const parsed = JSON.parse(decoder.decode(bytes));
          if (parsed.format === "cranklink-setting-backup-v1" || parsed.format === "esteps-setting-backup-v1") {
            kind = "setting-backup";
            serial = parsed.serial;
            summary = `${parsed.setting}: ${JSON.stringify(parsed.before)}`;
          } else if (parsed.values) {
            kind = "full-snapshot";
            serial = parsed.serial;
            summary = `Full configuration (${Object.keys(parsed.values).length} parameters)`;
          }
        } catch {
          // ignore parse error
        }
      } else if (path.endsWith(".jsonl")) {
        kind = "journal";
        const lines = decoder.decode(bytes).trim().split("\n").filter(Boolean);
        summary = `Transaction journal (${lines.length} events)`;
      }

      snapshots.push({
        filename: path,
        kind,
        timestamp: path.match(/\d{8,14}/)?.[0] || "stored",
        serial,
        summary,
        sizeBytes: bytes.length,
      });
    }

    this.update({ snapshots: snapshots.reverse() });
  }

  /**
   * Synchronized Dual-MCU Firmware Ingestion:
   * Accepts one or more firmware files (.dat), computes SHA-256 digests,
   * runs `inspect` in WASM, pairs DCAS (-D) and RX (-M) automatically,
   * and validates companion compatibility when both are present.
   */
  public async importFirmwarePackageFiles(files: FileList | File[]): Promise<void> {
    if (!this.terminal?.files) {
      throw new Error("Virtual storage not initialized.");
    }

    let dcasFile = this.state.firmware.dcasFile;
    let rxFile = this.state.firmware.rxFile;

    for (const file of Array.from(files)) {
      const bytes = new Uint8Array(await file.arrayBuffer());
      const hashBuffer = await crypto.subtle.digest("SHA-256", bytes);
      const sha256 = Array.from(new Uint8Array(hashBuffer))
        .map((b) => b.toString(16).padStart(2, "0"))
        .join("");

      try {
        await this.terminal.files.import(file.name, bytes);
      } catch (err: any) {
        if (!String(err?.message).includes("already exists")) {
          throw err;
        }
      }

      const inspectRes = await this.exec(`firmware inspect '${file.name}'`);
      let metadata = null;
      let inspectErr: string | undefined;
      if (inspectRes.code === 0) {
        metadata = this.extractLastJsonObject(inspectRes.stdout);
      } else {
        inspectErr = inspectRes.stderr || "Invalid firmware image format";
      }

      const role = classifyFirmwareRole(file.name, metadata);
      const inspected: InspectedFirmwareFile = {
        filename: file.name,
        sizeBytes: bytes.length,
        sha256,
        role,
        metadata,
        error: inspectErr,
      };

      if (role === "dcas") {
        dcasFile = inspected;
      } else if (role === "rx") {
        rxFile = inspected;
      } else if (!dcasFile) {
        dcasFile = inspected;
      } else {
        rxFile = inspected;
      }
    }

    // If both D and M are loaded, run WASM companion cross-validation
    let companionEval = evaluateCompanionPair(dcasFile, rxFile);
    if (dcasFile && rxFile && !dcasFile.error && !rxFile.error) {
      const pairCheck = await this.exec(
        `firmware inspect '${dcasFile.filename}' --companion '${rxFile.filename}'`,
      );
      if (pairCheck.code !== 0) {
        companionEval = {
          valid: false,
          message: pairCheck.stderr || "Companion validation rejected by WASM firmware guard.",
        };
      }
    }

    this.refreshSnapshots();
    this.update({
      firmware: {
        ...this.state.firmware,
        dcasFile,
        rxFile,
        companionValid: companionEval.valid,
        companionMessage: companionEval.message,
        dryRunPassed: false,
        dryRunSerial: null,
      },
    });
  }

  public async executeFirmwareSync(
    write: boolean,
    overrideSingleTarget?: "dcas" | "rx" | null,
  ): Promise<void> {
    const { transport, identity, firmware } = this.state;
    if (!transport || !identity) {
      throw new Error("Connect to a drive unit before running firmware operations.");
    }
    if (!firmware.dcasFile || !firmware.rxFile || !firmware.companionValid) {
      throw new Error("Load a validated DCAS (-D) + RX (-M) firmware companion pair first.");
    }
    if (write && (!firmware.dryRunPassed || firmware.dryRunSerial !== identity.serial)) {
      throw new Error("Complete a non-erasing compatibility check on this drive unit before programming.");
    }

    const prefix = `fw_${write ? "write" : "check"}_${Date.now()}`;
    const plan = buildSynchronizedFlashPlan({
      transport,
      serial: identity.serial,
      dcas: firmware.dcasFile,
      rx: firmware.rxFile,
      write,
      prefix,
      overrideSingleTarget,
    });

    this.update({
      phase: "flashing",
      errorBanner: null,
      statusMessage: plan.description,
    });

    try {
      let lastOutput: any = null;
      for (const cmd of plan.commands) {
        const res = await this.exec(cmd);
        if (res.code !== 0) {
          throw new Error(res.stderr || `Firmware command failed (${res.code})`);
        }
        lastOutput = this.extractLastJsonObject(res.stdout) || res.stdout;
      }

      this.refreshSnapshots();
      this.update({
        phase: "ready",
        statusMessage: write
          ? "Synchronized firmware programming and verification complete."
          : "Non-erasing dual-MCU compatibility check passed. Ready to program.",
        firmware: {
          ...this.state.firmware,
          dryRunPassed: !write ? true : false,
          dryRunSerial: !write ? identity.serial : null,
          lastFlashReceipt: lastOutput,
        },
      });
    } catch (err: any) {
      const msg = err?.message || String(err);
      this.refreshSnapshots();
      this.update({
        phase: "error",
        errorBanner: msg,
        statusMessage: `Firmware operation stopped: ${msg}`,
        firmware: {
          ...this.state.firmware,
          dryRunPassed: false,
          dryRunSerial: null,
        },
      });
      throw err;
    }
  }

  public downloadVirtualFile(filename: string): void {
    this.exec(`download '${filename}'`).catch(() => {});
  }

  public setSection(section: WorkspaceSection): void {
    this.update({ activeSection: section });
  }

  public setParameterGroup(group: SettingSectionId | "all"): void {
    this.update({ activeParameterGroup: group });
  }

  public setProfileTab(tab: 1 | 2): void {
    this.update({ activeProfileTab: tab });
  }

  public setSearchFilter(query: string): void {
    this.update({ searchFilter: query });
  }

  public toggleConsole(expanded?: boolean): void {
    this.update({
      consoleExpanded: expanded !== undefined ? expanded : !this.state.consoleExpanded,
    });
  }

  public setSimulatorConfig(patch: Partial<SimulatorOptions>): void {
    const next = { ...this.state.simulatorOptions, ...patch };
    this.update({ simulatorOptions: next });
    if (this.state.transport === "sim" && patch.fault && this.terminal?.simulator?.setFault) {
      this.terminal.simulator.setFault(patch.fault);
    }
  }

  public async runRawCommand(cmd: string): Promise<void> {
    if (!cmd.trim()) return;
    await this.exec(cmd);
    this.refreshSnapshots();
  }

  public requestMcpApproval(
    tool: string,
    params: any,
    diff: Array<{ name: string; before: any; after: any }>,
  ): Promise<boolean> {
    return new Promise((resolve, reject) => {
      const enrichedDiff = diff.map((d) => {
        const spec = this.state.schema.find((s) => s.name === d.name);
        return {
          name: d.name,
          label: spec?.label || d.name,
          before: spec ? formatSettingValue(spec, d.before) : d.before,
          after: spec ? formatSettingValue(spec, d.after) : d.after,
        };
      });
      this.update({
        mcpApproval: {
          id: Math.random().toString(36).slice(2, 10),
          tool,
          params,
          diff: enrichedDiff,
          resolve: (ok: boolean) => {
            this.update({ mcpApproval: null });
            resolve(ok);
          },
          reject: (err: Error) => {
            this.update({ mcpApproval: null });
            reject(err);
          },
        },
      });
    });
  }

  public respondMcpApproval(approved: boolean): void {
    if (this.state.mcpApproval) {
      this.state.mcpApproval.resolve(approved);
    }
  }

  private extractLastJsonObject(text: string): any | null {
    const trimmed = text.trim();
    if (!trimmed) return null;
    try {
      return JSON.parse(trimmed);
    } catch {
      // Find first `{` or `[` that parses cleanly to the end
      for (let i = 0; i < trimmed.length; i++) {
        if (trimmed[i] === "{" || trimmed[i] === "[") {
          try {
            return JSON.parse(trimmed.slice(i));
          } catch {
            // continue scanning
          }
        }
      }
    }
    return null;
  }
}
