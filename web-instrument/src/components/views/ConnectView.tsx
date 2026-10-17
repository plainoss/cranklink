import { useState } from "react";
import {
  Usb,
  Bluetooth,
  Cpu,
  ChevronDown,
  AlertCircle,
  Smartphone,
  MoreHorizontal,
  Cable,
} from "lucide-react";
import type { InstrumentState, SimulatedModel, FaultInjectionType } from "../../types.ts";
import type { SessionController } from "../../core/sessionController.ts";
import { detectPlatform } from "../../core/platform.ts";
import { Button } from "../ui/Button.tsx";
import { Badge } from "../ui/Badge.tsx";

interface ConnectViewProps {
  state: InstrumentState;
  controller: SessionController;
}

export function ConnectView({ state, controller }: ConnectViewProps) {
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [simModel, setSimModel] = useState<SimulatedModel>("ep800-crg");
  const [simFault, setSimFault] = useState<FaultInjectionType>("none");

  const plat = detectPlatform();

  const isBusy =
    state.phase === "connecting" ||
    state.phase === "probing" ||
    state.phase === "reading";

  return (
    <div className="min-h-screen flex flex-col items-center justify-center py-10 px-4 sm:px-6 bg-[#f4f3ef] text-stone-900 font-sans">
      <div className="w-full max-w-md flex flex-col gap-6">
        {/* Sleek App Branding */}
        <div className="text-center flex flex-col items-center">
          <div className="w-14 h-14 rounded-2xl bg-stone-900 text-white flex items-center justify-center shadow-sm mb-3">
            <Bluetooth size={28} className="text-blue-400" />
          </div>
          <h1 className="text-2xl font-bold tracking-tight text-stone-900 font-sans">
            Connect Bike
          </h1>
          <p className="text-xs text-stone-500 mt-1">
            Shimano STEPS wireless and diagnostic link
          </p>
          <span
            data-testid="header-status-badge"
            className="mt-2 text-[11px] font-mono font-medium px-2.5 py-0.5 rounded-full bg-stone-200/80 text-stone-700"
          >
            {isBusy
              ? state.statusMessage.toUpperCase()
              : state.phase === "error"
                ? "CONNECTION ERROR"
                : "DISCONNECTED"}
          </span>
        </div>

        {/* Error Banner */}
        {state.errorBanner && (
          <div
            className="p-4 rounded-2xl bg-red-50 border border-red-200 text-red-900 flex items-start gap-3 shadow-xs"
            data-testid="error-banner"
          >
            <AlertCircle size={20} className="text-red-600 shrink-0 mt-0.5" />
            <div className="text-xs">
              <p className="font-semibold">Connection Failed</p>
              <p className="text-red-700 mt-0.5 font-mono break-all">
                {state.errorBanner}
              </p>
            </div>
          </div>
        )}

        {/* iOS Bluefy BLE Browser Guidance */}
        {plat.isIOS && !plat.isBluefy && (
          <div className="p-4 rounded-2xl bg-amber-50 border border-amber-200 text-amber-900 flex items-start gap-3 text-xs leading-relaxed shadow-xs">
            <Smartphone size={20} className="text-amber-700 shrink-0 mt-0.5" />
            <div>
              <strong className="block font-semibold">iPhone & iPad Notice</strong>
              Apple Safari does not support Web Bluetooth. Open this app inside the free <strong>Bluefy Web BLE Browser</strong> to connect wirelessly.
            </div>
          </div>
        )}

        {/* Primary Connection Surface */}
        <div className="bg-white border border-stone-200/90 rounded-3xl p-6 shadow-xs flex flex-col gap-4">
          {/* Main Primary Action: Bluetooth Connect */}
          <Button
            variant="primary"
            size="lg"
            disabled={isBusy}
            onClick={() => controller.connectAndSync("ble").catch(() => {})}
            data-action="connect-ble"
            data-testid="btn-connect-ble"
            className="w-full flex items-center justify-center gap-2.5 bg-stone-900 hover:bg-stone-800 text-white font-semibold text-sm py-3.5 rounded-2xl shadow-sm min-h-[48px]"
          >
            <Bluetooth size={18} className="text-blue-400" />
            <span>Connect via Bluetooth</span>
          </Button>

          {/* Secondary Wired USB Button (shown directly on desktop if supported) */}
          {!plat.isIOS && plat.supportsUsb && (
            <Button
              variant="outline"
              size="md"
              disabled={isBusy}
              onClick={() => controller.connectAndSync("usb").catch(() => {})}
              data-action="connect-usb"
              data-testid="btn-connect-usb"
              className="w-full flex items-center justify-center gap-2 border-stone-200 hover:bg-stone-50 text-stone-800 text-xs font-medium py-2.5 rounded-xl min-h-[44px]"
            >
              <Usb size={16} className="text-stone-600" />
              <span>Connect with USB Cable (SM-PCE02)</span>
            </Button>
          )}

          {/* Fallback hidden hook for USB testability if on iOS/mock */}
          {plat.isIOS && (
            <button
              type="button"
              data-testid="btn-connect-usb"
              data-action="connect-usb"
              onClick={() => controller.connectAndSync("usb").catch(() => {})}
              className="hidden"
            />
          )}

          {/* Advanced & Diagnostics Options Toggle (...) */}
          <div className="pt-2 border-t border-stone-100 flex flex-col">
            <button
              type="button"
              onClick={() => setShowAdvanced(!showAdvanced)}
              className="w-full flex items-center justify-between text-xs font-medium text-stone-500 hover:text-stone-900 py-2.5 px-2 rounded-xl hover:bg-stone-50 transition-colors cursor-pointer select-none min-h-[44px]"
              data-testid="btn-more-menu"
            >
              <div className="flex items-center gap-2">
                <MoreHorizontal size={16} />
                <span>Wired & Diagnostic Options</span>
              </div>
              <ChevronDown
                size={15}
                className={`text-stone-400 transition-transform duration-200 ${showAdvanced ? "rotate-180" : ""}`}
              />
            </button>

            {showAdvanced && (
              <div className="mt-2 pt-3 border-t border-stone-100 flex flex-col gap-3 menu-dropdown w-full">
                <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                  <div>
                    <label className="text-[11px] font-semibold text-stone-600 block mb-1">
                      Simulated Drive Unit
                    </label>
                    <select
                      value={simModel}
                      onChange={(e) => setSimModel(e.target.value as any)}
                      className="w-full text-xs font-mono p-2 rounded-xl border border-stone-200 bg-stone-50 text-stone-800 min-h-[40px] focus:outline-none focus:border-stone-400"
                      data-role="sim-model"
                      data-testid="sim-model-select"
                    >
                      <option value="ep800-crg">DU-EP800-CRG (Cargo)</option>
                      <option value="ep600">DU-EP600</option>
                      <option value="ep801">DU-EP801 (Flagship)</option>
                    </select>
                  </div>
                  <div>
                    <label className="text-[11px] font-semibold text-stone-600 block mb-1">
                      Fault Injection
                    </label>
                    <select
                      value={simFault}
                      onChange={(e) => setSimFault(e.target.value as any)}
                      className="w-full text-xs font-mono p-2 rounded-xl border border-stone-200 bg-stone-50 text-stone-800 min-h-[40px] focus:outline-none focus:border-stone-400"
                      data-role="sim-fault"
                      data-testid="sim-fault-select"
                    >
                      <option value="none">None (Healthy)</option>
                      <option value="auth_reject">Auth Rejection</option>
                      <option value="disconnect_on_connect">Disconnect On Connect</option>
                      <option value="write_timeout">Write Timeout</option>
                      <option value="readonly_rejection">Read-only Rejection</option>
                    </select>
                  </div>
                </div>

                <div className="flex items-center justify-between pt-2 gap-2 flex-wrap">
                  <Button
                    variant="outline"
                    size="sm"
                    disabled={isBusy}
                    onClick={() => controller.connectAndSync("serial").catch(() => {})}
                    data-action="connect-serial"
                    data-testid="btn-connect-serial"
                    className="text-xs"
                  >
                    Direct Serial Port
                  </Button>

                  <Button
                    variant="secondary"
                    size="md"
                    disabled={isBusy}
                    onClick={() =>
                      controller.connectAndSync("sim", { model: simModel, fault: simFault }).catch(() => {})
                    }
                    data-action="connect-sim"
                    data-testid="btn-connect-sim"
                    className="text-xs font-mono font-medium px-4 min-h-[44px] min-w-[44px]"
                  >
                    Start Simulator
                  </Button>
                </div>
              </div>
            )}
          </div>
        </div>

        {/* Refined Hardware Simulator Card (Preserves empty-state-start-sim for tests) */}
        <div
          className="bg-stone-100/80 border border-stone-200/80 rounded-2xl p-4 sm:p-5 flex items-center justify-between gap-3 text-xs"
          data-testid="empty-state-start-sim"
        >
          <div className="flex items-center gap-3">
            <div className="w-9 h-9 rounded-xl bg-stone-200 text-stone-700 flex items-center justify-center shrink-0">
              <Cpu size={18} />
            </div>
            <div>
              <strong className="block font-semibold text-stone-900 font-sans">
                Simulator Mode
              </strong>
              <span className="text-stone-500 text-[11px]">
                Test assist tuning and firmware recovery without hardware.
              </span>
            </div>
          </div>
          <Button
            variant="outline"
            size="md"
            disabled={isBusy}
            onClick={() =>
              controller.connectAndSync("sim", { model: simModel, fault: simFault }).catch(() => {})
            }
            data-action="connect-sim"
            data-testid="btn-connect-sim-quick"
            className="shrink-0 text-xs font-medium bg-white min-h-[44px] min-w-[44px]"
          >
            Launch
          </Button>
        </div>
      </div>
    </div>
  );
}
