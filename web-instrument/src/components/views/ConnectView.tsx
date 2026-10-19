import { useEffect, useState } from "react";
import { Bluetooth, Cable, Check, FlaskConical, Minus, Smartphone, Usb } from "lucide-react";
import { Alert, Button, Card, Chip, Disclosure, Link } from "@heroui/react";
import type { InstrumentState, SimulatedModel, FaultInjectionType } from "../../types.ts";
import type { SessionController } from "../../core/sessionController.ts";
import { checkBluetoothAvailability, detectPlatform, type PlatformInfo } from "../../core/platform.ts";
import { describeFailure } from "../../domain/errors.ts";
import { OptionSelect } from "../ui/OptionSelect.tsx";

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
    <main className="min-h-dvh flex flex-col items-center justify-center px-4 py-8 sm:px-6 bg-background text-foreground">
      <div className="w-full max-w-md flex flex-col gap-4">
        <header className="flex flex-col gap-2 px-1">
          <div className="flex items-center gap-2.5">
            <div className="w-9 h-9 rounded-xl bg-accent text-accent-foreground flex items-center justify-center" aria-hidden="true">
              <span className="font-bold text-xs font-mono">CL</span>
            </div>
            <h1 className="text-2xl font-bold tracking-tight">cranklink</h1>
          </div>
          <p className="text-sm text-muted leading-relaxed">
            Read and adjust settings, back up, and update firmware on Shimano STEPS drive units from your browser.
          </p>
        </header>

        {failure && (
          <Alert status="danger" data-testid="error-banner">
            <Alert.Indicator />
            <Alert.Content className="min-w-0">
              <Alert.Title>{failure.title}</Alert.Title>
              <Alert.Description className="[overflow-wrap:anywhere]">{failure.message}</Alert.Description>
            </Alert.Content>
          </Alert>
        )}

        {bluetoothBlocked && !platform.isIOS && (
          <Alert status="warning" data-testid="bluetooth-unavailable-warning">
            <Alert.Indicator />
            <Alert.Content>
              <Alert.Title>Web Bluetooth is unavailable or turned off</Alert.Title>
              <Alert.Description>
                Use a browser with Web Bluetooth, such as Chrome or Edge, or check this browser’s Bluetooth settings.
                {platform.supportsUsb && " You can also connect with an SM-PCE02 USB adapter."}
              </Alert.Description>
            </Alert.Content>
          </Alert>
        )}

        {platform.isIOS && !platform.isBluefy && (
          <Alert status="warning">
            <Alert.Indicator>
              <Smartphone size={20} aria-hidden="true" />
            </Alert.Indicator>
            <Alert.Content>
              <Alert.Title>Use Bluefy on iPhone or iPad</Alert.Title>
              <Alert.Description>
                Safari does not support Web Bluetooth. Open this page in the Bluefy browser to connect wirelessly.
              </Alert.Description>
            </Alert.Content>
          </Alert>
        )}

        <Card aria-label="Connect" className="gap-4">
          <Button
            size="lg"
            fullWidth
            isDisabled={isBusy || !primaryTransport}
            onPress={() => primaryTransport && connect(primaryTransport)}
            data-action={primaryTransport ? `connect-${primaryTransport}` : "connect-unavailable"}
            data-testid={primaryTransport ? `btn-connect-${primaryTransport}` : "btn-connect-unavailable"}
          >
            {primaryTransport === "ble" ? <Bluetooth size={18} aria-hidden="true" /> : <Usb size={18} aria-hidden="true" />}
            {primaryLabel}
          </Button>

          {canUseBluetooth && (
            <p className="text-xs text-muted leading-relaxed" data-testid="bluetooth-discovery-hint">
              Turn the bike on just before connecting. If it isn’t listed, turn it off and on again, then enable its
              Bluetooth connection mode if needed.
            </p>
          )}

          <TransportSupport platform={platform} bluetooth={canUseBluetooth} />

          {hasOtherOptions && (
            <Disclosure>
              <Disclosure.Heading>
                <Disclosure.Trigger className="w-full min-h-[44px] flex items-center justify-between text-sm text-muted" data-testid="btn-more-menu">
                  Other ways to connect
                  <Disclosure.Indicator />
                </Disclosure.Trigger>
              </Disclosure.Heading>
              <Disclosure.Content>
                <Disclosure.Body className="pt-2 flex flex-col gap-2 menu-dropdown">
                  {hasBleOption && (
                    <Button variant="outline" fullWidth isDisabled={isBusy} onPress={() => connect("ble")} data-action="connect-ble" data-testid="btn-connect-ble" className="justify-start">
                      <Bluetooth size={16} aria-hidden="true" />
                      Bluetooth gateway
                    </Button>
                  )}
                  {platform.supportsUsb && primaryTransport !== "usb" && (
                    <Button variant="outline" fullWidth isDisabled={isBusy} onPress={() => connect("usb")} data-action="connect-usb" data-testid="btn-connect-usb" className="justify-start">
                      <Usb size={16} aria-hidden="true" />
                      USB adapter (SM-PCE02)
                    </Button>
                  )}
                  {platform.supportsSerial && primaryTransport !== "serial" && (
                    <Button variant="outline" fullWidth isDisabled={isBusy} onPress={() => connect("serial")} data-action="connect-serial" data-testid="btn-connect-serial" className="justify-start">
                      <Cable size={16} aria-hidden="true" />
                      Direct serial port
                    </Button>
                  )}
                </Disclosure.Body>
              </Disclosure.Content>
            </Disclosure>
          )}
        </Card>

        {import.meta.env.DEV && (
          <Card variant="secondary" className="gap-3 border border-dashed border-separator" data-testid="empty-state-start-sim">
            <Card.Header className="p-0">
              <Card.Title className="flex items-center gap-2 text-sm font-semibold">
                <FlaskConical size={16} aria-hidden="true" />
                Try without hardware (development build)
              </Card.Title>
            </Card.Header>
            <Card.Content className="p-0 grid grid-cols-1 sm:grid-cols-2 gap-3">
              <OptionSelect
                label="Drive unit"
                value={simModel}
                onChange={(id) => setSimModel(id as SimulatedModel)}
                options={[
                  { id: "ep800-crg", label: "DU-EP800-CRG (Cargo)" },
                  { id: "ep600", label: "DU-EP600" },
                  { id: "ep801", label: "DU-EP801" },
                ]}
                data-testid="sim-model-select"
              />
              <OptionSelect
                label="Simulated fault"
                value={simFault}
                onChange={(id) => setSimFault(id as FaultInjectionType)}
                options={[
                  { id: "none", label: "None" },
                  { id: "auth_reject", label: "Motor rejects authorization" },
                  { id: "disconnect_on_connect", label: "Gateway drops on connect" },
                  { id: "write_timeout", label: "Gateway stops responding on write" },
                  { id: "readonly_rejection", label: "Motor rejects writes" },
                ]}
                data-testid="sim-fault-select"
              />
            </Card.Content>
            <p className="text-xs text-muted">
              The simulator emulates a Bluetooth gateway, so simulated faults surface as Bluetooth errors.
            </p>
            <Button
              variant="outline"
              fullWidth
              isDisabled={isBusy}
              onPress={() => controller.connectAndSync("sim", { model: simModel, fault: simFault }).catch(() => {})}
              data-action="connect-sim"
              data-testid="btn-connect-sim"
            >
              Start simulator
            </Button>
          </Card>
        )}

        <p className="px-1 text-xs text-muted leading-relaxed">
          Needs Chrome or Edge with Web Bluetooth, WebUSB, or Web Serial, and a compatible Bluetooth gateway or SM-PCE02 adapter.{" "}
          <Link href={DOCS_URL} target="_blank" rel="noreferrer" className="text-xs">
            Requirements and limitations
            <Link.Icon />
          </Link>
        </p>
        <Button variant="ghost" size="sm" onPress={onOpenWarning} data-testid="btn-open-warning-modal" className="self-start max-w-full h-auto whitespace-normal text-left">
          Unofficial software &amp; hardware safety notice
        </Button>
      </div>
    </main>
  );
}

function TransportSupport({ platform, bluetooth }: { platform: PlatformInfo; bluetooth: boolean }) {
  const rows = [
    { label: "Bluetooth", ok: bluetooth },
    { label: "USB", ok: platform.supportsUsb },
    { label: "Serial", ok: platform.supportsSerial },
  ];
  return (
    <ul className="flex flex-wrap gap-2" aria-label="Connections this browser supports">
      {rows.map((row) => (
        <li key={row.label}>
          <Chip size="sm" variant="soft" color={row.ok ? "success" : "default"}>
            {row.ok ? <Check size={12} aria-hidden="true" /> : <Minus size={12} aria-hidden="true" />}
            {row.label}
            <span className="sr-only">{row.ok ? " supported" : " not supported in this browser"}</span>
          </Chip>
        </li>
      ))}
    </ul>
  );
}
