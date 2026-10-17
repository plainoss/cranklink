import { h } from "preact";
import { useState, useRef } from "preact/hooks";
import {
  ArrowLeft,
  Disc3,
  UploadCloud,
  FileCheck2,
  AlertTriangle,
  CheckCircle2,
  Cpu,
  ShieldAlert,
  Loader2,
} from "lucide-preact";
import type { InstrumentState } from "../../types.ts";
import type { SessionController } from "../../core/sessionController.ts";
import { Card } from "../ui/Card.tsx";
import { Button } from "../ui/Button.tsx";
import { Badge } from "../ui/Badge.tsx";

interface FirmwareViewProps {
  state: InstrumentState;
  controller: SessionController;
  onBack: () => void;
}

export function FirmwareView({ state, controller, onBack }: FirmwareViewProps) {
  const fileInputRef = useRef<HTMLInputElement>(null);
  const [dragOver, setDragOver] = useState(false);

  const { firmware } = state;
  const isFlashing = state.phase === "flashing";

  const handleFiles = (files: FileList | null) => {
    if (files && files.length > 0) {
      controller.importFirmwarePackageFiles(files).catch(() => {});
    }
  };

  const onDrop = (e: DragEvent) => {
    e.preventDefault();
    setDragOver(false);
    if (e.dataTransfer?.files) {
      handleFiles(e.dataTransfer.files);
    }
  };

  return (
    <div
      className="w-full max-w-4xl mx-auto flex flex-col gap-6 py-6 px-4"
      data-testid="firmware-section"
    >
      {/* Navigation */}
      <div className="flex items-center justify-between">
        <button
          type="button"
          onClick={onBack}
          className="inline-flex items-center gap-2 text-sm font-medium text-stone-600 hover:text-stone-900 cursor-pointer select-none py-1.5 px-2.5 rounded-lg hover:bg-stone-200/60 transition-colors"
          data-testid="btn-back-overview"
        >
          <ArrowLeft size={16} />
          <span>Back to Overview</span>
        </button>

        <Badge variant={firmware.companionValid ? "success" : "neutral"}>
          {firmware.companionValid ? "Companion Verified" : "Waiting for Files"}
        </Badge>
      </div>

      {/* Header */}
      <div>
        <h2 className="text-2xl font-bold tracking-tight text-stone-900 font-sans flex items-center gap-2.5">
          <Disc3 className="text-blue-600" size={26} />
          <span>Synchronized Dual-MCU Firmware Update</span>
        </h2>
        <p className="text-sm text-stone-600 mt-1">
          Shimano EP8 drive units run two synchronized microcontrollers: DCAS (D) and Renesas RX (M).
          Firmware must be validated as a matched companion pair before programming.
        </p>
      </div>

      {/* Flashing Progress Locked State */}
      {isFlashing ? (
        <Card className="flex flex-col items-center justify-center py-12 px-6 text-center gap-4 bg-amber-50/50 border-amber-300">
          <div className="w-16 h-16 rounded-full bg-amber-100 text-amber-800 flex items-center justify-center animate-spin">
            <Loader2 size={36} />
          </div>
          <div>
            <h3 className="text-lg font-bold text-stone-900">
              Programming Drive Unit Firmware...
            </h3>
            <p className="text-sm text-amber-800 mt-1 max-w-md font-medium">
              {state.statusMessage}
            </p>
          </div>
          <div className="p-4 rounded-xl bg-amber-100/70 border border-amber-300/80 text-amber-950 text-xs max-w-md text-left flex items-start gap-3">
            <ShieldAlert size={20} className="text-amber-800 shrink-0 mt-0.5" />
            <div>
              <strong className="block font-semibold">Keep Cables Firmly Connected</strong>
              Do not turn off your computer or bike power during the flash cycle. The motor will reboot automatically once verified.
            </div>
          </div>
        </Card>
      ) : (
        <>
          {/* Step 1: Dropzone Container */}
          <div
            data-dropzone="true"
            onDragOver={(e) => {
              e.preventDefault();
              setDragOver(true);
            }}
            onDragLeave={() => setDragOver(false)}
            onDrop={onDrop}
            className={`border-2 border-dashed rounded-2xl p-8 flex flex-col items-center justify-center text-center transition-all cursor-pointer ${
              dragOver
                ? "border-blue-500 bg-blue-50/60 scale-[1.01]"
                : "border-stone-300 bg-white hover:border-stone-400 hover:bg-stone-50/40"
            }`}
            onClick={() => fileInputRef.current?.click()}
          >
            <input
              ref={fileInputRef}
              type="file"
              multiple
              accept=".dat,.bin"
              className="hidden"
              onChange={(e) => handleFiles((e.target as HTMLInputElement).files)}
            />
            <div className="w-12 h-12 rounded-xl bg-blue-50 text-blue-700 flex items-center justify-center mb-3">
              <UploadCloud size={24} />
            </div>
            <h3 className="text-base font-semibold text-stone-900">
              Drag & Drop Firmware Containers (.dat)
            </h3>
            <p className="text-xs text-stone-500 mt-1 max-w-md">
              Drop both the DCAS (-D) and RX (-M) files here to verify pair compatibility and calculate SHA-256 digests.
            </p>
            <Button variant="outline" size="sm" className="mt-4 pointer-events-none text-xs">
              Browse Files...
            </Button>
          </div>

          {/* Validation & Companion Status Card */}
          <Card className="flex flex-col gap-4 border-stone-200">
            <div className="flex items-center justify-between">
              <h4 className="text-sm font-bold text-stone-900 flex items-center gap-2">
                <FileCheck2 size={18} className="text-stone-700" />
                <span>Pair Inspection & Companion Check</span>
              </h4>
            </div>

            <div
              className={`p-3.5 rounded-xl border text-xs font-mono leading-relaxed ${
                firmware.companionValid
                  ? "bg-emerald-50 border-emerald-200 text-emerald-900"
                  : "bg-stone-50 border-stone-200 text-stone-700"
              }`}
              data-testid="companion-status"
            >
              {firmware.companionMessage}
            </div>

            {/* Microcontroller Status Pills */}
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 pt-1">
              <div className="p-3 rounded-lg border border-stone-200 bg-stone-50/50 flex flex-col gap-1">
                <div className="flex items-center justify-between text-xs">
                  <span className="font-semibold text-stone-800">DCAS MCU (-D)</span>
                  <Badge variant={firmware.dcasFile ? "success" : "neutral"}>
                    {firmware.dcasFile ? "Loaded" : "Missing"}
                  </Badge>
                </div>
                <div className="text-[11px] text-stone-500 font-mono truncate">
                  {firmware.dcasFile ? firmware.dcasFile.filename : "Waiting for DUEP800-D.x.x.x.dat"}
                </div>
              </div>

              <div className="p-3 rounded-lg border border-stone-200 bg-stone-50/50 flex flex-col gap-1">
                <div className="flex items-center justify-between text-xs">
                  <span className="font-semibold text-stone-800">Renesas RX MCU (-M)</span>
                  <Badge variant={firmware.rxFile ? "success" : "neutral"}>
                    {firmware.rxFile ? "Loaded" : "Missing"}
                  </Badge>
                </div>
                <div className="text-[11px] text-stone-500 font-mono truncate">
                  {firmware.rxFile ? firmware.rxFile.filename : "Waiting for DUEP800-M.x.x.x.dat"}
                </div>
              </div>
            </div>

            {/* Action Buttons */}
            <div className="flex flex-col sm:flex-row items-center justify-end gap-3 pt-3 border-t border-stone-100">
              <Button
                variant="outline"
                size="md"
                disabled={!firmware.companionValid}
                onClick={() => controller.executeFirmwareSync(false).catch(() => {})}
                data-action="fw-dry-run-pair"
                data-testid="btn-fw-dry-run-pair"
                className="w-full sm:w-auto text-xs"
              >
                Run Non-Erasing Compatibility Check
              </Button>

              <Button
                variant="primary"
                size="md"
                disabled={!firmware.companionValid || !firmware.dryRunPassed}
                onClick={() => controller.executeFirmwareSync(true).catch(() => {})}
                data-action="fw-flash-pair"
                data-testid="btn-fw-flash-pair"
                className="w-full sm:w-auto text-xs bg-blue-600 hover:bg-blue-700 text-white"
              >
                Program Synchronized Pair to Bike
              </Button>
            </div>
          </Card>

          {/* Advanced Single-MCU Recovery Override */}
          <details
            className="rounded-xl border border-stone-200 bg-white p-4 text-xs group"
            data-testid="single-mcu-override"
          >
            <summary className="font-semibold text-stone-700 hover:text-stone-900 cursor-pointer select-none">
              Advanced: Single-MCU Bootloader Recovery Override
            </summary>
            <div className="pt-3 text-stone-600 leading-relaxed border-t border-stone-100 mt-2 flex flex-col gap-3">
              <p>
                If one microcontroller was interrupted during a previous flash and remains in bootloader recovery mode,
                you can target that specific MCU independently.
              </p>
              <div className="flex items-center gap-3">
                <Button
                  variant="outline"
                  size="sm"
                  disabled={!firmware.dcasFile}
                  onClick={() => controller.executeFirmwareSync(true, "dcas").catch(() => {})}
                  className="text-xs"
                >
                  Force Program DCAS Only
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  disabled={!firmware.rxFile}
                  onClick={() => controller.executeFirmwareSync(true, "rx").catch(() => {})}
                  className="text-xs"
                >
                  Force Program RX Only
                </Button>
              </div>
            </div>
          </details>
        </>
      )}


    </div>
  );
}
