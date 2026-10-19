import { useEffect, useState } from "react";
import { AlertCircle, Bluetooth, Cable, Check, ChevronDown, FlaskConical, Minus, Smartphone, Usb } from "lucide-react";
import type { InstrumentState, SimulatedModel, FaultInjectionType } from "../../types.ts";
import type { SessionController } from "../../core/sessionController.ts";
import { checkBluetoothAvailability, detectPlatform } from "../../core/platform.ts";
import { describeFailure } from "../../domain/errors.ts";
import { Button } from "../ui/Button.tsx";

const DOCS_URL = "https://github.com/plainoss/cranklink/tree/main/web-instrument#browser-instrument";

interface ConnectViewProps {
  state: InstrumentState;
  controller: SessionController;
  onOpenWarning: () => void;
}

export function ConnectView({ state, controller, onOpenWarning }: ConnectViewProps) {
  const [showOptions, setShowOptions] = useState(false);
  const [simModel, setSimModel] = useState<SimulatedModel>("ep800-crg");
  const [simFault, setSimFault] = useState<FaultInjectionType>("none");

  const platform = detectPlatform();
  // Browsers can expose Web Bluetooth while it is turned off (for example Brave's global switch).
  const [bleAvailable, setBleAvailable] = useState<boolean | null>(null);
  useEffect(() => {
    let active = true;
    checkBluetoothAvailability().then((available) => {
      if (active) setBleAvailable(available);
    });
    return () => {
      active = false;
    };
  }, []);
  const bluetoothBlocked = platform.supportsBluetooth && bleAvailable === false;

  const isBusy = ["connecting", "probing", "reading"].includes(state.phase);
  const canUseBluetooth = platform.supportsBluetooth && (!platform.isIOS || platform.isBluefy) && !bluetoothBlocked;
  const primaryTransport = canUseBluetooth
    ? "ble"
    : platform.supportsUsb
      ? "usb"
      : platform.supportsSerial
        ? "serial"
        : null;
  const hasBleOption = platform.supportsBluetooth && primaryTransport !== "ble";
  const hasOtherOptions =
    hasBleOption ||
    (platform.supportsUsb && primaryTransport !== "usb") ||
    (platform.supportsSerial && primaryTransport !== "serial");

  const connect = (transport: "ble" | "usb" | "serial") => {
    controller.connectAndSync(transport).catch(() => {});
  };

  const primaryLabel = primaryTransport === "ble"
    ? "Connect via Bluetooth"
    : primaryTransport === "usb"
      ? "Connect via USB"
      : primaryTransport === "serial"
        ? "Connect via serial cable"
        : platform.isIOS && !platform.isBluefy
          ? "Open in Bluefy to connect"
          : bluetoothBlocked
            ? "Open in Chrome or Edge to connect"
            : "Connection unavailable";

  const failure = state.failure ? describeFailure(state.failure, state.transport, state.schema) : null;

  return (
    <main className="min-h-dvh flex flex-col items-center justify-center px-4 py-8 sm:px-6 bg-[#f4f3ef] text-stone-900">
      <div className="w-full max-w-md flex flex-col gap-4">
        <header className="flex flex-col gap-2 px-1">
          <div className="flex items-center gap-2.5">
            <div className="w-9 h-9 rounded-xl bg-stone-900 text-white flex items-center justify-center" aria-hidden="true">
              <span className="font-bold text-xs font-mono">CL</span>
            </div>
            <h1 className="text-2xl font-bold tracking-tight">cranklink</h1>
          </div>
          <p className="text-sm text-stone-600 leading-relaxed">
            Read and adjust settings, back up, and update firmware on Shimano STEPS drive units from your browser.
          </p>
        </header>

        {failure && (
          <div className="p-4 rounded-2xl bg-red-50 border border-red-200 text-red-950 flex items-start gap-3" data-testid="error-banner" role="alert">
            <AlertCircle size={20} className="text-red-600 shrink-0 mt-0.5" aria-hidden="true" />
            <div className="text-sm min-w-0">
              <p className="font-semibold">{failure.title}</p>
              <p className="text-red-800 mt-1 [overflow-wrap:anywhere]">{failure.message}</p>
            </div>
          </div>
        )}

        {bluetoothBlocked && !platform.isIOS && (
          <div
            className="p-4 rounded-2xl bg-amber-50 border border-amber-200 text-amber-900 flex items-start gap-3 text-sm leading-relaxed"
            data-testid="bluetooth-unavailable-warning"
            role="status"
          >
            <AlertCircle size={20} className="text-amber-700 shrink-0 mt-0.5" aria-hidden="true" />
            <div>
              <strong className="block font-semibold">Web Bluetooth is unavailable or turned off</strong>
              <span>
                Use a browser with Web Bluetooth, such as Chrome or Edge, or check this browser’s Bluetooth settings.
                {platform.supportsUsb && " You can also connect with an SM-PCE02 USB adapter."}
              </span>
            </div>
          </div>
        )}

        {platform.isIOS && !platform.isBluefy && (
          <div className="p-4 rounded-2xl bg-amber-50 border border-amber-200 text-amber-900 flex items-start gap-3 text-sm leading-relaxed">
            <Smartphone size={20} className="text-amber-700 shrink-0 mt-0.5" aria-hidden="true" />
            <div>
              <strong className="block font-semibold">Use Bluefy on iPhone or iPad</strong>
              <span>Safari does not support Web Bluetooth. Open this page in the Bluefy browser to connect wirelessly.</span>
            </div>
          </div>
        )}

        <section aria-label="Connect" className="bg-white border border-stone-200/90 rounded-3xl p-5 sm:p-6 shadow-xs flex flex-col gap-4">
          <Button
            variant="primary"
            size="lg"
            disabled={isBusy || !primaryTransport}
            onClick={() => primaryTransport && connect(primaryTransport)}
            data-action={primaryTransport ? `connect-${primaryTransport}` : "connect-unavailable"}
            data-testid={primaryTransport ? `btn-connect-${primaryTransport}` : "btn-connect-unavailable"}
            className="w-full gap-2.5"
          >
            {primaryTransport === "ble" ? <Bluetooth size={18} aria-hidden="true" /> : <Usb size={18} aria-hidden="true" />}
            <span>{primaryLabel}</span>
          </Button>

          {canUseBluetooth && (
            <p className="text-xs text-stone-500 leading-relaxed" data-testid="bluetooth-discovery-hint">
              Turn the bike on just before connecting. If it isn’t listed, turn it off and on again, then enable its
              Bluetooth connection mode if needed.
            </p>
          )}

          <TransportSupport platform={platform} bluetooth={canUseBluetooth} />

          {hasOtherOptions && (
            <div>
              <button
                type="button"
                onClick={() => setShowOptions((open) => !open)}
                aria-expanded={showOptions}
                aria-controls="other-connections"
                className="w-full min-h-[44px] flex items-center justify-between px-3 rounded-xl text-sm text-stone-600 hover:text-stone-900 hover:bg-stone-50 transition-colors"
                data-testid="btn-more-menu"
              >
                <span>Other ways to connect</span>
                <ChevronDown size={16} className={`text-stone-400 transition-transform ${showOptions ? "rotate-180" : ""}`} aria-hidden="true" />
              </button>

              {showOptions && (
                <div id="other-connections" className="mt-2 pt-3 border-t border-stone-100 flex flex-col gap-2 menu-dropdown">
                  {hasBleOption && (
                    <Button variant="outline" size="md" disabled={isBusy} onClick={() => connect("ble")} data-action="connect-ble" data-testid="btn-connect-ble" className="w-full justify-start gap-2">
                      <Bluetooth size={16} aria-hidden="true" />
                      <span>Bluetooth gateway</span>
                    </Button>
                  )}
                  {platform.supportsUsb && primaryTransport !== "usb" && (
                    <Button variant="outline" size="md" disabled={isBusy} onClick={() => connect("usb")} data-action="connect-usb" data-testid="btn-connect-usb" className="w-full justify-start gap-2">
                      <Usb size={16} aria-hidden="true" />
                      <span>USB adapter (SM-PCE02)</span>
                    </Button>
                  )}
                  {platform.supportsSerial && primaryTransport !== "serial" && (
                    <Button variant="outline" size="md" disabled={isBusy} onClick={() => connect("serial")} data-action="connect-serial" data-testid="btn-connect-serial" className="w-full justify-start gap-2">
                      <Cable size={16} aria-hidden="true" />
                      <span>Direct serial port</span>
                    </Button>
                  )}
                </div>
              )}
            </div>
          )}
        </section>

        {import.meta.env.DEV && (
          <section aria-labelledby="sim-title" className="bg-white/70 border border-dashed border-stone-300 rounded-3xl p-5 flex flex-col gap-3" data-testid="empty-state-start-sim">
            <h2 id="sim-title" className="flex items-center gap-2 text-sm font-semibold text-stone-800">
              <FlaskConical size={16} aria-hidden="true" />
              Try without hardware (development build)
            </h2>
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
              <label className="text-xs font-medium text-stone-600">
                Drive unit
                <select
                  value={simModel}
                  onChange={(event) => setSimModel(event.target.value as SimulatedModel)}
                  className="mt-1 w-full text-sm p-2 rounded-xl border border-stone-200 bg-white text-stone-800 min-h-[44px]"
                  data-testid="sim-model-select"
                >
                  <option value="ep800-crg">DU-EP800-CRG (Cargo)</option>
                  <option value="ep600">DU-EP600</option>
                  <option value="ep801">DU-EP801</option>
                </select>
              </label>
              <label className="text-xs font-medium text-stone-600">
                Simulated fault
                <select
                  value={simFault}
                  onChange={(event) => setSimFault(event.target.value as FaultInjectionType)}
                  className="mt-1 w-full text-sm p-2 rounded-xl border border-stone-200 bg-white text-stone-800 min-h-[44px]"
                  data-testid="sim-fault-select"
                >
                  <option value="none">None</option>
                  <option value="auth_reject">Motor rejects authorization</option>
                  <option value="disconnect_on_connect">Gateway drops on connect</option>
                  <option value="write_timeout">Gateway stops responding on write</option>
                  <option value="readonly_rejection">Motor rejects writes</option>
                </select>
              </label>
            </div>
            <p className="text-xs text-stone-500">
              The simulator emulates a Bluetooth gateway, so simulated faults surface as Bluetooth errors.
            </p>
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
          </section>
        )}

        <p className="px-1 text-xs text-stone-500 leading-relaxed">
          Needs Chrome or Edge with Web Bluetooth, WebUSB, or Web Serial, and a compatible Bluetooth gateway or SM-PCE02 adapter.{" "}
          <a href={DOCS_URL} target="_blank" rel="noreferrer" className="font-medium text-stone-700 underline underline-offset-2">
            Requirements and limitations
          </a>
        </p>
        <button
          type="button"
          onClick={onOpenWarning}
          data-testid="btn-open-warning-modal"
          className="self-start px-1 text-xs text-stone-600 underline underline-offset-2 hover:text-stone-900 min-h-[32px]"
        >
          Unofficial software &amp; hardware safety notice
        </button>
      </div>
    </main>
  );
}

function TransportSupport({ platform, bluetooth }: { platform: ReturnType<typeof detectPlatform>; bluetooth: boolean }) {
  const rows = [
    { label: "Bluetooth", ok: bluetooth },
    { label: "USB", ok: platform.supportsUsb },
    { label: "Serial", ok: platform.supportsSerial },
  ];
  return (
    <ul className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-stone-600" aria-label="Connections this browser supports">
      {rows.map((row) => (
        <li key={row.label} className="flex items-center gap-1">
          {row.ok
            ? <Check size={13} className="text-emerald-600" aria-hidden="true" />
            : <Minus size={13} className="text-stone-400" aria-hidden="true" />}
          <span className={row.ok ? "" : "text-stone-400"}>
            {row.label}
            <span className="sr-only">{row.ok ? " supported" : " not supported in this browser"}</span>
          </span>
        </li>
      ))}
    </ul>
  );
}
