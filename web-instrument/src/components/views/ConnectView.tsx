import { h } from "preact";
import { useState } from "preact/hooks";
import {
  Usb,
  Bluetooth,
  Cpu,
  ChevronDown,
  AlertCircle,
  Smartphone,
} from "lucide-preact";
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
    <div className="min-h-screen flex flex-col items-center justify-center py-12 px-4 sm:px-8 bg-[#f4f3ef] text-stone-900 font-sans">
      <div className="w-full max-w-xl flex flex-col gap-6">
        {/* Branding header */}
        <div className="text-center">
          <div className="inline-flex items-center gap-2 px-3 py-1 rounded-full bg-stone-200/80 text-stone-700 text-xs font-mono font-medium mb-3">
            <span>cranklink</span>
            <span>•</span>
            <span data-testid="header-status-badge">
              {isBusy
                ? state.statusMessage.toUpperCase()
                : state.phase === "error"
                  ? "CONNECTION ERROR"
                  : "DISCONNECTED"}
            </span>
          </div>
          <h1 className="text-2xl font-bold tracking-tight text-stone-900 font-sans">
            Connect Your Bike
          </h1>
          <p className="text-sm text-stone-600 mt-1 max-w-sm mx-auto">
            Choose how your Shimano STEPS drive unit is connected to this device.
          </p>
        </div>

        {/* Error Banner if any */}
        {state.errorBanner && (
          <div
            className="p-4 rounded-xl bg-red-50 border border-red-200 text-red-900 flex items-start gap-3 shadow-xs"
            data-testid="error-banner"
          >
            <AlertCircle size={20} className="text-red-600 shrink-0 mt-0.5" />
            <div className="text-sm">
              <p className="font-semibold">Connection Failed</p>
              <p className="text-red-700 text-xs mt-0.5 font-mono break-all">
                {state.errorBanner}
              </p>
            </div>
          </div>
        )}

        {/* iOS platform guidance when not in Bluefy */}
        {plat.isIOS && !plat.isBluefy && (
          <div className="p-4 rounded-xl bg-amber-50 border border-amber-200 text-amber-900 flex items-start gap-3 text-xs leading-relaxed shadow-xs">
            <Smartphone size={20} className="text-amber-700 shrink-0 mt-0.5" />
            <div>
              <strong className="block font-semibold">iPhone & iPad Bluetooth Notice</strong>
              Apple Safari does not support Web Bluetooth. To connect to your Shimano display wirelessly from iOS, please open this app inside the free <strong>Bluefy Web BLE Browser</strong>.
            </div>
          </div>
        )}

        {/* Connection Options */}
        <div className="flex flex-col gap-4">
          {/* Option 1: USB Cable (Hidden on iOS) */}
          {!plat.isIOS && (
            <div className="bg-white border border-stone-200/90 rounded-2xl p-5 sm:p-6 shadow-xs flex flex-col sm:flex-row sm:items-center justify-between gap-5 hover:border-stone-300 transition-all">
              <div className="flex items-start gap-4">
                <div className="w-12 h-12 rounded-xl bg-stone-900 text-white flex items-center justify-center shrink-0 shadow-xs">
                  <Usb size={22} />
                </div>
                <div>
                  <div className="flex items-center gap-2 flex-wrap">
                    <h3 className="text-sm font-semibold text-stone-900">
                      Connect with USB Cable
                    </h3>
                    <span className="text-[10px] font-mono font-medium px-2 py-0.5 rounded-full bg-emerald-100 text-emerald-800">
                      Recommended
                    </span>
                  </div>
                  <p className="text-xs text-stone-500 mt-1 leading-relaxed">
                    SM-PCE02 or PCE1 diagnostic adapter plugged into computer & bike.
                  </p>
                </div>
              </div>
              <Button
                variant="primary"
                size="md"
                disabled={isBusy}
                onClick={() => controller.connectAndSync("usb").catch(() => {})}
                data-action="connect-usb"
                data-testid="btn-connect-usb"
                className="w-full sm:w-32 shrink-0 font-medium"
              >
                Connect
              </Button>
            </div>
          )}

          {/* Option 2: Bluetooth */}
          <div className="bg-white border border-stone-200/90 rounded-2xl p-5 sm:p-6 shadow-xs flex flex-col sm:flex-row sm:items-center justify-between gap-5 hover:border-stone-300 transition-all">
            <div className="flex items-start gap-4">
              <div className="w-12 h-12 rounded-xl bg-blue-600 text-white flex items-center justify-center shrink-0 shadow-xs">
                <Bluetooth size={22} />
              </div>
              <div>
                <div className="flex items-center gap-2 flex-wrap">
                  <h3 className="text-sm font-semibold text-stone-900">
                    Connect via Bluetooth
                  </h3>
                  <span className="text-[10px] font-mono font-medium px-2 py-0.5 rounded-full bg-blue-50 text-blue-700 border border-blue-200">
                    Wireless
                  </span>
                </div>
                <p className="text-xs text-stone-500 mt-1 leading-relaxed">
                  Wireless link for SC-EM800, EW-EN100, or SC-E5003 displays.
                </p>
              </div>
            </div>
            <Button
              variant="outline"
              size="md"
              disabled={isBusy}
              onClick={() => controller.connectAndSync("ble").catch(() => {})}
              data-action="connect-ble"
              data-testid="btn-connect-ble"
              className="w-full sm:w-32 shrink-0 font-medium"
            >
              Pair
            </Button>
          </div>

          {/* Advanced / Simulator Dropdown Card */}
          <div className="bg-white border border-stone-200/90 rounded-2xl p-5 sm:p-6 shadow-xs">
            <div className="flex items-center justify-between flex-wrap gap-2">
              <button
                type="button"
                onClick={() => setShowAdvanced(!showAdvanced)}
                className="text-xs font-medium text-stone-600 hover:text-stone-900 inline-flex items-center gap-2 py-2 px-1 min-h-[44px] transition-colors cursor-pointer select-none"
                data-testid="btn-more-menu"
              >
                <Cpu size={15} className="text-stone-500" />
                <span>Advanced & Simulator Options</span>
                <ChevronDown
                  size={15}
                  className={`text-stone-400 transition-transform duration-200 ${showAdvanced ? "rotate-180" : ""}`}
                />
              </button>

              <div className="flex items-center gap-1.5 flex-wrap">
                {plat.supportsUsb && <Badge variant="neutral">WebUSB OK</Badge>}
                {plat.supportsBluetooth && <Badge variant="neutral">BLE OK</Badge>}
                {plat.supportsSerial && <Badge variant="neutral">Serial OK</Badge>}
              </div>
            </div>

            {showAdvanced && (
              <div className="mt-4 pt-4 border-t border-stone-100 flex flex-col gap-4 menu-dropdown w-full max-w-full">
                <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
                  <div>
                    <label className="text-xs font-semibold text-stone-700 block mb-1.5">
                      Simulated Drive Unit
                    </label>
                    <select
                      value={simModel}
                      onChange={(e) => setSimModel((e.target as HTMLSelectElement).value as any)}
                      className="w-full text-xs font-mono p-2.5 rounded-lg border border-stone-300 bg-white text-stone-800 min-h-[40px] focus:border-stone-500 focus:outline-none"
                      data-role="sim-model"
                      data-testid="sim-model-select"
                    >
                      <option value="ep800-crg">DU-EP800-CRG (Cargo)</option>
                      <option value="ep600">DU-EP600</option>
                      <option value="ep801">DU-EP801 (Flagship)</option>
                    </select>
                  </div>
                  <div>
                    <label className="text-xs font-semibold text-stone-700 block mb-1.5">
                      Fault Injection
                    </label>
                    <select
                      value={simFault}
                      onChange={(e) => setSimFault((e.target as HTMLSelectElement).value as any)}
                      className="w-full text-xs font-mono p-2.5 rounded-lg border border-stone-300 bg-white text-stone-800 min-h-[40px] focus:border-stone-500 focus:outline-none"
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

                <div className="flex items-center justify-between pt-2 flex-wrap gap-3">
                  <Button
                    variant="outline"
                    size="sm"
                    disabled={isBusy}
                    onClick={() => controller.connectAndSync("serial").catch(() => {})}
                    data-action="connect-serial"
                    data-testid="btn-connect-serial"
                    className="text-xs font-medium"
                  >
                    Direct Serial Port
                  </Button>

                  <Button
                    variant="purple"
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

        {/* Visible Simulator Container for Testing & Evaluation */}
        <div
          className="bg-purple-50/80 border border-purple-200/90 rounded-2xl p-5 sm:p-6 shadow-xs flex flex-col sm:flex-row sm:items-center justify-between gap-4"
          data-testid="empty-state-start-sim"
        >
          <div className="flex items-start sm:items-center gap-3.5">
            <div className="w-10 h-10 rounded-xl bg-purple-100 text-purple-700 flex items-center justify-center shrink-0 shadow-xs mt-0.5 sm:mt-0">
              <Cpu size={20} />
            </div>
            <div>
              <h3 className="text-sm font-semibold text-purple-950 font-sans">
                Need to test without hardware?
              </h3>
              <p className="text-xs text-purple-800/80 mt-0.5">
                Evaluate assist tuning and firmware recovery with the built-in simulator.
              </p>
            </div>
          </div>
          <Button
            variant="purple"
            size="md"
            disabled={isBusy}
            onClick={() =>
              controller.connectAndSync("sim", { model: simModel, fault: simFault }).catch(() => {})
            }
            data-action="connect-sim"
            data-testid="btn-connect-sim-quick"
            className="w-full sm:w-auto shrink-0 px-5 text-xs font-mono font-medium shadow-xs"
          >
            Start Simulator
          </Button>
        </div>

        {/* Footer help note */}
        <p className="text-center text-xs text-stone-500">
          cranklink connects directly from your browser. No cloud drivers or third-party background software required.
        </p>
      </div>
    </div>
  );
}
