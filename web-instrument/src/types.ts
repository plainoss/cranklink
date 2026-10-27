/**
 * types.ts - Strict domain contracts for esteps Field Instrument.
 */

export type TransportType = "usb" | "serial" | "ble" | "sim";
export type BluetoothBike = "steps" | "di2";

export type SimulatedModel = "ep800-crg" | "ep600" | "ep801";
export type MotorModel = SimulatedModel | "ep500" | "e5100" | "e8000" | "e7000" | "e6100" | "e6000" | "e6001" | "e6002-pre33" | "e6002";
export type SettingsModel = MotorModel | `${string}-${string}`;

export interface ComponentUnit {
  slot: number;
  model_series: number;
  model_unit: number;
  model: string | null;
  model_candidates: string[];
  part?: number | null;
  model_params?: number[] | null;
  raw?: number[];
}

export type FaultInjectionType =
  | "none"
  | "auth_reject"
  | "disconnect_on_connect"
  | "disconnect_on_write"
  | "write_timeout"
  | "readonly_rejection";

export interface SimulatorOptions {
  model: SimulatedModel;
  components?: boolean;
  initialPreset?: "eu_default" | "us_commuter" | "cargo_heavy";
  fault?: FaultInjectionType;
}

export interface SettingSpec {
  name: string;
  unit: string;
  value_type: "number" | "string" | "date" | "enum";
  min_raw: number;
  max_raw: number;
  step_raw: number;
  choices: number[];
  oem: boolean;
  nullable: boolean;
  read_only?: boolean;
  wire_units_per_input: number;
}

export type SettingSectionId =
  | "geometry"
  | "profiles"
  | "drivetrain"
  | "maintenance"
  | "oem";

export interface SettingPreset {
  label: string;
  value: number | string;
}

export interface EnrichedSetting extends SettingSpec {
  section: SettingSectionId;
  label: string;
  description: string;
  displayUnit: string;
  userMin: number;
  userMax: number;
  userStep: number;
  choiceLabels: Record<number, string> | null;
  presets: SettingPreset[] | null;
}

export interface DeviceIdentity {
  slot: number;
  model: string;
  modelSlug: SettingsModel;
  serial: string;
  dcasVersion: string;
  rxVersion: string;
  isCargo: boolean;
  isMotor: boolean;
  modelUnit: number;
  modelSeries?: number;
  unitAuthenticated?: boolean;
}

export interface SettingEntry {
  value: number | string | null;
  raw: string;
}

export interface JournalSnapshot {
  filename: string;
  kind: "full-snapshot" | "setting-backup" | "journal" | "firmware";
  /** Files written by one save, restore, or capture share a group key. */
  group: string;
  /** Parsed creation time, or null when the filename carries no stamp. */
  createdAt: Date | null;
  serial?: string;
  settingsModel?: string;
  summary: string;
  /** Setting name and prior value for single-setting backups. */
  setting?: string;
  before?: number | string | null;
  sizeBytes: number;
}

export interface FirmwareMetadata {
  kind: "dcas-x" | "dcas2i" | "dcas2" | "renesas";
  series: number;
  unit: number;
  board?: number | null;
  version: string | number[];
  compatibility_version?: string | number[];
  source_sha256: string;
  decoded_bytes?: number;
}

export interface InspectedFirmwareFile {
  filename: string;
  sizeBytes: number;
  sha256: string;
  role: "dcas" | "rx" | "unknown";
  metadata: FirmwareMetadata | null;
  error?: string;
}

/** The outcome of a non-erasing check, kept on the Firmware view. */
export interface FirmwareCheckResult {
  passed: boolean;
  /** The drive unit the check was bound to. */
  serial: string;
  /** Firmware the drive unit reported when the check started. */
  installed: { dcas: string; rx: string };
  /** Versions of the checked images. */
  images: { dcas: string; rx: string };
  /** Why the check stopped, when it failed. */
  error?: string;
}

export interface FirmwarePackageState {
  dcasFile: InspectedFirmwareFile | null;
  rxFile: InspectedFirmwareFile | null;
  companionValid: boolean;
  companionMessage: string;
  dryRunPassed: boolean;
  dryRunSerial: string | null;
  lastFlashReceipt: any | null;
  /** The running firmware operation: the non-erasing check writes nothing. */
  operation: "check" | "write" | null;
  lastCheck: FirmwareCheckResult | null;
}

export interface MCPApprovalRequest {
  id: string;
  tool: string;
  params: any;
  /** `regulated` rows change legal or factory limits and are marked for the operator. */
  diff: Array<{ name: string; label: string; before: any; after: any; regulated: boolean }>;
  /** The component the write would reach, shown to the operator. */
  target: { model: string; slot: number; serial: string };
  /** "Label (value)" for current values cranklink could not write back from a backup. */
  unwritable: string[];
  resolve: (approved: boolean) => void;
  reject: (err: Error) => void;
}

export interface LogEntry {
  id: number;
  timestamp: string;
  stream: "stdout" | "stderr" | "notice" | "cmd";
  text: string;
}

export type OperationKind = "connect" | "read" | "stage" | "save" | "restore" | "capture" | "firmware" | "disconnect" | "adapter";

/** The latest failure with enough context to say what happened to the bike. */
export interface OperationFailure {
  kind: OperationKind;
  /** Complete error text; the UI summarizes it and Diagnostics keeps it whole. */
  error: string;
  /** Settings in write order for a failed save. */
  settings?: string[];
  /** The staged edits were lost with this failure. */
  stagedDiscarded?: boolean;
  /** The staged edits are kept for the next connect to this component. */
  stagedHeld?: { model: string; serial: string };
}

export interface Toast {
  id: number;
  message: string;
  action?: "backup";
}

export type SessionPhase =
  | "disconnected"
  | "disconnecting"
  | "connecting"
  | "probing"
  | "reading"
  | "ready"
  | "committing"
  | "flashing"
  | "error";

export interface InstrumentState {
  phase: SessionPhase;
  /** Link closure cannot confirm that the bike has returned to riding mode. */
  bluetoothDisconnectNotice: boolean;
  statusMessage: string;
  failure: OperationFailure | null;
  /** The transport link closed after a failure; reconnect before more commands. */
  linkLost: boolean;
  toast: Toast | null;
  transport: TransportType | null;
  simulatorOptions: SimulatorOptions;
  bluetoothBike: BluetoothBike;
  identity: DeviceIdentity | null;
  units: ComponentUnit[];
  schema: EnrichedSetting[];
  liveValues: Record<string, SettingEntry>;
  unavailableValues: Record<string, string>;
  stagedValues: Record<string, number | string>;
  firmware: FirmwarePackageState;
  snapshots: JournalSnapshot[];
  logs: LogEntry[];
  mcpApproval: MCPApprovalRequest | null;
  /** The Bluetooth gateway is waiting for the operator to enter its passkey. */
  passkeyPromptOpen: boolean;
}
