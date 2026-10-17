import { useState } from "react";
import {
  ArrowLeft,
  ShieldCheck,
  Download,
  RotateCcw,
  Plus,
  FileText,
  Clock,
  HardDrive,
  AlertTriangle,
  Check,
} from "lucide-react";
import type { InstrumentState, JournalSnapshot } from "../../types.ts";
import type { SessionController } from "../../core/sessionController.ts";
import { Card } from "../ui/Card.tsx";
import { Button } from "../ui/Button.tsx";
import { Badge } from "../ui/Badge.tsx";
import { Modal } from "../ui/Dialog.tsx";

interface BackupViewProps {
  state: InstrumentState;
  controller: SessionController;
  onBack?: () => void;
}

export function BackupView({ state, controller, onBack }: BackupViewProps) {
  const [selectedSnapshot, setSelectedSnapshot] = useState<JournalSnapshot | null>(null);
  const [restoreModalOpen, setRestoreModalOpen] = useState(false);

  const handleCapture = () => {
    controller.captureManualSnapshot().catch(() => {});
  };

  const confirmRestore = (snapshot: JournalSnapshot) => {
    setSelectedSnapshot(snapshot);
    setRestoreModalOpen(true);
  };

  const executeRestore = async () => {
    if (!selectedSnapshot) return;
    setRestoreModalOpen(false);
    try {
      await controller.restoreBackupFile(selectedSnapshot.filename);
    } catch {
      // Error in state.errorBanner
    }
  };

  return (
    <div
      className="w-full max-w-4xl mx-auto flex flex-col gap-6 py-4"
      data-testid="journal-section"
    >
      {/* Top action row */}
      <div className="flex items-center justify-between">
        {onBack ? (
          <button
            type="button"
            onClick={onBack}
            className="inline-flex items-center gap-2 text-xs font-semibold text-stone-600 hover:text-stone-900 cursor-pointer select-none py-1.5 px-3 rounded-xl hover:bg-stone-200/60 transition-colors"
            data-testid="btn-back-overview"
          >
            <ArrowLeft size={15} />
            <span>Back to Overview</span>
          </button>
        ) : (
          <span className="text-xs font-mono font-medium text-stone-500">Backup & Restore</span>
        )}

        <Badge variant="neutral">
          {state.snapshots.length} Backup{state.snapshots.length !== 1 ? "s" : ""} Stored
        </Badge>
      </div>

      {/* Header */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
        <div>
          <h2 className="text-xl sm:text-2xl font-bold tracking-tight text-stone-900 font-sans flex items-center gap-2.5">
            <ShieldCheck className="text-emerald-700" size={24} />
            <span>Backup & Restore Hub</span>
          </h2>
          <p className="text-xs sm:text-sm text-stone-600 mt-1">
            Browse automatic pre-write snapshots or take a complete snapshot of your bike's configuration.
          </p>
        </div>

        <Button
          variant="primary"
          size="md"
          onClick={handleCapture}
          data-testid="btn-capture-snapshot"
          className="flex items-center gap-2 shrink-0 bg-stone-900 text-white text-xs"
        >
          <Plus size={16} />
          <span>Create Backup Now</span>
        </Button>
      </div>

      {/* Backups List Card */}
      <Card className="flex flex-col gap-4 border-stone-200">
        <div className="flex items-center justify-between">
          <h4 className="text-sm font-bold text-stone-900 flex items-center gap-2">
            <HardDrive size={18} className="text-stone-700" />
            <span>Saved Restore Points & Transaction Journals</span>
          </h4>
        </div>

        {state.snapshots.length === 0 ? (
          <div className="text-center py-12 px-4 border border-dashed border-stone-200 rounded-2xl bg-stone-50/50">
            <ShieldCheck size={36} className="text-stone-400 mx-auto mb-2" />
            <p className="text-sm font-semibold text-stone-700">No Backups Yet</p>
            <p className="text-xs text-stone-500 mt-0.5 max-w-sm mx-auto">
              Whenever you write settings or update firmware, cranklink will automatically create a restore point here.
            </p>
          </div>
        ) : (
          <div className="divide-y divide-stone-200 border border-stone-200 rounded-2xl overflow-hidden bg-white">
            {state.snapshots.map((item) => {
              const isRestorable =
                item.kind === "setting-backup" || item.kind === "full-snapshot";

              return (
                <div
                  key={item.filename}
                  className="p-4 flex flex-col sm:flex-row sm:items-center justify-between gap-4 hover:bg-stone-50/80 transition-colors"
                >
                  <div className="flex items-start gap-3">
                    <div className="w-9 h-9 rounded-xl bg-stone-100 text-stone-700 flex items-center justify-center shrink-0 mt-0.5">
                      <FileText size={18} />
                    </div>
                    <div className="flex flex-col gap-0.5">
                      <div className="flex items-center gap-2 flex-wrap">
                        <span className="text-xs font-semibold text-stone-900 font-mono">
                          {item.filename}
                        </span>
                        <Badge
                          variant={
                            item.kind === "full-snapshot"
                              ? "success"
                              : item.kind === "setting-backup"
                                ? "warning"
                                : "neutral"
                          }
                        >
                          {item.kind}
                        </Badge>
                      </div>
                      <p className="text-xs text-stone-600 font-sans">
                        {item.summary}
                      </p>
                      <span className="text-[11px] text-stone-400 font-mono flex items-center gap-1.5 mt-0.5">
                        <Clock size={11} />
                        <span>{item.timestamp}</span>
                        <span>•</span>
                        <span>{(item.sizeBytes / 1024).toFixed(1)} KiB</span>
                      </span>
                    </div>
                  </div>

                  <div className="flex items-center gap-2 shrink-0 self-end sm:self-center">
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => controller.downloadVirtualFile(item.filename)}
                      title="Download file to computer"
                      className="text-xs flex items-center gap-1.5"
                    >
                      <Download size={14} />
                      <span>Download</span>
                    </Button>

                    {isRestorable && (
                      <Button
                        variant="secondary"
                        size="sm"
                        onClick={() => controller.restoreBackupFile(item.filename).catch(() => {})}
                        data-action="restore-snapshot"
                        className="text-xs flex items-center gap-1.5 bg-stone-100 hover:bg-stone-200 text-stone-900 font-medium min-h-[44px]"
                      >
                        <RotateCcw size={14} />
                        <span>Restore to Bike</span>
                      </Button>
                    )}
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </Card>

      {/* Confirmation Modal for Restore */}
      <Modal
        open={restoreModalOpen}
        onOpenChange={setRestoreModalOpen}
        title="Restore Configuration to Bike"
        description="Are you sure you want to restore this configuration into your drive unit?"
        footer={
          <>
            <Button
              variant="outline"
              size="md"
              onClick={() => setRestoreModalOpen(false)}
            >
              Cancel
            </Button>
            <Button
              variant="primary"
              size="md"
              onClick={executeRestore}
              className="bg-stone-900 hover:bg-black text-white flex items-center gap-2"
            >
              <Check size={16} />
              <span>Confirm & Restore</span>
            </Button>
          </>
        }
      >
        <div className="flex flex-col gap-3 text-xs text-stone-700 leading-relaxed">
          <p>
            Target file: <strong className="font-mono text-stone-900">{selectedSnapshot?.filename}</strong>
          </p>
          <div className="p-3.5 rounded-2xl bg-amber-50 border border-amber-200 text-amber-900 flex items-start gap-2.5">
            <AlertTriangle size={18} className="text-amber-700 shrink-0 mt-0.5" />
            <div>
              Before applying this restore, cranklink will automatically create a rollback snapshot of your currently active configuration.
            </div>
          </div>
        </div>
      </Modal>
    </div>
  );
}
