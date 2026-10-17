import { useState } from "react";
import {
  AlertCircle,
  Bluetooth,
  Cable,
  ChevronDown,
  MoreHorizontal,
  Smartphone,
  Usb,
} from "lucide-react";
import type { InstrumentState, SimulatedModel, FaultInjectionType } from "../../types.ts";
import type { SessionController } from "../../core/sessionController.ts";
import { detectPlatform } from "../../core/platform.ts";
import { Button } from "../ui/Button.tsx";

interface ConnectViewProps {
  state: InstrumentState;
  controller: SessionController;
}

export function ConnectView({ state, controller }: ConnectViewProps) {
  const [showOptions, setShowOptions] = useState(false);
  const [simModel, setSimModel] = useState<SimulatedModel>("ep800-crg");
  const [simFault, setSimFault] = useState<FaultInjectionType>("none");

  const platform = detectPlatform();
  const isBusy = ["connecting", "probing", "reading"].includes(state.phase);
  const canUseBluetooth = platform.supportsBluetooth && (!platform.isIOS || platform.isBluefy);
  const primaryTransport = canUseBluetooth
    ? "ble"
    : platform.supportsUsb
      ? "usb"
      : platform.supportsSerial
        ? "serial"
        : null;
  const hasWiredOptions =
    (platform.supportsUsb && primaryTransport !== "usb") ||
    (platform.supportsSerial && primaryTransport !== "serial");
  const hasOptions = hasWiredOptions || import.meta.env.DEV;

  const connect = (transport: "ble" | "usb" | "serial") => {
    controller.connectAndSync(transport).catch(() => {});
  };

  const primaryLabel = isBusy
    ? state.statusMessage || "Connecting…"
    : primaryTransport === "ble"
      ? "Connect Cranklink via Bluetooth"
      : primaryTransport === "usb"
        ? "Connect with USB"
        : primaryTransport === "serial"
          ? "Connect with cable"
          : platform.isIOS && !platform.isBluefy
            ? "Open in Bluefy to connect"
            : "Connection unavailable";

  return (
    <main className="min-h-screen flex flex-col items-center justify-center px-4 py-8 sm:px-6 bg-[#f4f3ef] text-stone-900 font-sans">
      <div className="w-full max-w-md flex flex-col gap-4">
        {state.errorBanner && (
          <div
            className="p-4 rounded-2xl bg-red-50 border border-red-200 text-red-900 flex items-start gap-3"
            data-testid="error-banner"
            role="alert"
          >
            <AlertCircle size={20} className="text-red-600 shrink-0 mt-0.5" />
            <div className="text-sm min-w-0">
              <p className="font-semibold">Couldn’t connect</p>
              <p className="text-red-700 mt-1 break-words">{state.errorBanner}</p>
            </div>
          </div>
        )}

        {platform.isIOS && !platform.isBluefy && (
          <div className="p-4 rounded-2xl bg-amber-50 border border-amber-200 text-amber-900 flex items-start gap-3 text-sm leading-relaxed">
            <Smartphone size={20} className="text-amber-700 shrink-0 mt-0.5" />
            <div>
              <strong className="block font-semibold">Use Bluefy on iPhone or iPad</strong>
              <span>
                Safari does not support Web Bluetooth. Open this page in the Bluefy browser to connect wirelessly.
              </span>
            </div>
          </div>
        )}

        {!primaryTransport && !(platform.isIOS && !platform.isBluefy) && (
          <div className="p-4 rounded-2xl bg-amber-50 border border-amber-200 text-amber-900 text-sm leading-relaxed">
            This browser does not provide a compatible connection. Try Chrome or Edge on a supported desktop device.
          </div>
        )}

        <section className="bg-white border border-stone-200/90 rounded-3xl p-5 sm:p-6 shadow-xs">
          <Button
            variant="custom"
            size="lg"
            disabled={isBusy || !primaryTransport}
            onClick={() => primaryTransport && connect(primaryTransport)}
            data-action={primaryTransport ? `connect-${primaryTransport}` : "connect-unavailable"}
            data-testid={primaryTransport === "ble" ? "btn-connect-ble" : primaryTransport === "usb" ? "btn-connect-usb" : primaryTransport === "serial" ? "btn-connect-serial" : "btn-connect-unavailable"}
            className="w-full gap-2.5 bg-stone-900 hover:bg-stone-800 active:bg-black text-white"
          >
            {primaryTransport === "ble" ? <Bluetooth size={18} className="text-blue-300" /> : <Usb size={18} />}
            <span>{primaryLabel}</span>
          </Button>

          {hasOptions && (
            <div className="mt-3">
              <button
                type="button"
                onClick={() => setShowOptions((open) => !open)}
                aria-expanded={showOptions}
                className="w-full min-h-[44px] flex items-center justify-between px-3 rounded-xl text-sm text-stone-500 hover:text-stone-900 hover:bg-stone-50 transition-colors"
                data-testid="btn-more-menu"
              >
                <span className="flex items-center gap-2">
                  <MoreHorizontal size={17} />
                  <span>More connection options</span>
                </span>
                <ChevronDown
                  size={16}
                  className={`text-stone-400 transition-transform ${showOptions ? "rotate-180" : ""}`}
                />
              </button>

              {showOptions && (
                <div className="mt-2 pt-3 border-t border-stone-100 flex flex-col gap-2 menu-dropdown">
                  {platform.supportsUsb && primaryTransport !== "usb" && (
                    <Button
                      variant="outline"
                      size="md"
                      disabled={isBusy}
                      onClick={() => connect("usb")}
                      data-action="connect-usb"
                      data-testid="btn-connect-usb"
                      className="w-full justify-start gap-2"
                    >
                      <Usb size={16} />
                      <span>USB (SM-PCE02)</span>
                    </Button>
                  )}
                  {platform.supportsSerial && primaryTransport !== "serial" && (
                    <Button
                      variant="outline"
                      size="md"
                      disabled={isBusy}
                      onClick={() => connect("serial")}
                      data-action="connect-serial"
                      data-testid="btn-connect-serial"
                      className="w-full justify-start gap-2"
                    >
                      <Cable size={16} />
                      <span>Direct serial port</span>
                    </Button>
                  )}
                  {import.meta.env.DEV && (
                    <div className="mt-2 border-t border-stone-100 pt-3 flex flex-col gap-3">
                      <p className="text-xs font-medium text-stone-500">Development simulator</p>
                      <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                        <label className="text-xs font-medium text-stone-600">
                          Drive unit
                          <select
                            value={simModel}
                            onChange={(event) => setSimModel(event.target.value as SimulatedModel)}
                            className="mt-1 w-full text-xs p-2 rounded-xl border border-stone-200 bg-white text-stone-800 min-h-[40px]"
                            data-role="sim-model"
                            data-testid="sim-model-select"
                          >
                            <option value="ep800-crg">DU-EP800-CRG (Cargo)</option>
                            <option value="ep600">DU-EP600</option>
                            <option value="ep801">DU-EP801 (Flagship)</option>
                          </select>
                        </label>
                        <label className="text-xs font-medium text-stone-600">
                          Fault injection
                          <select
                            value={simFault}
                            onChange={(event) => setSimFault(event.target.value as FaultInjectionType)}
                            className="mt-1 w-full text-xs p-2 rounded-xl border border-stone-200 bg-white text-stone-800 min-h-[40px]"
                            data-role="sim-fault"
                            data-testid="sim-fault-select"
                          >
                            <option value="none">None (Healthy)</option>
                            <option value="auth_reject">Auth Rejection</option>
                            <option value="disconnect_on_connect">Disconnect On Connect</option>
                            <option value="write_timeout">Write Timeout</option>
                            <option value="readonly_rejection">Read-only Rejection</option>
                          </select>
                        </label>
                      </div>
                      <Button
                        variant="outline"
                        size="md"
                        disabled={isBusy}
                        onClick={() => controller.connectAndSync("sim", { model: simModel, fault: simFault }).catch(() => {})}
                        data-action="connect-sim"
                        data-testid="btn-connect-sim"
                        className="w-full min-h-[44px]"
                      >
                        Start simulator
                      </Button>
                    </div>
                  )}
                </div>
              )}
            </div>
          )}
        </section>

        {import.meta.env.DEV && <span className="sr-only" data-testid="empty-state-start-sim">Development simulator options are available in the connection menu.</span>}
      </div>
    </main>
  );
}
