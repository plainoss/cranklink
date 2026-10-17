import { RefreshCw, AlertTriangle } from "lucide-react";
import type { InstrumentState } from "../../types.ts";

export interface OperationModalProps {
  state: InstrumentState;
}

export function OperationModal({ state }: OperationModalProps) {
  const isBusy =
    state.phase === "connecting" ||
    state.phase === "probing" ||
    state.phase === "reading" ||
    state.phase === "committing" ||
    state.phase === "flashing";

  if (!isBusy) return null;

  const getTitle = () => {
    switch (state.phase) {
      case "connecting":
        return state.transport === "ble"
          ? "Connecting via Bluetooth..."
          : state.transport === "usb"
            ? "Connecting via USB Cable..."
            : "Connecting to Drive Unit...";
      case "probing":
        return "Probing Drive Unit...";
      case "reading":
        return "Reading Parameters...";
      case "committing":
        return "Writing Settings...";
      case "flashing":
        return "Updating Firmware...";
      default:
        return "Operation in Progress...";
    }
  };

  const getNotice = () => {
    if (state.phase === "flashing") {
      return "CRITICAL: Firmware flash in progress. Keep communication cables firmly connected and do not power off the bike.";
    }
    if (state.phase === "committing") {
      return "Writing parameters to Shimano EEPROM. Please keep communication active.";
    }
    return "Please keep communication cables attached and do not power off the bike.";
  };

  return (
    <div
      role="dialog"
      aria-modal="true"
      className="fixed inset-0 z-50 flex items-center justify-center p-4"
      data-testid="operation-modal"
    >
      <div className="fixed inset-0 bg-stone-950/60 backdrop-blur-xs transition-opacity animate-in fade-in duration-150" />
      <div className="relative bg-white rounded-2xl shadow-2xl border border-stone-200 z-50 p-6 sm:p-8 w-[calc(100%-2rem)] max-w-md focus:outline-none animate-in zoom-in-95 duration-150">
        <div className="flex flex-col items-center justify-center text-center gap-5">
          <div className="w-16 h-16 rounded-2xl bg-stone-900 text-white flex items-center justify-center shadow-md">
            <RefreshCw size={28} className="animate-spin text-amber-400" />
          </div>

          <div className="flex flex-col gap-2">
            <h3 className="text-xl font-bold tracking-tight text-stone-900 font-sans">
              {getTitle()}
            </h3>
            <p className="text-sm font-mono text-stone-600 bg-stone-100 py-1.5 px-3 rounded-lg border border-stone-200 break-all">
              {state.statusMessage || "Communicating with Shimano bus..."}
            </p>
          </div>



          <div className="p-3.5 rounded-xl bg-amber-50 border border-amber-200 text-amber-900 text-xs flex items-start gap-2.5 text-left leading-relaxed">
            <AlertTriangle size={18} className="text-amber-600 shrink-0 mt-0.5" />
            <span>{getNotice()}</span>
          </div>
        </div>
      </div>
    </div>
  );
}
