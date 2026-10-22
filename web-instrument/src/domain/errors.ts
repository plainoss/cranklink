/**
 * errors.ts - Operator messages for failures; complete errors stay in Diagnostics.
 */

import type { EnrichedSetting, OperationFailure, TransportType } from "../types.ts";
import { settingLabel } from "./settingFormat.ts";

const CLEANUP_UNCONFIRMED = /PC-link stop.*not.*confirmed|stop could not be confirmed|cleanup.*(?:failed|not.*confirmed)/i;
const LINK_CLOSED = /GATT Server is disconnected|Bluetooth disconnected|Bluetooth connection is closed|BLE session failed|disconnect and inspect/i;
/** WebUSB and Web Serial ports report an unplugged SM-PCE02 this way (usb.js, serial.js, app.js). */
const WIRED_CLOSED = /(?:USB|Serial) (?:device )?disconnected|(?:USB|Serial) port is closed/i;
/** The CLI's interrupted-flash warning; its full text carries the recovery command. */
const FLASH_INTERRUPTED = /FLASH WRITE INTERRUPTED|DO NOT POWER CYCLE/i;
/** A flash progress line: the erase and write had started. */
const FLASH_STARTED = /^Programming \d+\/\d+ bytes \(\d+%\)/m;

/** README steps for a motor stuck in its bootloader; recovery runs only in the CLI. */
export const RECOVERY_URL = "https://github.com/plainoss/cranklink#recover-a-motor-stuck-in-bootloader-mode";

/** Flash progress lines and the rules around the CLI's warning only bury it here. */
const TERMINAL_NOISE = /^(?:Programming \d+\/\d+ bytes \(\d+%\)|=+)[ \t]*(?:\n|$)/gm;

function rawError(error: string): string {
  return error
    .replace(TERMINAL_NOISE, "")
    .replace(/\n{3,}/g, "\n\n")
    .trim()
    .split(/\n\s*at |\nError:/)[0]
    .replace(/^error:\s*/i, "")
    .replace(/\s*\[exit \d+\]\s*$/, "")
    .trim();
}

/** Concise operator message for a raw error. */
export function userFacingError(error: string, transport: TransportType | null = null): string {
  const link = transport === "sim" ? "The simulated Bluetooth link" : "The Bluetooth link";
  // Never summarise away the do-not-power-cycle warning or the recovery command.
  if (FLASH_INTERRUPTED.test(error)) return rawError(error);
  if (WIRED_CLOSED.test(error)) {
    return "The USB adapter disconnected. Plug it back in and reconnect to read the bike’s current settings.";
  }
  if (CLEANUP_UNCONFIRMED.test(error)) {
    if (transport === "usb" || transport === "serial") {
      return "The bike did not confirm it was released over USB. Check the adapter, then reconnect to read its current settings.";
    }
    return `${link} closed before the bike confirmed it was released. Turn on the bike’s connection mode, then reconnect to read its current settings.`;
  }
  if (/BLE authentication failed|BLE passkey must|passkey.*(?:invalid|incorrect)/i.test(error)) {
    return "Bluetooth PIN was not accepted. Reconnect and enter the bike’s six-digit PIN again.";
  }
  if (/drive-unit authentication is not verified/i.test(error)) {
    return "Saving this setting isn’t supported on this motor firmware yet. No change was sent.";
  }
  if (/drive.unit authentication|modern authentication.*(?:reject|fail)|authentication challenge rejected/i.test(error)) {
    return "The motor rejected authorization for this change. Nothing was retried; check Diagnostics for details.";
  }
  if (/BLE response timeout|notification overflow/i.test(error)) {
    return `${link} stopped responding. Reconnect to read the bike’s current settings.`;
  }
  if (LINK_CLOSED.test(error)) {
    return `${link} closed. Turn on the bike’s connection mode and reconnect.`;
  }
  if (/globally disabled/i.test(error)) {
    return "Web Bluetooth is disabled globally in this browser. Use Google Chrome or Microsoft Edge to connect wirelessly, or check your browser's Bluetooth settings.";
  }
  if (/browser host operation ble.open failed|Connection attempt failed/i.test(error)) {
    return "Couldn’t connect to the bike. Turn on its Bluetooth connection mode and try again.";
  }
  return rawError(error);
}

