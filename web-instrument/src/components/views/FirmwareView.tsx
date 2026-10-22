import { useEffect, useState, useRef, type DragEvent } from "react";
import { UploadCloud, FileCheck2 } from "lucide-react";
import { Alert, Button, Card, Chip, Disclosure, Link } from "@heroui/react";
import type { InstrumentState } from "../../types.ts";
import type { SessionController } from "../../core/sessionController.ts";
import { ConfirmDialog } from "../ui/ConfirmDialog.tsx";

type ProgramTarget = "pair" | "dcas" | "rx";

const RECOVERY_URL = "https://github.com/plainoss/cranklink#recover-a-motor-stuck-in-bootloader-mode";

const PROGRAM_COPY: Record<ProgramTarget, { title: string; body: string }> = {
  pair: {
    title: "Program both microcontrollers?",
    body: "This erases and rewrites DCAS and RX firmware. Keep the bike powered and connected until it finishes.",
  },
  dcas: {
    title: "Force-program DCAS only?",
    body: "This erases and rewrites only DCAS firmware. It cannot reach a DCAS stuck in its bootloader. Programming one side out of step with the other can leave the motor unusable until both match.",
  },
  rx: {
    title: "Force-program RX only?",
    body: "This erases and rewrites only RX firmware. It cannot reach an RX stuck in its bootloader. Programming one side out of step with the other can leave the motor unusable until both match.",
  },
};
interface FirmwareViewProps {
  state: InstrumentState;
  controller: SessionController;
  onOpenWarning: () => void;
}

