import { RefreshCw, AlertTriangle, Info } from "lucide-react";
import type { InstrumentState, SessionPhase, TransportType } from "../../types.ts";

const TITLES: Partial<Record<SessionPhase, string>> = {
  probing: "Identifying drive unit…",
  reading: "Reading settings…",
  committing: "Writing to the bike…",
  flashing: "Updating firmware…",
};

function connectingTitle(transport: TransportType | null): string {
  switch (transport) {
    case "ble": return "Connecting over Bluetooth…";
    case "usb": return "Connecting over USB…";
    case "serial": return "Connecting over serial…";
    case "sim": return "Starting simulator…";
    default: return "Connecting…";
  }
}

/** What the operator must not do while this phase runs, per transport. */
function guidance(phase: SessionPhase, transport: TransportType | null): { text: string; critical: boolean } {
  if (transport === "sim") {
    return { text: "Simulated drive unit. No bike hardware is involved.", critical: false };
  }
  const link = transport === "ble"
    ? "Keep this page open and the bike powered on and in connection mode"
    : "Keep the cable connected and the bike powered on";
  if (phase === "flashing") {
    return { text: `Firmware is being written. ${link} until this finishes, or the motor may need recovery.`, critical: true };
  }
  if (phase === "committing") {
    return { text: `${link} until the write finishes.`, critical: true };
  }
  return { text: `${link}.`, critical: false };
}

export function OperationModal({ state }: { state: InstrumentState }) {
  const busy = ["connecting", "probing", "reading", "committing", "flashing"].includes(state.phase);
  if (!busy) return null;

  const title = state.phase === "connecting" ? connectingTitle(state.transport) : TITLES[state.phase] ?? "Working…";
  const notice = guidance(state.phase, state.transport);

  return (
    <div
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="operation-title"
      aria-describedby="operation-status"
      className="fixed inset-0 z-50 flex items-center justify-center p-4"
      data-testid="operation-modal"
    >
      <div className="fixed inset-0 bg-stone-950/60 backdrop-blur-xs animate-in fade-in duration-150" />
      <div className="relative bg-white rounded-2xl shadow-2xl border border-stone-200 z-50 p-6 sm:p-8 w-[calc(100%-2rem)] max-w-md animate-in zoom-in-95 duration-150">
        <div className="flex flex-col items-center text-center gap-5">
          <div className="w-14 h-14 rounded-2xl bg-stone-900 text-white flex items-center justify-center" aria-hidden="true">
            <RefreshCw size={26} className="animate-spin text-amber-400" />
          </div>
          <div className="flex flex-col gap-2">
            <h2 id="operation-title" className="text-xl font-bold tracking-tight text-stone-900">{title}</h2>
            <p id="operation-status" className="text-sm text-stone-600 [overflow-wrap:anywhere]" aria-live="polite">
              {state.statusMessage}
            </p>
          </div>
          <div
            className={`p-3.5 rounded-xl border text-xs flex items-start gap-2.5 text-left leading-relaxed ${
              notice.critical ? "bg-amber-50 border-amber-200 text-amber-900" : "bg-stone-50 border-stone-200 text-stone-700"
            }`}
          >
            {notice.critical
              ? <AlertTriangle size={16} className="text-amber-600 shrink-0 mt-0.5" aria-hidden="true" />
              : <Info size={16} className="text-stone-500 shrink-0 mt-0.5" aria-hidden="true" />}
            <span>{notice.text}</span>
          </div>
        </div>
      </div>
    </div>
  );
}
