import { h } from "preact";
import { Dialog as BaseDialog } from "@base-ui/react/dialog";
import { RefreshCw, AlertTriangle } from "lucide-preact";
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
    <BaseDialog.Root open={isBusy} onOpenChange={() => {}}>
      <BaseDialog.Portal>
        <BaseDialog.Backdrop className="fixed inset-0 bg-stone-950/60 backdrop-blur-xs z-50 transition-opacity" />
        <BaseDialog.Popup
          className="fixed top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 bg-white rounded-2xl shadow-2xl border border-stone-200 z-50 p-6 sm:p-8 w-[calc(100%-2rem)] max-w-md focus:outline-none"
          data-testid="operation-modal"
        >
          <div className="flex flex-col items-center justify-center text-center gap-5">
            {/* Animated Spinner with high-contrast icon badge */}
            <div className="w-16 h-16 rounded-2xl bg-stone-900 text-white flex items-center justify-center shadow-md">
              <RefreshCw size={28} className="animate-spin text-amber-400" />
            </div>

            {/* Title & Live Status */}
            <div className="flex flex-col gap-2 w-full max-w-sm">
              <BaseDialog.Title className="text-lg font-bold tracking-tight text-stone-900 font-sans">
                {getTitle()}
              </BaseDialog.Title>
              <BaseDialog.Description className="text-xs font-mono text-stone-600 bg-stone-100 py-2 px-3 rounded-lg border border-stone-200/90 break-all leading-relaxed">
                {state.statusMessage || "Working..."}
              </BaseDialog.Description>
            </div>

            {/* Reassuring safety notice */}
            <div className="flex items-start gap-2.5 text-xs text-amber-900 bg-amber-50/90 border border-amber-200 rounded-xl p-3.5 text-left w-full shadow-xs">
              <AlertTriangle size={16} className="text-amber-600 shrink-0 mt-0.5" />
              <span className="leading-relaxed font-medium">{getNotice()}</span>
            </div>
          </div>
        </BaseDialog.Popup>
      </BaseDialog.Portal>
    </BaseDialog.Root>
  );
}