/**
 * True when a failure may have left the motor in its bootloader. The wired
 * flash prints its own warning; the Bluetooth ble-pair relay prints none, so
 * there any firmware failure after the write started counts.
 */
function isFlashCritical(error: string, kind?: OperationFailure["kind"], transport?: TransportType | null): boolean {
  return FLASH_INTERRUPTED.test(error)
    || (kind === "firmware" && (transport === "ble" || transport === "sim") && FLASH_STARTED.test(error));
}

/**
 * True when the transport is gone and further commands need a reconnect. An
 * interrupted flash needs recovery instead: the motor may be in its
 * bootloader, where a reconnect cannot find it.
 */
export function isLinkLost(error: string, kind?: OperationFailure["kind"], transport?: TransportType | null): boolean {
  if (isFlashCritical(error, kind, transport)) return false;
  return CLEANUP_UNCONFIRMED.test(error) || LINK_CLOSED.test(error) || WIRED_CLOSED.test(error);
}

export interface FailureDescription {
  title: string;
  /** What happened to the bike, stated first. */
  outcome: string | null;
  message: string;
  linkLost: boolean;
  /** The motor may be in its bootloader: CLI recovery is the only safe next step. */
  critical: boolean;
}

const TITLES: Record<OperationFailure["kind"], string> = {
  connect: "Couldn’t connect",
  read: "Couldn’t read settings",
  stage: "Can’t stage that value",
  save: "Save stopped",
  restore: "Restore stopped",
  capture: "Backup failed",
  firmware: "Firmware operation stopped",
  disconnect: "Disconnect not confirmed",
};

/** Lead with the state of the bike, then the cause, so the next step is clear. */
export function describeFailure(
  failure: OperationFailure,
  transport: TransportType | null,
  schema: EnrichedSetting[],
): FailureDescription {
  const { kind, error } = failure;
  const critical = isFlashCritical(error, kind, transport);
  let outcome: string | null = null;

  const batch = error.match(/batch stopped at operation (\d+); (\d+) completed operations? remain applied/i);
  if (kind === "save" && batch) {
    const failedIndex = Number(batch[1]) - 1;
    const applied = Number(batch[2]);
    const settings = failure.settings ?? [];
    const total = Math.max(settings.length, failedIndex + 1);
    const failedName = settings[failedIndex];
    const failedLabel = failedName ? settingLabel(schema, failedName) : `Change ${failedIndex + 1}`;
    const failedText = /outcome may be uncertain/i.test(error)
      ? `${failedLabel} may or may not have been written.`
      : `${failedLabel} was not written.`;
    const appliedText = total === 1
      ? ""
      : applied === 0
        ? `None of the ${total} changes were applied. `
        : `${applied} of ${total} changes were applied before the error. `;
    outcome = appliedText + failedText;
  } else if (critical) {
    outcome = "The motor may be left in recovery mode. Do not power-cycle it. The web app cannot recover it: on a computer "
      + "with an SM-PCE02, follow the recovery steps with the cranklink CLI"
      + (FLASH_INTERRUPTED.test(error) ? ", using the command below with that computer’s --port." : ".");
  } else if (kind === "save" && /authentication is not verified|authentication challenge rejected|authentication failed/i.test(error)) {
    outcome = "No change was written. Your staged values are kept.";
  }
  if (failure.stagedDiscarded) outcome = `${outcome ? `${outcome} ` : ""}Your unsaved changes were discarded.`;

  return {
    title: TITLES[kind],
    outcome,
    // A summary such as "reconnect" must not replace the cause of a critical failure.
    message: critical ? rawError(error) : userFacingError(error, transport),
    linkLost: isLinkLost(error, kind, transport),
    critical,
  };
}
