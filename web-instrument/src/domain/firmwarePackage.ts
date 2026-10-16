/**
 * firmwarePackage.ts - Domain logic for Synchronized Dual-MCU Firmware Packages (DCAS + RX).
 *
 * Instead of forcing the user to manually classify, inspect, dry-run, and flash DCAS (D)
 * and RX (M) chips separately, this module models the drive unit firmware release as a
 * synchronized companion package by default. Single-MCU flashing is relegated to an explicit
 * recovery override.
 */

import type { FirmwareMetadata, InspectedFirmwareFile } from "../types.ts";

export function classifyFirmwareRole(
  filename: string,
  metadata?: FirmwareMetadata | null,
): "dcas" | "rx" | "unknown" {
  if (metadata?.kind === "DcasX") return "dcas";
  if (metadata?.kind === "Renesas") return "rx";
  const upper = filename.toUpperCase();
  if (upper.includes("-D.") || upper.includes("_D.") || upper.includes("DCAS")) {
    return "dcas";
  }
  if (upper.includes("-M.") || upper.includes("_M.") || upper.includes("RX")) {
    return "rx";
  }
  return "unknown";
}

export function formatVersion(v: string | number[] | undefined | null): string {
  if (!v) return "unknown";
  if (Array.isArray(v)) return v.join(".");
  return String(v);
}

export interface CompanionValidationResult {
  valid: boolean;
  message: string;
  dcasVersion?: string;
  rxVersion?: string;
  series?: number;
}

export function evaluateCompanionPair(
  dcas: InspectedFirmwareFile | null,
  rx: InspectedFirmwareFile | null,
): CompanionValidationResult {
  if (!dcas && !rx) {
    return {
      valid: false,
      message: "Select a matched DCAS (-D) and RX (-M) firmware pair to begin.",
    };
  }
  if (!dcas) {
    return {
      valid: false,
      message: `RX image (${rx?.filename}) loaded. Waiting for companion DCAS (-D) image.`,
    };
  }
  if (!rx) {
    return {
      valid: false,
      message: `DCAS image (${dcas.filename}) loaded. Waiting for companion RX (-M) image.`,
    };
  }
  if (dcas.error) {
    return { valid: false, message: `DCAS image error: ${dcas.error}` };
  }
  if (rx.error) {
    return { valid: false, message: `RX image error: ${rx.error}` };
  }
  if (!dcas.metadata || !rx.metadata) {
    return {
      valid: false,
      message: "Inspecting firmware metadata...",
    };
  }

  const dMeta = dcas.metadata;
  const rMeta = rx.metadata;

  if (dMeta.series !== rMeta.series || dMeta.unit !== rMeta.unit) {
    return {
      valid: false,
      message: `Target hardware mismatch: DCAS is series 0x${dMeta.series.toString(16)}/${dMeta.unit}, RX is series 0x${rMeta.series.toString(16)}/${rMeta.unit}.`,
    };
  }

  const dVer = formatVersion(dMeta.version);
  const rVer = formatVersion(rMeta.version);
  const dCompat = formatVersion(dMeta.compatibility_version);

  // If DCAS specifies a compatibility_version, verify RX version is compatible
  if (
    dMeta.compatibility_version &&
    rMeta.version &&
    dCompat !== "unknown" &&
    dCompat !== rVer
  ) {
    // Allow minor patch >= compatibility floor on same major.minor
    const dParts = dCompat.split(".").map(Number);
    const rParts = rVer.split(".").map(Number);
    if (dParts[0] !== rParts[0] || dParts[1] !== rParts[1]) {
      return {
        valid: false,
        message: `Companion version mismatch: DCAS ${dVer} requires RX ${dCompat}, but RX is ${rVer}.`,
      };
    }
  }

  return {
    valid: true,
    dcasVersion: dVer,
    rxVersion: rVer,
    series: dMeta.series,
    message: `Synchronized package verified: DCAS ${dVer} + RX ${rVer} (Series 0x${dMeta.series.toString(16).toUpperCase()})`,
  };
}

export interface SyncFlashPlan {
  mode: "ble-pair" | "wired-pair" | "single-dcas" | "single-rx";
  description: string;
  commands: string[];
  backupFile: string;
  journalFiles: string[];
}

function shellQuote(s: string): string {
  return `'${s.replaceAll("'", "'\\''")}'`;
}

