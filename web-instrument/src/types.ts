/**
 * types.ts - Strict domain contracts for esteps Field Instrument.
 */

export type TransportType = "usb" | "serial" | "ble" | "sim";

export type SimulatedModel = "ep800-crg" | "ep600" | "ep801";

export type FaultInjectionType =
  | "none"
  | "auth_reject"
  | "disconnect_on_connect"
  | "disconnect_on_write"
  | "write_timeout"
  | "readonly_rejection";

export interface SimulatorOptions {
  model: SimulatedModel;
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
  modelSlug: SimulatedModel;
  serial: string;
  dcasVersion: string;
  rxVersion: string;
  isCargo: boolean;
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
  timestamp: string;
  serial?: string;
  summary: string;
  sizeBytes: number;
}

export interface FirmwareMetadata {
  kind: "DcasX" | "Renesas" | string;
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

export interface FirmwarePackageState {
  dcasFile: InspectedFirmwareFile | null;
  rxFile: InspectedFirmwareFile | null;
  companionValid: boolean;
  companionMessage: string;
  dryRunPassed: boolean;
  dryRunSerial: string | null;
  lastFlashReceipt: any | null;
}

export type WorkspaceSection =
  | "parameters"
  | "firmware"
  | "journal";

export interface MCPApprovalRequest {
  id: string;
  tool: string;
  params: any;
  diff: Array<{ name: string; label: string; before: any; after: any }>;
  resolve: (approved: boolean) => void;
  reject: (err: Error) => void;
}

export interface LogEntry {
  id: number;
  timestamp: string;
  stream: "stdout" | "stderr" | "cmd";
  text: string;
}

export type SessionPhase =
  | "disconnected"
  | "connecting"
  | "probing"
  | "reading"
  | "ready"
  | "committing"
  | "flashing"
  | "error";

export interface InstrumentState {
  phase: SessionPhase;
  statusMessage: string;
  errorBanner: string | null;
  transport: TransportType | null;
  simulatorOptions: SimulatorOptions;
  identity: DeviceIdentity | null;
  schema: EnrichedSetting[];
  liveValues: Record<string, SettingEntry>;
  unavailableValues: Record<string, string>;
  stagedValues: Record<string, number | string>;
  activeSection: WorkspaceSection;
  activeParameterGroup: SettingSectionId | "all";
  activeProfileTab: 1 | 2;
  searchFilter: string;
  firmware: FirmwarePackageState;
  snapshots: JournalSnapshot[];
  logs: LogEntry[];
  consoleExpanded: boolean;
  mcpApproval: MCPApprovalRequest | null;
}
