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
  SettingsModel,
  ComponentUnit,
  SimulatorOptions,
  DeviceIdentity,
  SettingEntry,
  JournalSnapshot,
  InspectedFirmwareFile,
  OperationFailure,
  OperationKind,
} from "../types.ts";
import { enrichSchema, destinationForSpeed, destinationSpeedRange, settingAcceptsValue } from "../domain/schema.ts";
import { formatValue, formatSetting, settingLabel } from "../domain/settingFormat.ts";
import { isLinkLost } from "../domain/errors.ts";
import { CATALOG_NAME, detectDeviceIdentity, isMotorStock, isModernModel } from "../domain/motorIdentity.ts";
import {
  classifyFirmwareRole,
  evaluateCompanionPair,
  buildSynchronizedFlashPlan,
  shellQuote,
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
  /** Registers how the host asks the operator for the Bluetooth passkey. */
  setPasskeyPrompt: (prompt: (signal: AbortSignal) => Promise<string | null>) => void;
  simulator?: any;
  configureSimulator?: (opts: any) => void;
}

/** The same physical component: serial, schema, slot and stock identity all match. */
function sameComponent(a: DeviceIdentity | null, b: DeviceIdentity | null): boolean {
  return !!a && !!b && a.serial === b.serial && a.modelSlug === b.modelSlug && a.slot === b.slot
    && a.modelSeries === b.modelSeries && a.modelUnit === b.modelUnit;
}

function sameBytes(a: Uint8Array, b: Uint8Array): boolean {
  return a.length === b.length && a.every((byte, i) => byte === b[i]);
}

export class SessionController {
  private state: InstrumentState;
  private listeners: Set<(state: InstrumentState) => void> = new Set();
  private terminal: TerminalHost | null = null;
  private logCounter = 0;
  private toastCounter = 0;
  private activeCapture: { stdout: string; stderr: string; echo: string } | null = null;
  private execQueue: Promise<any> = Promise.resolve();
  private pendingPasskey: ((value: string | null) => void) | null = null;
  private syncInProgress: Promise<DeviceIdentity | null> | null = null;
  private speedDestinationBeforeAuto: { staged: number | string | undefined } | null = null;
  /** True while disconnect() runs; agent reviews are refused until it settles. */
  private disconnecting = false;

