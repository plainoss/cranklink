/**
 * BackupView.tsx - Restore points grouped by the operation that created them.
 */

import { useEffect, useState } from "react";
import { Archive, Download, FileText, Plus, RotateCcw } from "lucide-react";
import type { InstrumentState, JournalSnapshot } from "../../types.ts";
import type { SessionController } from "../../core/sessionController.ts";
import { formatSetting, settingLabel } from "../../domain/settingFormat.ts";
import { Button, Card, Disclosure, toast } from "@heroui/react";
import { ConfirmDialog } from "../ui/ConfirmDialog.tsx";

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
    const restorable = files.filter((file) =>
      file.kind === "setting-backup"
      && Boolean(state.identity)
      && file.serial?.toUpperCase() === state.identity?.serial.toUpperCase()
      && (file.settingsModel === state.identity?.modelSlug || (state.identity?.isMotor && !file.settingsModel)));
    const lines = restorable.map((file) =>
      file.setting !== undefined
        ? `${settingLabel(state.schema, file.setting)}: ${formatSetting(state.schema, file.setting, file.before)}`
        : file.summary);
    const title = files[0].kind === "firmware"
      ? "Firmware image"
      : files.some((file) => file.kind === "full-snapshot")
        ? "Configuration snapshot (download only)"
        : restorable.length > 0
          ? GROUP_TITLES.find(([pattern]) => pattern.test(key))?.[1] ?? "Stored file"
          : files.some((file) => file.kind === "setting-backup")
            ? "Backup for another component"
            : "Operation log (nothing to restore)";
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
  useEffect(() => setPending(null), [state.identity?.slot, state.identity?.serial, state.identity?.modelSlug]);
  const points = restorePoints(state);
  const busy = state.phase !== "ready" || state.linkLost || !state.identity;

  const restore = async () => {
    if (!pending) return;
    const files = pending.restorable.map((file) => file.filename);
    setPending(null);
    await controller.restoreBackupFiles(files).catch(() => {});
  };

  return (
    <div className="w-full flex flex-col gap-6" data-testid="journal-section">
      <div className="flex flex-col sm:flex-row sm:items-end justify-between gap-4">
        <p className="text-sm text-muted max-w-prose">
          cranklink saves the previous value of every setting before writing it. Restore points are kept in this browser;
          download them to keep a copy elsewhere. Full configuration snapshots are for download; restore uses the backups created before a write. Active component: {state.identity?.model}, serial {state.identity?.serial}.
        </p>
        <Button isDisabled={busy || state.schema.length === 0} onPress={() => controller.captureManualSnapshot().catch(() => {})} data-testid="btn-capture-snapshot" className="shrink-0">
          <Plus size={16} aria-hidden="true" />
          Back up all settings now
        </Button>
      </div>

      {points.length === 0 ? (
        <Card variant="transparent" className="items-center text-center py-12 border border-dashed border-separator">
          <Archive size={32} className="text-muted" aria-hidden="true" />
          <Card.Title className="text-sm font-semibold">No restore points yet</Card.Title>
          <Card.Description className="text-xs max-w-sm">
            They appear here automatically after you save settings, or when you back up all settings.
          </Card.Description>
        </Card>
      ) : (
        <ul className="flex flex-col gap-3" aria-label="Restore points">
          {points.map((point) => (
            <li key={point.key}>
              <Card className="gap-3" data-testid="restore-point">
                <Card.Header className="p-0 flex flex-col sm:flex-row sm:items-start justify-between gap-3">
                  <div className="min-w-0">
                    <Card.Title className="text-sm font-semibold">{point.title}</Card.Title>
                    <Card.Description className="text-xs">
                      {point.createdAt ? <time dateTime={point.createdAt.toISOString()}>{dateFormat.format(point.createdAt)}</time> : "Date unknown"}
                    </Card.Description>
                    <ul className="mt-2 text-xs flex flex-col gap-0.5">
                      {point.lines.map((line) => (
                        <li key={line} className="[overflow-wrap:anywhere]">{line}</li>
                      ))}
                    </ul>
                  </div>
                  {point.restorable.length > 0 && (
                    <Button variant="outline" isDisabled={busy} onPress={() => {
                      toast.clear();
                      setPending(point);
                    }} data-action="restore-snapshot" className="shrink-0 self-start">
                      <RotateCcw size={14} aria-hidden="true" />
                      Restore these values
                    </Button>
                  )}
                </Card.Header>
                <Disclosure>
                  <Disclosure.Heading>
                    <Disclosure.Trigger className="text-xs text-muted flex items-center gap-1 min-h-[32px]">
                      {point.files.length} file{point.files.length === 1 ? "" : "s"}
                      <Disclosure.Indicator />
                    </Disclosure.Trigger>
                  </Disclosure.Heading>
                  <Disclosure.Content>
                    <Disclosure.Body>
                      <ul className="flex flex-col divide-y divide-separator border border-separator rounded-xl text-xs">
                        {point.files.map((file) => (
                          <li key={file.filename} className="flex items-center justify-between gap-3 px-3 py-2">
                            <span className="flex items-center gap-2 min-w-0">
                              <FileText size={14} className="shrink-0 text-muted" aria-hidden="true" />
                              <span className="font-mono [overflow-wrap:anywhere]">{file.filename}</span>
                              <span className="shrink-0 text-muted">{(file.sizeBytes / 1024).toFixed(1)} KiB</span>
                            </span>
                            <Button
                              variant="ghost"
                              size="sm"
                              onPress={() => controller.downloadVirtualFile(file.filename)}
                              aria-label={`Download ${file.filename}`}
                              className="shrink-0"
                            >
                              <Download size={14} aria-hidden="true" />
                              Download
                            </Button>
                          </li>
                        ))}
                      </ul>
                    </Disclosure.Body>
                  </Disclosure.Content>
                </Disclosure>
              </Card>
            </li>
          ))}
        </ul>
      )}

      <ConfirmDialog
        isOpen={pending !== null}
        onOpenChange={(open) => !open && setPending(null)}
        title={`Restore these values to ${state.identity?.model ?? "the selected component"}?`}
        actions={[{ label: "Restore", onPress: restore, "data-testid": "btn-confirm-restore" }]}
      >
        <p className="text-sm text-muted mb-3">The current values are backed up first, so this can be undone.</p>
        <ul className="text-sm flex flex-col gap-1 rounded-xl border border-separator p-3">
          {pending?.lines.map((line) => <li key={line}>{line}</li>)}
        </ul>
      </ConfirmDialog>
    </div>
  );
}
