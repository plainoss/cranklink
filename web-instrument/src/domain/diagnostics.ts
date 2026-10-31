import type { InstrumentState } from "../types.ts";

declare const __CRANKLINK_BUILD__: string;
export const BUILD_REVISION = typeof __CRANKLINK_BUILD__ === "string" ? __CRANKLINK_BUILD__ : "development";

/** Raw console commands can include an explicitly supplied PIN. Never export it. */
export function redactDiagnosticText(text: string): string {
  return text.replace(/(--passkey(?:\s*=\s*|\s+))(?:"[^"]*"|'[^']*'|[^\s]+)/gi, "$1[redacted]");
}

export function formatDiagnosticReport(
  state: InstrumentState,
  metadata: { url: string; userAgent: string; generatedAt?: string },
): string {
  const identity = state.identity;
  return redactDiagnosticText([
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
    ...(state.failure ? [`Failure (${state.failure.kind}):`, state.failure.error, ""] : []),
    `Session log (UTC; latest ${state.logs.length} entries):`,
    ...state.logs.map((log) => `[${log.timestamp}] ${log.stream}: ${log.text}`),
  ].join("\n"));
}
