/**
 * BackupView.tsx - Restore points grouped by the operation that created them.
 */

import { useState } from "react";
import { Archive, Download, FileText, Plus, RotateCcw } from "lucide-react";
import type { InstrumentState, JournalSnapshot } from "../../types.ts";
import type { SessionController } from "../../core/sessionController.ts";
import { formatSetting, settingLabel } from "../../domain/settingFormat.ts";
import { Button } from "../ui/Button.tsx";
import { Modal } from "../ui/Dialog.tsx";

interface BackupViewProps {
  state: InstrumentState;
  controller: SessionController;
}

interface RestorePoint {
  key: string;
  title: string;
  createdAt: Date | null;
  /** One line per setting or snapshot the point would restore. */
  lines: string[];
  restorable: JournalSnapshot[];
  files: JournalSnapshot[];
}

const GROUP_TITLES: Array<[RegExp, string]> = [
  [/^tx_/, "Values before a save"],
  [/^pre_restore_/, "Values before a restore"],
  [/^snapshot_/, "Full backup"],
  [/^fw_/, "Firmware operation log"],
];

function restorePoints(state: InstrumentState): RestorePoint[] {
  const groups = new Map<string, JournalSnapshot[]>();
  for (const snapshot of state.snapshots) {
    const key = snapshot.kind === "firmware" ? `firmware:${snapshot.filename}` : snapshot.group;
    groups.set(key, [...(groups.get(key) ?? []), snapshot]);
  }
  return [...groups.entries()].map(([key, files]) => {
    const restorable = files.filter((file) => file.kind === "setting-backup" || file.kind === "full-snapshot");
    const lines = restorable.map((file) =>
      file.setting !== undefined
        ? `${settingLabel(state.schema, file.setting)}: ${formatSetting(state.schema, file.setting, file.before)}`
        : file.summary);
    const title = files[0].kind === "firmware"
      ? "Firmware image"
      : restorable.length === 0
        ? "Operation log (nothing to restore)"
        : GROUP_TITLES.find(([pattern]) => pattern.test(key))?.[1] ?? "Stored file";
    return {
      key,
      title,
      createdAt: files.find((file) => file.createdAt)?.createdAt ?? null,
      lines: lines.length ? lines : files.map((file) => file.summary),
      restorable,
      files,
    };
  });
}

const dateFormat = new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" });

export function BackupView({ state, controller }: BackupViewProps) {
  const [pending, setPending] = useState<RestorePoint | null>(null);
  const points = restorePoints(state);
  const busy = state.phase !== "ready";

  const restore = async () => {
    if (!pending) return;
    const files = pending.restorable.map((file) => file.filename);
    setPending(null);
    await controller.restoreBackupFiles(files).catch(() => {});
  };

  return (
    <div className="w-full flex flex-col gap-6" data-testid="journal-section">
      <div className="flex flex-col sm:flex-row sm:items-end justify-between gap-4">
        <p className="text-sm text-stone-600 max-w-prose">
          cranklink saves the previous value of every setting before writing it. Restore points are kept in this browser;
          download them to keep a copy elsewhere.
        </p>
        <Button
          variant="primary"
          size="md"
          disabled={busy}
          onClick={() => controller.captureManualSnapshot().catch(() => {})}
          data-testid="btn-capture-snapshot"
          className="gap-2 shrink-0"
        >
          <Plus size={16} aria-hidden="true" />
          Back up all settings now
        </Button>
      </div>

      {points.length === 0 ? (
        <div className="text-center py-12 px-4 border border-dashed border-stone-300 rounded-3xl bg-white/60">
          <Archive size={32} className="text-stone-400 mx-auto mb-2" aria-hidden="true" />
          <p className="text-sm font-semibold text-stone-700">No restore points yet</p>
          <p className="text-xs text-stone-500 mt-1 max-w-sm mx-auto">
            They appear here automatically after you save settings, or when you back up all settings.
          </p>
        </div>
      ) : (
        <ul className="flex flex-col gap-3" aria-label="Restore points">
          {points.map((point) => (
            <li key={point.key} className="bg-white border border-stone-200 rounded-2xl p-4 flex flex-col gap-3" data-testid="restore-point">
              <div className="flex flex-col sm:flex-row sm:items-start justify-between gap-3">
                <div className="min-w-0">
                  <h2 className="text-sm font-semibold text-stone-900">{point.title}</h2>
                  <p className="text-xs text-stone-500">
                    {point.createdAt ? <time dateTime={point.createdAt.toISOString()}>{dateFormat.format(point.createdAt)}</time> : "Date unknown"}
                  </p>
                  <ul className="mt-2 text-xs text-stone-700 flex flex-col gap-0.5">
                    {point.lines.map((line) => (
                      <li key={line} className="[overflow-wrap:anywhere]">{line}</li>
                    ))}
                  </ul>
                </div>
                {point.restorable.length > 0 && (
                  <Button
                    variant="outline"
                    size="sm"
                    disabled={busy}
                    onClick={() => setPending(point)}
                    data-action="restore-snapshot"
                    className="gap-1.5 shrink-0 self-start min-h-[44px]"
                  >
                    <RotateCcw size={14} aria-hidden="true" />
                    Restore these values
                  </Button>
                )}
              </div>
              <details className="text-xs">
                <summary className="cursor-pointer select-none text-stone-600 hover:text-stone-900 min-h-[32px] flex items-center">
                  {point.files.length} file{point.files.length === 1 ? "" : "s"}
                </summary>
                <ul className="mt-2 flex flex-col divide-y divide-stone-100 border border-stone-200 rounded-xl">
                  {point.files.map((file) => (
                    <li key={file.filename} className="flex items-center justify-between gap-3 px-3 py-2">
                      <span className="flex items-center gap-2 min-w-0">
                        <FileText size={14} className="shrink-0 text-stone-500" aria-hidden="true" />
                        <span className="font-mono text-stone-700 [overflow-wrap:anywhere]">{file.filename}</span>
                        <span className="shrink-0 text-stone-400">{(file.sizeBytes / 1024).toFixed(1)} KiB</span>
                      </span>
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() => controller.downloadVirtualFile(file.filename)}
                        aria-label={`Download ${file.filename}`}
                        className="gap-1 shrink-0 min-h-[40px]"
                      >
                        <Download size={14} aria-hidden="true" />
                        Download
                      </Button>
                    </li>
                  ))}
                </ul>
              </details>
            </li>
          ))}
        </ul>
      )}

      <Modal
        open={pending !== null}
        onOpenChange={(open) => !open && setPending(null)}
        title="Restore these values to the bike?"
        description="The current values are backed up first, so this can be undone."
        footer={
          <>
            <Button variant="outline" size="md" onClick={() => setPending(null)}>
              Cancel
            </Button>
            <Button variant="primary" size="md" onClick={restore} data-testid="btn-confirm-restore">
              Restore
            </Button>
          </>
        }
      >
        <ul className="text-sm text-stone-800 flex flex-col gap-1 rounded-xl border border-stone-200 p-3">
          {pending?.lines.map((line) => <li key={line}>{line}</li>)}
        </ul>
      </Modal>
    </div>
  );
}
