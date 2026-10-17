import { h } from "preact";
import { useState } from "preact/hooks";
import {
  Usb,
  Bluetooth,
  Cpu,
  ChevronDown,
  AlertCircle,
  RefreshCw,
  Smartphone,
  ExternalLink,
} from "lucide-preact";
import type { InstrumentState, SimulatedModel, FaultInjectionType } from "../../types.ts";
import type { SessionController } from "../../core/sessionController.ts";
import { detectPlatform } from "../../core/platform.ts";
import { Button } from "../ui/Button.tsx";
import { Card } from "../ui/Card.tsx";
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
    <div className="min-h-screen flex flex-col items-center justify-center py-12 px-6 sm:px-8 bg-[#f4f3ef] text-stone-900 font-sans">
      <div className="w-full max-w-xl flex flex-col gap-8">
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

        {/* Primary Action Card */}
        <Card className="flex flex-col gap-4 shadow-sm border-stone-300">
          {/* Option 1: USB Cable (Hidden on iOS) */}
          {!plat.isIOS && (
            <div className="p-4 rounded-lg border border-stone-200 bg-stone-50/50 hover:bg-stone-50 transition-colors flex flex-col sm:flex-row sm:items-center justify-between gap-4">
              <div className="flex items-start gap-3.5">
                <div className="w-10 h-10 rounded-lg bg-stone-900 text-white flex items-center justify-center shrink-0 shadow-xs">
                  <Usb size={20} />
                </div>
                <div>
                  <div className="flex items-center gap-2">
                    <h3 className="text-sm font-semibold text-stone-900">
                      Connect with USB Cable
                    </h3>
                    <span className="text-[10px] font-mono font-medium px-2 py-0.5 rounded-full bg-emerald-100 text-emerald-800">
                      Recommended
                    </span>
                  </div>
                  <p className="text-xs text-stone-500 mt-0.5">
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
                className="sm:w-28 shrink-0"
              >
                {isBusy && state.transport === "usb" ? (
                  <RefreshCw size={16} className="animate-spin" />
                ) : (
                  "Connect"
                )}
              </Button>
            </div>
          )}

          {/* Option 2: Bluetooth */}
          <div className="p-4 rounded-lg border border-stone-200 bg-stone-50/50 hover:bg-stone-50 transition-colors flex flex-col sm:flex-row sm:items-center justify-between gap-4">
            <div className="flex items-start gap-3.5">
              <div className="w-10 h-10 rounded-lg bg-blue-600 text-white flex items-center justify-center shrink-0 shadow-xs">
                <Bluetooth size={20} />
              </div>
              <div>
                <div className="flex items-center gap-2">
                  <h3 className="text-sm font-semibold text-stone-900">
                    Connect via Bluetooth
                  </h3>
                </div>
                <p className="text-xs text-stone-500 mt-0.5">
                  Wireless link for SC-EM800, EW-EN100, or SC-E5003 displays.
                </p>
              </div>
            </div>
            <Button
              variant="secondary"
              size="md"
              disabled={isBusy}
              onClick={() => controller.connectAndSync("ble").catch(() => {})}
              data-action="connect-ble"
              data-testid="btn-connect-ble"
              className="sm:w-28 shrink-0 bg-white"
            >
              {isBusy && state.transport === "ble" ? (
                <RefreshCw size={16} className="animate-spin" />
              ) : (
                "Pair"
              )}
            </Button>
          </div>

          {/* Busy progress status */}
          {isBusy && (
            <div className="p-3.5 rounded-lg bg-stone-100 border border-stone-200 flex items-center gap-3 text-stone-800 text-sm">
              <RefreshCw size={18} className="animate-spin text-stone-600 shrink-0" />
              <div className="flex flex-col">
                <span className="font-medium text-xs text-stone-900 font-mono">
                  {state.statusMessage}
                </span>
                <span className="text-[11px] text-stone-500">
                  Please keep communication cables attached...
                </span>
              </div>
            </div>
          )}

          {/* Advanced / Simulator Dropdown */}
          <div className="pt-2 border-t border-stone-200">
            <div className="flex items-center justify-between">
              <button
                type="button"
                onClick={() => setShowAdvanced(!showAdvanced)}
                className="text-xs text-stone-500 hover:text-stone-800 inline-flex items-center gap-1.5 py-2 px-1 min-h-[44px] transition-colors cursor-pointer select-none"
                data-testid="btn-more-menu"
              >
                <Cpu size={14} />
                <span>Advanced & Simulator Options</span>
                <ChevronDown
                  size={14}
                  className={`transition-transform duration-200 ${showAdvanced ? "rotate-180" : ""}`}
                />
              </button>

              <div className="flex items-center gap-1.5">
                {plat.supportsUsb && <Badge variant="neutral">WebUSB OK</Badge>}
                {plat.supportsBluetooth && <Badge variant="neutral">BLE OK</Badge>}
                {plat.supportsSerial && <Badge variant="neutral">Serial OK</Badge>}
              </div>
            </div>

            {showAdvanced && (
              <div className="mt-3 p-4 rounded-lg bg-stone-50 border border-stone-200 flex flex-col gap-3 menu-dropdown w-full max-w-full">
                <div className="flex flex-col sm:flex-row gap-3">
                  <div className="flex-1">
                    <label className="text-xs font-semibold text-stone-700 block mb-1">
                      Simulated Drive Unit
                    </label>
                    <select
                      value={simModel}
                      onChange={(e) => setSimModel((e.target as HTMLSelectElement).value as any)}
                      className="w-full text-xs font-mono p-2 rounded border border-stone-300 bg-white"
                      data-role="sim-model"
                      data-testid="sim-model-select"
                    >
                      <option value="ep800-crg">DU-EP800-CRG (Cargo)</option>
                      <option value="ep600">DU-EP600</option>
                      <option value="ep801">DU-EP801 (Flagship)</option>
                    </select>
                  </div>
                  <div className="flex-1">
                    <label className="text-xs font-semibold text-stone-700 block mb-1">
                      Fault Injection
                    </label>
                    <select
                      value={simFault}
                      onChange={(e) => setSimFault((e.target as HTMLSelectElement).value as any)}
                      className="w-full text-xs font-mono p-2 rounded border border-stone-300 bg-white"
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

                <div className="flex items-center justify-between pt-1">
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
                    className="bg-purple-100 hover:bg-purple-200 text-purple-900 border-purple-300 text-xs font-mono font-medium min-h-[44px] min-w-[44px]"
                  >
                    Start Simulator
                  </Button>
                </div>
              </div>
            )}
          </div>
        </Card>

        {/* Visible Simulator Container for Testing & Evaluation */}
        <div
          className="p-3.5 rounded-xl border border-purple-200 bg-purple-50/70 flex items-center justify-between gap-3 text-xs"
          data-testid="empty-state-start-sim"
        >
          <div className="flex items-center gap-2">
            <Cpu size={16} className="text-purple-700 shrink-0" />
            <span className="font-semibold text-purple-950 font-sans">
              Need to test or evaluate without physical hardware?
            </span>
          </div>
          <Button
            variant="secondary"
            size="sm"
            disabled={isBusy}
            onClick={() =>
              controller.connectAndSync("sim", { model: simModel, fault: simFault }).catch(() => {})
            }
            data-action="connect-sim"
            data-testid="btn-connect-sim-quick"
            className="bg-purple-600 hover:bg-purple-700 text-white border-none text-xs font-mono font-medium shrink-0 shadow-xs"
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
