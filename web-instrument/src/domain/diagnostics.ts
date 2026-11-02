import type { InstrumentState } from "../types.ts";

declare const __CRANKLINK_BUILD__: string;
export const BUILD_REVISION = typeof __CRANKLINK_BUILD__ === "string" ? __CRANKLINK_BUILD__ : "development";

/**
 * Raw console commands can include an explicitly supplied PIN, and settings
 * output can include the bike's `bluetooth-passkey` value (as six digits or as
 * their ASCII hex). Never export either.
 */
export function redactDiagnosticText(text: string, passkeyContext = false): string {
  const flagsRedacted = redactPasskeyFlags(text);
  if (!passkeyContext && !/passkey/i.test(flagsRedacted)) return flagsRedacted;
  return flagsRedacted
    .replace(/(?<![0-9A-Fa-f])(?:3[0-9]){6}(?![0-9A-Fa-f])/g, "[redacted]")
    .replace(/(?<!\d)\d{6}(?!\d)/g, "[redacted]");
}

/** Output that follows a passkey command (e.g. a single read) is redacted too. */
function redactedLogLines(logs: InstrumentState["logs"]): string[] {
  let passkeyContext = false;
  return logs.map((log) => {
    if (log.stream === "cmd") passkeyContext = /passkey/i.test(log.text);
    return `[${log.timestamp}] ${log.stream}: ${redactDiagnosticText(log.text, passkeyContext)}`;
  });
}

function redactPasskeyFlags(text: string): string {
  return text.replace(/(--passkey(?:\s*=\s*|\s+))(?:"[^"]*"|'[^']*'|[^\s]+)/gi, "$1[redacted]");
}

export function formatDiagnosticReport(
  state: InstrumentState,
  metadata: { url: string; userAgent: string; generatedAt?: string },
): string {
  const identity = state.identity;
  // Each entry is redacted on its own so one passkey line does not mask
  // unrelated numbers elsewhere in the report.
  return redactPasskeyFlags([
    "cranklink diagnostic report",
    `Build: ${BUILD_REVISION}`,
    `Generated (UTC): ${metadata.generatedAt ?? new Date().toISOString()}`,
    `Page: ${metadata.url.split(/[?#]/)[0]}`,
    `Browser: ${metadata.userAgent}`,
    `Transport: ${state.transport ?? "none"}`,
    `Phase: ${state.phase}`,
    `Bluetooth override: ${state.bluetoothBike}`,
    `Detected system: ${state.bluetoothSystem ?? "unknown"} (${state.bluetoothSystemSource ?? "unknown"})`,
    `Master series: ${state.bluetoothMasterSeries == null ? "unknown" : `0x${state.bluetoothMasterSeries.toString(16)}`}`,
    ...(identity ? [`Component: ${identity.model}, slot ${identity.slot}`, `Serial: ${identity.serial}`,
      `DCAS: ${identity.dcasVersion}; RX: ${identity.rxVersion}`] : []),
    "",
    ...(state.failure ? [`Failure (${state.failure.kind}):`, redactDiagnosticText(state.failure.error), ""] : []),
    `Session log (UTC; latest ${state.logs.length} entries):`,
    ...redactedLogLines(state.logs),
  ].join("\n"));
}
