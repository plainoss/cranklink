/**
 * errors.ts - Operator messages for failures; complete errors stay in Diagnostics.
 */

import type { EnrichedSetting, OperationFailure, TransportType } from "../types.ts";
import { settingLabel } from "./settingFormat.ts";

const CLEANUP_UNCONFIRMED = /PC-link stop.*not.*confirmed|stop could not be confirmed|cleanup.*(?:failed|not.*confirmed)/i;
const LINK_CLOSED = /GATT Server is disconnected|Bluetooth disconnected|Bluetooth connection is closed|BLE session failed|disconnect and inspect/i;

/** Concise operator message for a raw error. */
export function userFacingError(error: string, transport: TransportType | null = null): string {
  const link = transport === "sim" ? "The simulated Bluetooth link" : "The Bluetooth link";
  if (CLEANUP_UNCONFIRMED.test(error)) {
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
  return error
    .split(/\n\s*at |\nError:/)[0]
    .replace(/^error:\s*/i, "")
    .replace(/\s*\[exit \d+\]\s*$/, "")
    .trim();
}

/** True when the transport is gone and further commands need a reconnect. */
export function isLinkLost(error: string): boolean {
  return CLEANUP_UNCONFIRMED.test(error) || LINK_CLOSED.test(error);
}

export interface FailureDescription {
  title: string;
  /** What happened to the bike, stated first. */
  outcome: string | null;
  message: string;
  linkLost: boolean;
}

const TITLES: Record<OperationFailure["kind"], string> = {
  connect: "Couldn’t connect",
  read: "Couldn’t read settings",
  save: "Save stopped",
  restore: "Restore stopped",
  capture: "Backup failed",
  firmware: "Firmware operation stopped",
  disconnect: "Disconnect not confirmed",
};

/** Lead with the state of the bike, then the cause, so the next step is clear. */
export function describeFailure(
  error: string,
  failure: OperationFailure | null,
  transport: TransportType | null,
  schema: EnrichedSetting[],
): FailureDescription {
  const kind = failure?.kind ?? "read";
  let outcome: string | null = null;

  const batch = error.match(/batch stopped at operation (\d+); (\d+) completed operations? remain applied/i);
  if (kind === "save" && batch) {
    const failedIndex = Number(batch[1]) - 1;
    const applied = Number(batch[2]);
    const settings = failure?.settings ?? [];
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
  } else if (kind === "save" && /authentication is not verified|authentication challenge rejected|authentication failed/i.test(error)) {
    outcome = "No change was written. Your staged values are kept.";
  }

  return {
    title: TITLES[kind],
    outcome,
    message: userFacingError(error, transport),
    linkLost: isLinkLost(error),
  };
}
