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
  MotorModel,
  SimulatorOptions,
  DeviceIdentity,
  SettingEntry,
  JournalSnapshot,
  InspectedFirmwareFile,
  OperationFailure,
  OperationKind,
} from "../types.ts";
import { enrichSchema, formatSettingValue, destinationForSpeed, destinationSpeedRange } from "../domain/schema.ts";
import { formatSetting } from "../domain/settingFormat.ts";
import { isLinkLost } from "../domain/errors.ts";
import { detectMotorIdentity, isModernModel } from "../domain/motorIdentity.ts";
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
  private toastCounter = 0;
  private activeCapture: { stdout: string; stderr: string; echo: string } | null = null;
  private execQueue: Promise<any> = Promise.resolve();
  private syncInProgress: Promise<DeviceIdentity> | null = null;
  private speedDestinationBeforeAuto: { staged: number | string | undefined } | null = null;

  constructor() {
    this.state = {
      phase: "disconnected",
      statusMessage: "Select a physical adapter or start the simulator.",
      errorBanner: null,
      failure: null,
      linkLost: false,
      toast: null,
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

  /** Record a failure with enough context for the UI to explain the bike's state. */
  private fail(kind: OperationKind, err: any, patch: Partial<InstrumentState> = {}, settings?: string[]): string {
    const msg = err?.message || String(err);
    const failure: OperationFailure = settings ? { kind, settings } : { kind };
    this.update({ errorBanner: msg, failure, linkLost: isLinkLost(msg), ...patch });
    return msg;
  }

  private startOperation(patch: Partial<InstrumentState>): void {
    this.update({ errorBanner: null, failure: null, toast: null, ...patch });
  }

  private notify(message: string, action?: "backup"): void {
    this.update({ toast: { id: ++this.toastCounter, message, ...(action ? { action } : {}) } });
  }

  public dismissToast(): void {
    if (this.state.toast) this.update({ toast: null });
  }

  public dismissError(): void {
    this.update({ errorBanner: null, failure: null });
  }

  public attachTerminal(term: TerminalHost): void {
    this.terminal = term;
    if (typeof window !== "undefined") {
      const prevPrint = (window as any).estepsTerminalPrint;
      (window as any).estepsTerminalPrint = (text: string, stream: "stdout" | "stderr" | "notice" = "stdout") => {
        if (prevPrint) prevPrint(text, stream);
        if (this.activeCapture && stream !== "notice") {
          this.activeCapture[stream] += text;
          // exec() already logged this command; the host echoes it to stdout.
          if (stream === "stdout" && text.trim() === this.activeCapture.echo) return;
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
    // Progress notices update the connection status; standing disclaimers do not.
    const isProgress = stream === "notice" && !/^Experimental /.test(trimmed);
    this.update({
      logs: nextLogs,
      ...(isProgress && ["connecting", "probing"].includes(this.state.phase)
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
      const echo = `cranklink> ${commandLine}`;
      this.appendLog("cmd", echo);
      const capture = { stdout: "", stderr: "", echo };
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

    this.startOperation({
      phase: "connecting",
      transport,
      linkLost: false,
      simulatorOptions: mergedSim,
      statusMessage:
        transport === "sim"
          ? `Starting simulated ${mergedSim.model.toUpperCase()}…`
          : transport === "ble"
            ? "Choose your bike in the browser’s Bluetooth list…"
            : `Opening ${transport.toUpperCase()} adapter…`,
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
        phase: "probing",
        statusMessage: "Identifying drive unit and firmware versions…",
      });

      let initialSnapshot: any = null;
      let identity: DeviceIdentity;
      if (transport === "ble") {
        this.update({
          phase: "reading",
          statusMessage: "Identifying drive unit and reading settings…",
        });
        const result = await this.exec("--ble-master=master settings read");
        if (result.code !== 0) {
          throw new Error(result.stderr || `Settings read failed (${result.code})`);
        }
        initialSnapshot = this.extractLastJsonObject(result.stdout);
        if (!initialSnapshot?.identity || !initialSnapshot?.values) {
          throw new Error("Settings response lacks drive-unit identity or parameters. Reload the updated application before reconnecting.");
        }
        identity = this.parseIdentity(initialSnapshot.identity);
      } else {
        identity = await this.probeIdentity();
      }

      this.update({
        identity,
        phase: "reading",
        statusMessage: `Reading ${identity.model} settings…`,
      });

      const schema = await this.loadSchema(identity.modelSlug);
      this.update({ schema });

      if (initialSnapshot) {
        this.applyParameterSnapshot(initialSnapshot, identity);
      } else {
        await this.readAllParameters(transport, identity);
      }
      this.refreshSnapshots();

      if (this.state.phase === "reading" || this.state.phase === "probing") {
        this.update({
          phase: "ready",
          statusMessage: `Synced with ${identity.model} (${Object.keys(this.state.liveValues).length} parameters read).`,
        });
      }

      return identity;
    } catch (err: any) {
      const msg = this.fail("connect", err, { phase: "error" });
      this.update({ statusMessage: `Handshake failed: ${msg}` });
      throw err;
    }
  }

  /**
   * Reopen the session after the link closed, keeping staged edits when the
   * same drive unit answers.
   */
  public async reconnect(): Promise<void> {
    const { transport, identity, stagedValues, simulatorOptions } = this.state;
    if (!transport) throw new Error("No previous connection to reopen.");
    await this.disconnect().catch(() => {});
    const next = await this.connectAndSync(transport, transport === "sim" ? simulatorOptions : undefined);
    if (identity && next.serial === identity.serial && Object.keys(stagedValues).length > 0) {
      for (const [name, value] of Object.entries(stagedValues)) this.stageParameter(name, value);
    }
  }


  private async probeIdentity(): Promise<DeviceIdentity> {
    const probeRes = await this.exec("probe");
    if (probeRes.code !== 0) {
      throw new Error(probeRes.stderr || `Probe failed (${probeRes.code})`);
    }

    const parsed = this.extractLastJsonObject(probeRes.stdout);
    if (!parsed) {
      throw new Error("Could not parse JSON identity from probe output.");
    }

    return this.parseIdentity(parsed);
  }

  private parseIdentity(parsed: any): DeviceIdentity {
    if (!/^[0-9a-f]{12}$/i.test(String(parsed.serial || ""))) {
      throw new Error("Drive-unit response lacks a verified serial identity.");
    }
    return {
      slot: parsed.slot ?? 13,
      ...detectMotorIdentity(parsed),
      serial: String(parsed.serial).toUpperCase(),
      dcasVersion: String(parsed.dcas_version || "—"),
      rxVersion: String(parsed.rx_version || "—"),
      modelSeries: parsed.model_series,
    };
  }

  private async loadSchema(modelSlug: MotorModel) {
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
      this.startOperation({
        phase: "reading",
        statusMessage: "Reading settings from the drive unit…",
      });
    }

    try {
      const prefixFlags = this.buildSessionFlags(transport, identity, false);
      const readRes = await this.exec(`${prefixFlags}settings read`);
      if (readRes.code !== 0) {
        throw new Error(readRes.stderr || `Settings read failed (${readRes.code})`);
      }

      const snapshot = this.extractLastJsonObject(readRes.stdout);
      const values = this.applyParameterSnapshot(snapshot, identity);
      this.update({ linkLost: false });
      return values;
    } catch (err: any) {
      const msg = this.fail("read", err, { phase: wasReady ? "ready" : "error" });
      this.update({ statusMessage: `Read error: ${msg}` });
      throw err;
    }
  }

  private applyParameterSnapshot(snapshot: any, identity: DeviceIdentity): Record<string, SettingEntry> {
    if (!snapshot?.values || typeof snapshot.values !== "object" || Array.isArray(snapshot.values)) {
      throw new Error("Drive-unit response lacks a valid settings snapshot.");
    }
    const liveValues: Record<string, SettingEntry> = {};
    const unavailableValues: Record<string, string> = {};
    for (const [key, entry] of Object.entries<any>(snapshot.values)) {
      liveValues[key] = { value: entry?.value ?? null, raw: entry?.raw ?? "" };
    }
    for (const [key, reason] of Object.entries<any>(snapshot.unavailable || {})) {
      unavailableValues[key] = String(reason);
    }
    this.update({
      liveValues,
      unavailableValues,
      ...(this.state.phase === "reading" ? {
        phase: "ready" as const,
        statusMessage: `Synced with ${identity.model} (${Object.keys(liveValues).length} parameters read).`,
      } : {}),
    });
    return liveValues;
  }

  private buildSessionFlags(
    transport: TransportType,
    identity: DeviceIdentity,
    requiresUnitAuth: boolean = false,
  ): string {
    const cleanSerial = identity.serial.replace(/[-:\s]/g, "");
    if (isModernModel(identity.modelSlug) && (transport === "ble" || transport === "sim" || requiresUnitAuth)) {
      return `--expect-serial ${cleanSerial} --ble-unit-auth `;
    }
    return `--expect-serial ${cleanSerial} `;
  }

  public async disconnect(): Promise<void> {
    let cleanupError: string | null = null;
    try {
      if (this.terminal) {
        const result = await this.exec("disconnect");
        if (result.code !== 0) {
          throw new Error(result.stderr || `Bluetooth cleanup failed (${result.code}). Check the bike before reconnecting.`);
        }
      }
    } catch (err: any) {
      cleanupError = err?.message || String(err);
      throw err;
    } finally {
      this.speedDestinationBeforeAuto = null;
      this.update({
        phase: "disconnected",
        transport: null,
        identity: null,
        liveValues: {},
        unavailableValues: {},
        stagedValues: {},
        errorBanner: cleanupError,
        failure: cleanupError ? { kind: "disconnect" } : null,
        linkLost: false,
        toast: null,
        statusMessage: cleanupError ? "Disconnected, but bike cleanup was not confirmed." : "Disconnected.",
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

  /**
   * Executes all staged parameter changes in a single atomic WASM `settings batch` session.
   * Automatically creates pre-state backup snapshots and journals every operation.
   */
  public async commitStagedBatch(): Promise<any> {
    if (this.syncInProgress) {
      await this.syncInProgress.catch(() => {});
    }
    if (this.state.phase === "committing") {
      throw new Error("A save is already running. Wait for it to finish.");
    }
    const { transport, identity, stagedValues } = this.state;
    if (!transport || !identity) {
      throw new Error("No active drive unit session.");
    }
    const entries = Object.entries(stagedValues);
    if (entries.length === 0) return null;

    // Destination is written before speed-limit so ceiling checks use the updated destination.
    const sortedEntries = [...entries].sort(([a], [b]) => {
      if (a === "destination") return -1;
      if (b === "destination") return 1;
      return 0;
    });
    const plural = entries.length > 1 ? "s" : "";

    this.startOperation({
      phase: "committing",
      statusMessage: `Backing up and writing ${entries.length} setting${plural}…`,
    });

    try {
      const operations = sortedEntries.map(([name, value]) => ({
        op: "set" as const,
        name,
        value: String(value),
      }));

      const stamp = `${fileStamp()}-${Math.random().toString(36).slice(2, 6)}`;
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
        linkLost: false,
        statusMessage: `Committed ${entries.length} change${plural} (${backupPrefix}).`,
      });
      this.notify(`Saved ${entries.length} setting${plural} to the bike. A backup of the previous values was created.`, "backup");

      return resultJson;
    } catch (err: any) {
      this.refreshSnapshots();
      const msg = this.fail("save", err, { phase: "ready" }, sortedEntries.map(([name]) => name));
      this.update({ statusMessage: `Transaction halted: ${msg}` });
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
    this.startOperation({
      phase: "reading",
      statusMessage: "Reading every setting into a backup…",
    });
    try {
      const filename = `snapshot_${identity.serial}_${fileStamp()}.json`;
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
      this.notify("Full backup saved in this browser.");
      return filename;
    } catch (err: any) {
      const msg = this.fail("capture", err, { phase: "ready" });
      this.update({ statusMessage: msg });
      throw err;
    }
  }

  /**
   * Restore one save's backups in a single pass. Each file creates its own
   * rollback backup first, then settings are read once to show the result.
   */
  public async restoreBackupFiles(filenames: string[]): Promise<void> {
    if (this.syncInProgress) {
      await this.syncInProgress.catch(() => {});
    }
    const { transport, identity } = this.state;
    if (!transport || !identity) {
      throw new Error("Connect to the target drive unit before restoring a backup.");
    }
    if (filenames.length === 0) return;
    this.startOperation({
      phase: "committing",
      statusMessage: `Restoring ${filenames.length} backup file${filenames.length > 1 ? "s" : ""}…`,
    });
    try {
      const stamp = fileStamp();
      const flags = this.buildSessionFlags(transport, identity, true);
      for (const [index, filename] of filenames.entries()) {
        const rollbackFile = `pre_restore_${stamp}.${String(index + 1).padStart(3, "0")}.backup.json`;
        const res = await this.exec(`${flags}settings restore '${filename}' --backup '${rollbackFile}'`);
        if (res.code !== 0) {
          throw new Error(res.stderr || `Restore failed (${res.code})`);
        }
      }
      await this.readAllParameters(transport, identity);
      this.refreshSnapshots();
      this.update({
        phase: "ready",
        statusMessage: `Restored ${filenames.join(", ")}.`,
      });
      this.notify("Restored the backup. The values it replaced were backed up first.", "backup");
    } catch (err: any) {
      this.refreshSnapshots();
      const msg = this.fail("restore", err, { phase: "ready" });
      this.update({ statusMessage: msg });
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
      let setting: string | undefined;
      let before: number | string | null = null;
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
            setting = String(parsed.setting);
            before = parsed.before ?? null;
            summary = `${formatSetting(this.state.schema, setting, before)} before the change`;
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
        summary = `Operation log (${lines.length} events)`;
      }

      snapshots.push({
        filename: path,
        kind,
        group: snapshotGroup(path),
        createdAt: parseFileStamp(path),
        serial,
        summary,
        ...(setting !== undefined ? { setting, before } : {}),
        sizeBytes: bytes.length,
      });
    }

    snapshots.sort((a, b) => (b.createdAt?.getTime() ?? 0) - (a.createdAt?.getTime() ?? 0));
    this.update({ snapshots });
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

    this.startOperation({
      phase: "flashing",
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
      this.refreshSnapshots();
      const msg = this.fail("firmware", err, { phase: "ready" });
      this.update({
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

/** UTC timestamp used in every file this session writes: YYYYMMDDHHMMSS. */
function fileStamp(date = new Date()): string {
  return date.toISOString().replace(/[-:T.Z]/g, "").slice(0, 14);
}

function parseFileStamp(path: string): Date | null {
  const match = path.match(/(\d{4})(\d{2})(\d{2})(\d{2})(\d{2})(\d{2})/);
  if (!match) return null;
  const [, y, mo, d, h, mi, se] = match.map(Number);
  const date = new Date(Date.UTC(y, mo - 1, d, h, mi, se));
  return Number.isNaN(date.getTime()) ? null : date;
}

/** Files from one operation share the prefix before their per-setting index. */
function snapshotGroup(path: string): string {
  return path.replace(/(?:\.\d{3})?\.(?:backup\.json|journal\.jsonl|plan\.json)$/, "");
}