export function FirmwareView({ state, controller, onOpenWarning }: FirmwareViewProps) {
  const fileInputRef = useRef<HTMLInputElement>(null);
  const [dragOver, setDragOver] = useState(false);
  const [confirming, setConfirming] = useState<ProgramTarget | null>(null);
  useEffect(() => setConfirming(null), [state.identity?.slot, state.identity?.serial, state.identity?.modelSlug]);

  const { firmware } = state;
  const isFlashing = state.phase === "flashing";

  if (!state.identity?.isMotor) {
    return (
      <Card className="gap-2" data-testid="component-firmware-unavailable">
        <Card.Title className="text-base font-bold">Firmware tools require a supported drive unit</Card.Title>
        <Card.Description className="text-sm">Firmware programming is unavailable for {state.identity?.model ?? "the selected component"}. Choose a drive unit from the active component list to use firmware tools.</Card.Description>
      </Card>
    );
  }

  // The same preconditions executeFirmwareSync enforces for a write.
  const canProgram = firmware.companionValid && firmware.dryRunPassed && firmware.dryRunSerial === state.identity.serial
    && !isFlashing && !state.linkLost;
  // Over Bluetooth the CLI runs only the ble-pair relay and refuses one MCU alone.
  const wired = state.transport === "usb" || state.transport === "serial";

  const handleFiles = (files: FileList | null) => {
    if (files && files.length > 0) {
      controller.importFirmwarePackageFiles(files).catch(() => {});
    }
  };

  const onDrop = (e: DragEvent<HTMLDivElement>) => {
    e.preventDefault();
    setDragOver(false);
    if (e.dataTransfer?.files) {
      handleFiles(e.dataTransfer.files);
    }
  };

  const program = () => {
    const target = confirming;
    setConfirming(null);
    if (!target) return;
    controller.executeFirmwareSync(true, target === "pair" ? null : target).catch(() => {});
  };

  const fileSlot = (label: string, file: typeof firmware.dcasFile, expected: string) => (
    <Card variant="secondary" className="gap-1 p-3">
      <div className="flex items-center justify-between text-xs">
        <span className="font-semibold">{label}</span>
        <Chip size="sm" variant="soft" color={file ? "success" : "default"}>
          {file ? "Loaded" : "Missing"}
        </Chip>
      </div>
      <p className="text-[11px] text-muted font-mono truncate">{file ? file.filename : `Waiting for ${expected}`}</p>
    </Card>
  );

  return (
    <div className="w-full flex flex-col gap-6" data-testid="firmware-section">
      <div className="flex flex-col sm:flex-row sm:items-start justify-between gap-3">
        <p className="text-sm text-muted leading-relaxed max-w-prose">
          These drive units run two microcontrollers, DCAS (-D) and RX (-M). Load a matched pair of firmware files, run the
          non-erasing check, then program both together.
        </p>
        <Chip variant="soft" color={firmware.companionValid ? "success" : "default"} className="shrink-0 self-start">
          {firmware.companionValid ? "Pair verified" : "Waiting for files"}
        </Chip>
      </div>

      <Alert status="warning" data-testid="fw-risk-notice">
        <Alert.Indicator />
        <Alert.Content>
          <Alert.Description className="text-xs leading-relaxed">
            <strong>Experimental:</strong> firmware programming is tested on EP800-CRG only, and firmware compatibility is not
            verified.
          </Alert.Description>
          <Button variant="ghost" size="sm" onPress={onOpenWarning} data-testid="btn-review-fw-warning" className="mt-1 -ml-2 self-start">
            Read the risk notice
          </Button>
        </Alert.Content>
      </Alert>

      <Card
        data-dropzone="true"
        onDragOver={(e: DragEvent<HTMLDivElement>) => {
          e.preventDefault();
          setDragOver(true);
        }}
        onDragLeave={() => setDragOver(false)}
        onDrop={onDrop}
        className={`items-center text-center border-2 border-dashed p-8 transition-colors ${dragOver ? "border-warning bg-warning-soft" : "border-separator"}`}
      >
        {/* The browser's file picker; HeroUI's Button opens it. */}
        <input
          ref={fileInputRef}
          type="file"
          multiple
          accept=".dat,.bin"
          className="sr-only"
          tabIndex={-1}
          aria-hidden="true"
          onChange={(e) => handleFiles(e.target.files)}
        />
        <UploadCloud size={28} className="text-muted" aria-hidden="true" />
        <Card.Title className="text-sm sm:text-base font-semibold">Drop the -D and -M firmware files here</Card.Title>
        <Card.Description className="text-xs max-w-md">
          Both files are checked for compatibility with each other and their SHA-256 digests are shown.
        </Card.Description>
        <Button variant="outline" size="sm" onPress={() => fileInputRef.current?.click()}>
          Choose files…
        </Button>
      </Card>

      <Card className="gap-4">
        <Card.Title className="text-sm font-bold flex items-center gap-2">
          <FileCheck2 size={18} aria-hidden="true" />
          Pair check
        </Card.Title>
        <Alert status={firmware.companionValid ? "success" : "default"}>
          <Alert.Content>
            <Alert.Description className="text-xs font-mono leading-relaxed" data-testid="companion-status">
              {firmware.companionMessage}
            </Alert.Description>
          </Alert.Content>
        </Alert>
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
          {fileSlot("DCAS MCU (-D)", firmware.dcasFile, "DUEP800-D.x.x.x.dat")}
          {fileSlot("Renesas RX MCU (-M)", firmware.rxFile, "DUEP800-M.x.x.x.dat")}
        </div>
        <Card.Footer className="flex flex-col sm:flex-row items-stretch sm:items-center justify-end gap-3 p-0 pt-3 border-t border-separator">
          <Button
            variant="outline"
            isDisabled={!firmware.companionValid || isFlashing || state.linkLost}
            onPress={() => controller.executeFirmwareSync(false).catch(() => {})}
            data-action="fw-dry-run-pair"
            data-testid="btn-fw-dry-run-pair"
          >
            Run non-erasing check
          </Button>
          <Button
            isDisabled={!canProgram}
            onPress={() => setConfirming("pair")}
            data-action="fw-flash-pair"
            data-testid="btn-fw-flash-pair"
          >
            Program both to the bike
          </Button>
        </Card.Footer>
      </Card>

      <Card className="gap-0 py-2" data-testid="single-mcu-override">
        <Disclosure>
          <Disclosure.Heading>
            <Disclosure.Trigger className="w-full flex items-center justify-between text-xs font-semibold min-h-[40px]">
              Program one microcontroller only
              <Disclosure.Indicator />
            </Disclosure.Trigger>
          </Disclosure.Heading>
          <Disclosure.Content>
            <Disclosure.Body className="flex flex-col gap-3 text-xs text-muted leading-relaxed pb-2">
              <p>
                {wired
                  ? "After the non-erasing check passes, you can program DCAS or RX alone from the checked pair."
                  : "Programming DCAS or RX alone needs a wired SM-PCE02 connection; over Bluetooth both are programmed together."}{" "}
                Bootloader recovery is not available in the browser: for a microcontroller stuck in its bootloader, use the cranklink
                CLI with an SM-PCE02 and its <code className="font-mono">--recovery</code> option.{" "}
                <Link href={RECOVERY_URL} target="_blank" rel="noreferrer" className="text-xs">
                  Recovery steps
                  <Link.Icon />
                </Link>
              </p>
              <div className="flex items-center gap-3 flex-wrap">
                <Button variant="danger-soft" size="sm" isDisabled={!canProgram || !wired} onPress={() => setConfirming("dcas")}>
                  Force program DCAS only
                </Button>
                <Button variant="danger-soft" size="sm" isDisabled={!canProgram || !wired} onPress={() => setConfirming("rx")}>
                  Force program RX only
                </Button>
              </div>
            </Disclosure.Body>
          </Disclosure.Content>
        </Disclosure>
      </Card>

      <ConfirmDialog
        isOpen={confirming !== null}
        onOpenChange={(open) => !open && setConfirming(null)}
        title={confirming ? PROGRAM_COPY[confirming].title : ""}
        status={confirming === "pair" ? "warning" : "danger"}
        actions={[
          {
            label: "Program",
            variant: confirming === "pair" ? "primary" : "danger",
            onPress: program,
            "data-testid": "btn-confirm-program",
          },
        ]}
      >
        <p className="text-sm text-muted leading-relaxed">{confirming && PROGRAM_COPY[confirming].body}</p>
      </ConfirmDialog>
    </div>
  );
}