export function buildSynchronizedFlashPlan(options: {
  transport: "usb" | "serial" | "ble" | "sim";
  serial: string;
  dcas: InspectedFirmwareFile;
  rx: InspectedFirmwareFile;
  write: boolean;
  prefix: string;
  overrideSingleTarget?: "dcas" | "rx" | null;
}): SyncFlashPlan {
  const { transport, serial, dcas, rx, write, prefix, overrideSingleTarget } = options;
  const cleanSerial = serial.replace(/[-:\s]/g, "");
  const backupFile = `${prefix}.settings.backup.json`;
  const writeFlag = write ? " --write" : "";

  if (overrideSingleTarget === "dcas") {
    const journal = `${prefix}.dcas.journal.jsonl`;
    const cmd =
      transport === "ble" || transport === "sim"
        ? `--ble-firmware --ble-firmware-backup ${shellQuote(backupFile)} flash-dcas ${shellQuote(dcas.filename)} --companion ${shellQuote(rx.filename)} --expect-serial ${cleanSerial} --expect-sha256 ${dcas.sha256} --journal ${shellQuote(journal)}${writeFlag}`
        : `flash-dcas ${shellQuote(dcas.filename)} --companion ${shellQuote(rx.filename)} --expect-serial ${cleanSerial} --expect-sha256 ${dcas.sha256} --journal ${shellQuote(journal)}${writeFlag}`;
    return {
      mode: "single-dcas",
      description: `${write ? "Program" : "Dry-run"} DCAS MCU only (${dcas.filename})`,
      commands: [cmd],
      backupFile,
      journalFiles: [journal],
    };
  }

  if (overrideSingleTarget === "rx") {
    const journal = `${prefix}.rx.journal.jsonl`;
    const cmd =
      transport === "ble" || transport === "sim"
        ? `--ble-firmware --ble-firmware-backup ${shellQuote(backupFile)} flash-rx ${shellQuote(rx.filename)} --companion ${shellQuote(dcas.filename)} --expect-serial ${cleanSerial} --expect-sha256 ${rx.sha256} --journal ${shellQuote(journal)}${writeFlag}`
        : `flash-rx ${shellQuote(rx.filename)} --companion ${shellQuote(dcas.filename)} --expect-serial ${cleanSerial} --expect-sha256 ${rx.sha256} --journal ${shellQuote(journal)}${writeFlag}`;
    return {
      mode: "single-rx",
      description: `${write ? "Program" : "Dry-run"} RX MCU only (${rx.filename})`,
      commands: [cmd],
      backupFile,
      journalFiles: [journal],
    };
  }

  // Default: Synchronized Dual-MCU Package (both chips)
  if (transport === "ble" || transport === "sim") {
    const journal = `${prefix}.pair.journal.jsonl`;
    const cmd = `--ble-firmware ble flash-pair --dcas ${shellQuote(dcas.filename)} --rx ${shellQuote(rx.filename)} --expect-serial ${cleanSerial} --expect-dcas-sha256 ${dcas.sha256} --expect-rx-sha256 ${rx.sha256} --journal ${shellQuote(journal)} --backup ${shellQuote(backupFile)}${writeFlag}`;
    return {
      mode: "ble-pair",
      description: `${write ? "Synchronized Dual-MCU Flash" : "Dual-MCU Non-Erasing Check"} (DCAS + RX in one serial-bound transaction)`,
      commands: [cmd],
      backupFile,
      journalFiles: [journal],
    };
  }

  // Wired USB / Serial: Backup settings -> Flash DCAS -> Flash RX in sequence
  const dJournal = `${prefix}.01-dcas.journal.jsonl`;
  const rJournal = `${prefix}.02-rx.journal.jsonl`;
  const commands: string[] = [];
  if (write) {
    commands.push(
      `--expect-motor-serial ${cleanSerial} settings read --output ${shellQuote(backupFile)}`,
    );
  }
  commands.push(
    `flash-dcas ${shellQuote(dcas.filename)} --companion ${shellQuote(rx.filename)} --expect-serial ${cleanSerial} --expect-sha256 ${dcas.sha256} --journal ${shellQuote(dJournal)}${writeFlag}`,
  );
  commands.push(
    `flash-rx ${shellQuote(rx.filename)} --companion ${shellQuote(dcas.filename)} --expect-serial ${cleanSerial} --expect-sha256 ${rx.sha256} --journal ${shellQuote(rJournal)}${writeFlag}`,
  );

  return {
    mode: "wired-pair",
    description: `${write ? "Synchronized Dual-MCU Flash" : "Dual-MCU Non-Erasing Check"} (Sequenced DCAS + RX with companion verification)`,
    commands,
    backupFile,
    journalFiles: [dJournal, rJournal],
  };
}