  constructor() {
    this.state = {
      phase: "disconnected",
      statusMessage: "Select a physical adapter or start the simulator.",
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
      units: [],
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
      passkeyPromptOpen: false,
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

  /**
   * Record a failure with enough context for the UI to explain the bike's
   * state. `status` builds the status line from the error text.
   */
  private fail(
    kind: OperationKind,
    err: unknown,
    patch: Partial<InstrumentState> & { status?: (error: string) => string; settings?: string[] } = {},
  ): void {
    const error = err instanceof Error ? err.message : String(err);
    const { status, settings, ...rest } = patch;
    const failure: OperationFailure = settings ? { kind, error, settings } : { kind, error };
    this.update({
      failure,
      // Sticky: an unrelated rejection (e.g. staging while offline) must not
      // mark a dead link healthy. Connecting or a successful read clears it.
      linkLost: this.state.linkLost || isLinkLost(error),
      statusMessage: status ? status(error) : error,
      ...rest,
    });
  }

  private startOperation(patch: Partial<InstrumentState>): void {
    this.update({ failure: null, toast: null, ...patch });
  }

  private notify(message: string, action?: "backup"): void {
    this.update({ toast: { id: ++this.toastCounter, message, ...(action ? { action } : {}) } });
  }

  public dismissToast(): void {
    if (this.state.toast) this.update({ toast: null });
  }

  public dismissError(): void {
    this.update({ failure: null });
  }

  public attachTerminal(term: TerminalHost): void {
    this.terminal = term;
    term.setPasskeyPrompt((signal) => this.requestPasskey(signal));
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

  /** Ask the operator for the Bluetooth passkey; resolves null when cancelled or aborted. */
  public requestPasskey(signal: AbortSignal): Promise<string | null> {
    if (this.pendingPasskey) return Promise.reject(new Error("A passkey prompt is already open"));
    return new Promise((resolve) => {
      const settle = (value: string | null) => {
        signal.removeEventListener("abort", onAbort);
        this.pendingPasskey = null;
        this.update({ passkeyPromptOpen: false });
        resolve(value);
      };
      const onAbort = () => settle(null);
      if (signal.aborted) return resolve(null);
      signal.addEventListener("abort", onAbort, { once: true });
      this.pendingPasskey = settle;
      this.update({ passkeyPromptOpen: true });
    });
  }

  /** Answer the open passkey prompt; null cancels it. */
  public answerPasskey(value: string | null): void {
    this.pendingPasskey?.(value);
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
  ): Promise<DeviceIdentity | null> {
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
  ): Promise<DeviceIdentity | null> {
    const mergedSim: SimulatorOptions = {
      ...this.state.simulatorOptions,
      ...simOpts,
    };

    this.state.mcpApproval?.resolve(false);
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
        statusMessage: "Discovering connected components…",
      });
      const units = await this.readInventory();
      const preferred = units.find((unit) => isMotorStock(unit.model_series, unit.model_unit))
        ?? units.find((unit) => (unit.model || unit.model_candidates.length) && ![unit.model, ...unit.model_candidates].some((name) => name?.startsWith("DU-")));
      if (!preferred) {
        this.update({ phase: "ready", statusMessage: "Connected. Select a catalogued component from the inventory." });
        return null;
      }
      // An unsupported target does not discard a successful connection or its
      // inventory: the operator can choose another component.
      return await this.doSelectComponent(preferred).catch(() => null);
    } catch (err: any) {
      this.fail("connect", err, { phase: "error", status: (error) => `Handshake failed: ${error}` });
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
    let next = await this.connectAndSync(transport, transport === "sim" ? simulatorOptions : undefined);
    const samePrevious = (candidate: DeviceIdentity | null) => !!identity && !!candidate
      && candidate.serial === identity.serial && candidate.modelSlug === identity.modelSlug
      && candidate.modelSeries === identity.modelSeries && candidate.modelUnit === identity.modelUnit;
    if (identity && !samePrevious(next)) {
      const candidates = this.state.units.filter((unit) => unit.model_series === identity.modelSeries && unit.model_unit === identity.modelUnit
        && (identity.isMotor || unit.model === identity.model || unit.model_candidates.includes(identity.model)))
        .sort((a, b) => Number(b.slot === identity.slot) - Number(a.slot === identity.slot));
      for (const previous of candidates) {
        if (previous.slot === next?.slot) continue;
        next = await this.selectComponent(previous.slot);
        if (samePrevious(next)) break;
      }
    }
    if (samePrevious(next) && Object.keys(stagedValues).length > 0) {
      for (const [name, value] of Object.entries(stagedValues)) this.stageParameter(name, value);
    }
  }


  private async readInventory(): Promise<ComponentUnit[]> {
    const result = await this.exec(`${this.state.transport === "ble" ? "--ble-master=master " : ""}units`);
    if (result.code !== 0) throw new Error(result.stderr || "Component discovery failed.");
    const parsed = this.extractLastJsonObject(result.stdout);
    if (!Array.isArray(parsed?.units)) throw new Error("Component discovery returned no valid inventory.");
    const slots = new Set<number>();
    const units: ComponentUnit[] = parsed.units.map((unit: any) => {
      const validName = (name: unknown) => typeof name === "string" && CATALOG_NAME.test(name);
      if (!Number.isInteger(unit.slot) || unit.slot < 0 || unit.slot > 31 || slots.has(unit.slot)
          || !Number.isInteger(unit.model_series) || unit.model_series < 0 || unit.model_series > 255
          || !Number.isInteger(unit.model_unit) || unit.model_unit < 0 || unit.model_unit > 255
          || (unit.model !== null && !validName(unit.model))
          || !Array.isArray(unit.model_candidates) || !unit.model_candidates.every(validName)) {
        throw new Error("Component discovery returned an invalid identity.");
      }
      slots.add(unit.slot);
      return unit as ComponentUnit;
    });
    this.update({ units });
    return units;
  }

  public async refreshUnits(): Promise<ComponentUnit[]> {
    this.requireIdleTargetChange();
    this.startOperation({ phase: "probing", statusMessage: "Refreshing component inventory…" });
    try {
      const units = await this.readInventory();
      this.update({ phase: "ready", statusMessage: `Found ${units.length} connected components.` });
      return units;
    } catch (error) {
      this.fail("read", error, { phase: "ready" });
      throw error;
    }
  }

  private requireIdleTargetChange(): void {
    if (!this.state.transport || this.state.phase !== "ready") throw new Error("Wait for the current operation before selecting a component.");
    if (Object.keys(this.state.stagedValues).length) throw new Error("Save or discard staged changes before selecting a component.");
    if (this.state.linkLost) throw new Error("Reconnect before selecting a component.");
    if (this.state.mcpApproval) throw new Error("Finish the pending change review before selecting a component.");
  }

  public async selectComponent(slot: number): Promise<DeviceIdentity> {
    this.requireIdleTargetChange();
    const unit = this.state.units.find((entry) => entry.slot === slot);
    if (!unit || (!unit.model && !unit.model_candidates.length)) throw new Error("Select a catalogued component from the current inventory.");
    const task = this.doSelectComponent(unit);
    this.syncInProgress = task;
    try { return await task; }
    finally { if (this.syncInProgress === task) this.syncInProgress = null; }
  }

  private async doSelectComponent(unit: ComponentUnit): Promise<DeviceIdentity> {
    const transport = this.state.transport!;
    this.startOperation({ phase: "reading", statusMessage: `Reading ${unit.model || `component at slot ${unit.slot}`}…` });
    try {
      const target = `--slot ${unit.slot} ${unit.model ? `--component ${shellQuote(unit.model)} ` : ""}`;
      const modern = isMotorStock(unit.model_series, unit.model_unit) && [0x30, 0x35, 0x37].includes(unit.model_series);
      const result = await this.exec(`${transport === "ble" ? "--ble-master=master " : ""}${target}${modern ? "--ble-unit-auth " : ""}settings read`);
      if (result.code !== 0) throw new Error(result.stderr || "Component settings read failed.");
      const snapshot = this.extractLastJsonObject(result.stdout);
      const identity = this.parseIdentity(snapshot?.identity);
      if (identity.slot !== unit.slot || identity.modelSeries !== unit.model_series || identity.modelUnit !== unit.model_unit
          || (unit.model && identity.model !== unit.model)
          || (!identity.isMotor && !unit.model && unit.model_candidates.length && !unit.model_candidates.includes(identity.model))) {
        throw new Error("The component identity changed. Refresh the inventory before selecting it.");
      }
      const schema = await this.loadSchema(identity.modelSlug, identity.isMotor);
      // Validate before replacing the old target's identity and values.
      if (!snapshot?.values || typeof snapshot.values !== "object" || Array.isArray(snapshot.values)) throw new Error("Component response lacks a valid settings snapshot.");
      this.speedDestinationBeforeAuto = null;
      this.update({ identity, schema, stagedValues: {}, firmware: { ...this.state.firmware, dryRunPassed: false, dryRunSerial: null, lastFlashReceipt: null } });
      this.applyParameterSnapshot(snapshot, identity);
      this.refreshSnapshots();
      return identity;
    } catch (error) {
      this.fail("connect", error, { phase: "ready" });
      throw error;
    }
  }

  private parseIdentity(parsed: any): DeviceIdentity {
    if (!parsed || !Number.isInteger(parsed.slot) || parsed.slot < 0 || parsed.slot > 31
        || !/^[0-9a-f]{12}$/i.test(String(parsed.serial || ""))) {
      throw new Error("Component response lacks a verified serial identity.");
    }
    return {
      slot: parsed.slot,
      ...detectDeviceIdentity(parsed),
      serial: String(parsed.serial).toUpperCase(),
      dcasVersion: String(parsed.dcas_version || "—"),
      rxVersion: String(parsed.rx_version || "—"),
      modelSeries: parsed.model_series,
    };
  }

  private async loadSchema(modelSlug: SettingsModel, isMotor: boolean) {
    if (isMotor) {
      try {
        const res = await fetch(`./schemas/${modelSlug}.json`);
        if (res.ok) {
          const rawSpecs = await res.json();
          return enrichSchema(rawSpecs, modelSlug);
        }
      } catch {
        // Offline schema queries use the same Rust model gate as live reads.
      }
    }
    const schemaRes = await this.exec(`settings schema --model ${shellQuote(modelSlug)}`);
    if (schemaRes.code === 0) {
      const arr = JSON.parse(schemaRes.stdout);
      return enrichSchema(arr, modelSlug);
    }
    throw new Error(schemaRes.stderr || `Could not load the ${modelSlug} settings schema.`);
  }

  public async readAllParameters(
    transportOverride?: TransportType | null,
    identityOverride?: DeviceIdentity | null,
  ): Promise<Record<string, SettingEntry>> {
    const transport = transportOverride ?? this.state.transport;
    const identity = identityOverride ?? this.state.identity;
    if (!transport || !identity) {
      throw new Error("Connect to a component before reading parameters.");
    }

    const wasReady = this.state.phase === "ready";
    if (wasReady) {
      this.startOperation({
        phase: "reading",
        statusMessage: "Reading settings from the component…",
      });
    }

    let superseded = false;
    try {
      const prefixFlags = this.buildSessionFlags(transport, identity, false);
      const readRes = await this.exec(`${prefixFlags}settings read`);
      // Another component was selected while this read was queued: its values
      // must not replace the new target's, and its state belongs to that operation.
      if (!sameComponent(this.state.identity, identity)) {
        superseded = true;
        throw new Error("The selected component changed during the read; its values were discarded.");
      }
      if (readRes.code !== 0) {
        throw new Error(readRes.stderr || `Settings read failed (${readRes.code})`);
      }

      const snapshot = this.extractLastJsonObject(readRes.stdout);
      const values = this.applyParameterSnapshot(snapshot, identity);
      this.update({ linkLost: false });
      return values;
    } catch (err: any) {
      if (!superseded) {
        this.fail("read", err, { phase: wasReady ? "ready" : "error", status: (error) => `Read error: ${error}` });
      }
      throw err;
    }
  }

  private applyParameterSnapshot(snapshot: any, identity: DeviceIdentity): Record<string, SettingEntry> {
    if (!snapshot?.values || typeof snapshot.values !== "object" || Array.isArray(snapshot.values)) {
      throw new Error("Component response lacks a valid settings snapshot.");
    }
    const actual = this.parseIdentity(snapshot.identity);
    if (actual.serial !== identity.serial || actual.modelSlug !== identity.modelSlug || actual.slot !== identity.slot
        || actual.modelSeries !== identity.modelSeries || actual.modelUnit !== identity.modelUnit) {
      throw new Error("Settings snapshot belongs to a different component.");
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
    const component = identity.isMotor ? "" : `--component ${shellQuote(identity.model)} `;
    const target = `--slot ${identity.slot} ${component}--expect-serial ${cleanSerial} `;
    if (identity.isMotor && isModernModel(identity.modelSlug) && (transport === "ble" || transport === "sim" || requiresUnitAuth)) {
      return `${target}--ble-unit-auth `;
    }
    return target;
  }

  public async disconnect(): Promise<void> {
    let cleanupError: string | null = null;
    this.disconnecting = true;
    // Refuse the review now: approving it while the disconnect runs would
    // queue the write behind it.
    this.state.mcpApproval?.resolve(false);
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
      this.disconnecting = false;
      this.speedDestinationBeforeAuto = null;
      // A review shown for this session must not authorize a write in the next one.
      this.state.mcpApproval?.resolve(false);
      this.update({
        phase: "disconnected",
        transport: null,
        identity: null,
        units: [],
        schema: [],
        liveValues: {},
        unavailableValues: {},
        stagedValues: {},
        failure: cleanupError ? { kind: "disconnect", error: cleanupError } : null,
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
    const reject = (message: string): never => { this.fail("stage", message); throw new Error(message); };
    const spec = this.state.schema.find((setting) => setting.name === name);
    if (this.state.phase !== "ready" || this.state.linkLost) reject("Wait for a connected, idle component before staging settings.");
    if (!spec || spec.read_only || this.state.unavailableValues[name] || !this.state.liveValues[name]) {
      return reject(`Setting ${name} is unavailable or read-only on this component.`);
    }
    if (spec.value_type === "number" || spec.value_type === "enum") {
      if ((String(value).trim() === "" || !settingAcceptsValue(spec, Number(value))) && name !== "speed-limit") reject(`Setting ${name} is outside the permitted range or choices.`);
      value = Number(value);
    }
    const nextStaged = { ...this.state.stagedValues };
    const stage = (key: string, next: number | string) => {
      const live = this.state.liveValues[key]?.value;
      if (key !== "hub-adjustment" && live !== undefined && String(live) === String(next)) delete nextStaged[key];
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
        this.fail("stage", `No supported destination permits ${value} km/h.`);
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
    this.update({ stagedValues: nextStaged, failure: this.state.failure?.kind === "stage" ? null : this.state.failure });
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
    this.update({ stagedValues: {}, failure: this.state.failure?.kind === "stage" ? null : this.state.failure });
  }

  /**
   * Executes all staged parameter changes in a single WASM `settings batch` session.
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
      throw new Error("No active component session.");
    }
    if (this.state.phase !== "ready" || this.state.linkLost) throw new Error("Reconnect to an idle component before saving.");
    const entries = Object.entries(stagedValues);
    if (entries.length === 0) return null;
    for (const [name] of entries) {
      const spec = this.state.schema.find((setting) => setting.name === name);
      if (!spec || spec.read_only || this.state.unavailableValues[name]) throw new Error(`Setting ${name} is unavailable or read-only on this component.`);
    }

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
        `${flags}settings batch ${shellQuote(planFile)} --backup-prefix ${shellQuote(backupPrefix)}`,
      );

      if (batchRes.code !== 0) {
        throw new Error(batchRes.stderr || `Batch write failed (${batchRes.code})`);
      }

      const resultJson = this.extractLastJsonObject(batchRes.stdout);

      if (resultJson?.completed !== operations.length || !Array.isArray(resultJson?.results)
          || resultJson.results.length !== operations.length
          || !resultJson.results.every((receipt: any, index: number) => receipt?.setting === operations[index].name && receipt.after !== undefined
            && String(receipt.after) === operations[index].value)) {
        throw new Error("Save did not return a complete set of verified setting receipts. Read the component before deciding what to retry.");
      }
      // Update liveValues only from complete, verified readbacks.
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
      this.fail("save", err, {
        phase: "ready",
        settings: sortedEntries.map(([name]) => name),
        status: (error) => `Transaction halted: ${error}`,
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
      throw new Error("Connect to a component to capture a snapshot.");
    }
    this.startOperation({
      phase: "reading",
      statusMessage: "Reading every setting into a backup…",
    });
    try {
      const filename = `snapshot_${identity.modelSlug}_${identity.serial}_${fileStamp()}.json`;
      const flags = this.buildSessionFlags(transport, identity, false);
      const res = await this.exec(`${flags}settings read --output ${shellQuote(filename)}`);
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
      this.fail("capture", err, { phase: "ready" });
      throw err;
    }
  }

  /**
   * Restore one save's backups in a single `settings batch`, so the core can
   * order a region change against its speed and power limits (one file at a
   * time, undoing a region raise is refused). Each restore creates its own
   * rollback backup first, then settings are read once to show the result.
   */
  public async restoreBackupFiles(filenames: string[]): Promise<void> {
    if (this.syncInProgress) {
      await this.syncInProgress.catch(() => {});
    }
    const { transport, identity } = this.state;
    if (!transport || !identity) {
      throw new Error("Connect to the target component before restoring a backup.");
    }
    if (filenames.length === 0) return;
    if (this.state.phase !== "ready" || this.state.linkLost) throw new Error("Reconnect to an idle component before restoring.");
    if (Object.keys(this.state.stagedValues).length) throw new Error("Save or discard staged changes before restoring a backup.");
    for (const filename of filenames) {
      const backup = JSON.parse(new TextDecoder().decode(this.terminal!.files.bytes(filename)));
      if (!["cranklink-setting-backup-v1", "esteps-setting-backup-v1"].includes(backup.format)
          || String(backup.serial).toUpperCase() !== identity.serial
          || (!identity.isMotor && backup.settings_model !== identity.modelSlug)
          || (backup.settings_model !== undefined && backup.settings_model !== identity.modelSlug)) {
        throw new Error("Backup model or serial does not match the selected component.");
      }
    }
    this.startOperation({
      phase: "committing",
      statusMessage: `Restoring ${filenames.length} backup file${filenames.length > 1 ? "s" : ""}…`,
    });
    try {
      const backupPrefix = `pre_restore_${fileStamp()}`;
      const planFile = `${backupPrefix}.plan.json`;
      await this.terminal!.files.import(
        planFile,
        new TextEncoder().encode(JSON.stringify(filenames.map((file) => ({ op: "restore", file })), null, 2)),
      );
      const flags = this.buildSessionFlags(transport, identity, true);
      const res = await this.exec(`${flags}settings batch ${shellQuote(planFile)} --backup-prefix ${shellQuote(backupPrefix)}`);
      if (res.code !== 0) {
        throw new Error(res.stderr || `Restore failed (${res.code})`);
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
      this.fail("restore", err, { phase: "ready" });
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
      let settingsModel: string | undefined;
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
            settingsModel = parsed.settings_model;
            setting = String(parsed.setting);
            before = parsed.before ?? null;
            summary = `${formatSetting(this.state.schema, setting, before)} before the change`;
          } else if (parsed.values) {
            kind = "full-snapshot";
            serial = parsed.identity?.serial ?? parsed.serial;
            settingsModel = parsed.identity?.settings_model ?? parsed.settings_model;
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
        settingsModel,
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
    // Another tab may have stored one of these names since the last command.
    await this.terminal.files.initialize();

    for (const file of Array.from(files)) {
      const bytes = new Uint8Array(await file.arrayBuffer());
      const hashBuffer = await crypto.subtle.digest("SHA-256", bytes);
      const sha256 = Array.from(new Uint8Array(hashBuffer))
        .map((b) => b.toString(16).padStart(2, "0"))
        .join("");

      // The store never replaces a file, so a different image under a stored
      // name gets a hash-tagged name; inspect must read the bytes just hashed.
      const store = this.terminal.files;
      const holds = (path: string) => store.list().some((f) => f.path === path);
      let filename = file.name;
      if (holds(filename) && !sameBytes(store.bytes(filename), bytes)) {
        const dot = filename.lastIndexOf(".");
        const tag = sha256.slice(0, 12);
        filename = dot > 0 ? `${filename.slice(0, dot)}.${tag}${filename.slice(dot)}` : `${filename}.${tag}`;
      }
      if (!holds(filename)) await store.import(filename, bytes);
      else if (!sameBytes(store.bytes(filename), bytes)) {
        const error = `${filename} already holds a different image.`;
        this.fail("firmware", error);
        throw new Error(error);
      }

      const inspectRes = await this.exec(`firmware inspect ${shellQuote(filename)}`);
      let metadata = null;
      let inspectErr: string | undefined;
      if (inspectRes.code === 0) {
        metadata = this.extractLastJsonObject(inspectRes.stdout);
      } else {
        inspectErr = inspectRes.stderr || "Invalid firmware image format";
      }

      const role = classifyFirmwareRole(file.name, metadata);
      const inspected: InspectedFirmwareFile = {
        filename,
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
        `firmware inspect ${shellQuote(dcasFile.filename)} --companion ${shellQuote(rxFile.filename)}`,
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
    if (!identity.isMotor) throw new Error("Firmware operations are available only for supported drive units.");
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
      this.fail("firmware", err, {
        phase: "ready",
        status: (error) => `Firmware operation stopped: ${error}`,
        firmware: { ...this.state.firmware, dryRunPassed: false, dryRunSerial: null },
      });
      throw err;
    }
  }

  public downloadVirtualFile(filename: string): void {
    this.exec(`download ${shellQuote(filename)}`).catch(() => {});
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
      const identity = this.state.identity;
      if (!identity || this.disconnecting) {
        reject(new Error("Select a connected component first."));
        return;
      }
      const enrichedDiff = diff.map((d) => {
        const spec = this.state.schema.find((s) => s.name === d.name);
        return {
          name: d.name,
          label: settingLabel(this.state.schema, d.name),
          before: formatValue(spec, d.before),
          after: formatValue(spec, d.after),
        };
      });
      this.update({
        mcpApproval: {
          id: Math.random().toString(36).slice(2, 10),
          tool,
          params,
          diff: enrichedDiff,
          target: { model: identity.model, slot: identity.slot, serial: identity.serial },
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
